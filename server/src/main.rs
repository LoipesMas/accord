use tokio::net::TcpListener;

use accord::connection::*;

mod commands;
use commands::*;

use accord::packets::*;

use tokio::sync::mpsc::{self, Receiver, Sender};

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:".to_string() + accord::DEFAULT_PORT)
        .await
        .unwrap();

    let (ctx, crx) = mpsc::channel::<ChannelCommands>(32);

    tokio::spawn(channel_loop(crx));

    println!("Server ready!");
    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        let (tx, rx) = mpsc::channel::<ServerConnectionCommands>(32);
        ctx.send(ChannelCommands::NewConnection(tx.clone(), addr))
            .await
            .unwrap();
        let connection = Connection::<ServerboundPacket, ClientboundPacket>::new(socket);
        let (reader, writer) = connection.split();
        tokio::spawn(reading_loop(reader, addr, tx, ctx.clone()));
        tokio::spawn(writing_loop(writer, rx));
    }
}

async fn channel_loop(mut receiver: Receiver<ChannelCommands>) {
    use std::collections::HashMap;
    let mut txs: HashMap<std::net::SocketAddr, Sender<ServerConnectionCommands>> = HashMap::new();
    let mut usernames: HashMap<std::net::SocketAddr, String> = HashMap::new();
    loop {
        use ChannelCommands::*;
        match receiver.recv().await.unwrap() {
            NewConnection(tx, addr) => {
                println!("Connection from: {:?}", addr);
                txs.insert(addr, tx);
            }
            Write(p) => {
                println!("Message: {:?}", &p);
                for tx_ in txs.values() {
                    tx_.send(ServerConnectionCommands::Write(p.clone()))
                        .await
                        .ok();
                }
            }
            UserJoined(username, addr) => {
                for tx_ in txs.values() {
                    tx_.send(ServerConnectionCommands::Write(
                        ClientboundPacket::UserJoined(username.clone()),
                    ))
                    .await
                    .ok();
                }
                usernames.insert(addr, username);
            }
            UserLeft(addr) => {
                println!("Connection ended from: {}", addr);
                let username = usernames.remove(&addr).unwrap_or_else(|| "".to_string());
                txs.remove(&addr);
                for tx_ in txs.values() {
                    tx_.send(ServerConnectionCommands::Write(
                        ClientboundPacket::UserLeft(username.clone()),
                    ))
                    .await
                    .ok();
                }
            }
        }
    }
}

async fn reading_loop(
    mut reader: ConnectionReader<ServerboundPacket>,
    addr: std::net::SocketAddr,
    connection_sender: Sender<ServerConnectionCommands>,
    channel_sender: Sender<ChannelCommands>,
) {
    let mut username = None;
    loop {
        println!("reading packet");
        match reader.read_packet().await {
            Ok(p) => {
                println!("Got packet: {:?}", p);
                if let Some(p) = p {
                    match p {
                        ServerboundPacket::Ping => {
                            let com = ServerConnectionCommands::Write(ClientboundPacket::Pong);
                            connection_sender.send(com).await.unwrap();
                        }
                        ServerboundPacket::Message(m) => {
                            use std::time::{SystemTime, UNIX_EPOCH};
                            let p = ClientboundPacket::Message {
                                text: m,
                                sender: username
                                    .clone()
                                    .expect("Not logged in user tried to send a message"),
                                time: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                            };
                            channel_sender
                                .send(ChannelCommands::Write(p))
                                .await
                                .unwrap();
                        }
                        ServerboundPacket::Login {
                            username: un,
                            password: _,
                        } => {
                            if username.is_some() {
                                println!(
                                    "{} tried to log in while already logged in, ignoring.",
                                    un
                                );
                            } else {
                                println!("{} logged in", un);
                                connection_sender
                                    .send(ServerConnectionCommands::Write(
                                        ClientboundPacket::LoginAck,
                                    ))
                                    .await
                                    .unwrap();
                                channel_sender
                                    .send(ChannelCommands::UserJoined(un.clone(), addr))
                                    .await
                                    .unwrap();
                                username = Some(un);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                channel_sender
                    .send(ChannelCommands::UserLeft(addr))
                    .await
                    .unwrap();
                connection_sender
                    .send(ServerConnectionCommands::Close)
                    .await
                    .unwrap();
                println!("Err: {:?}", e);
                break;
            }
        }
    }
}
async fn writing_loop(
    mut writer: ConnectionWriter<ClientboundPacket>,
    mut connection_receiver: Receiver<ServerConnectionCommands>,
) {
    loop {
        if let Some(com) = connection_receiver.recv().await {
            use ServerConnectionCommands::*;
            match com {
                Close => break,
                Write(p) => writer.write_packet(p).await.unwrap(),
            }
        }
    }
}
