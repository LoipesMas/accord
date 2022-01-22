use tokio::net::TcpListener;

use accord::connection::*;

mod commands;
use commands::*;

use accord::packets::*;
use accord::utils::{verify_message, verify_username};

use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;

//TODO: restructure, maybe use structs for channel etc.
//TODO: use logging crate?
//TODO: encryption?

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
    let mut connected_users: HashMap<std::net::SocketAddr, String> = HashMap::new();
    let mut accounts: HashMap<String, [u8; 32]> = HashMap::new();
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
            LoginAttempt {
                username,
                password,
                addr,
                otx,
            } => {
                let pass_hash = hash_password(password);
                let res;
                if !verify_username(&username) {
                    res = LoginOneshotCommand::Failed("Invalid username!".to_string());
                } else if let Some(pass_hash_existing) = accounts.get(&username) {
                    if &pass_hash == pass_hash_existing {
                        if connected_users.values().any(|u| u == &username) {
                            res = LoginOneshotCommand::Failed("Already logged in.".to_string());
                        } else {
                            println!("Logged in: {}", username);
                            res = LoginOneshotCommand::Success(username.clone());
                        }
                    } else {
                        res = LoginOneshotCommand::Failed("Incorrect password".to_string());
                    }
                } else {
                    accounts.insert(username.clone(), pass_hash);
                    println!("New account: {}", username);
                    res = LoginOneshotCommand::Success(username.clone());
                }
                if let LoginOneshotCommand::Success(_) = res {
                    connected_users.insert(addr, username);
                } else {
                    println!("Logged in: {}", username);
                }
                otx.send(res).unwrap();
            }
            UserJoined(username) => {
                for tx_ in txs.values() {
                    tx_.send(ServerConnectionCommands::Write(
                        ClientboundPacket::UserJoined(username.clone()),
                    ))
                    .await
                    .ok();
                }
            }
            UserLeft(addr) => {
                println!("Connection ended from: {}", addr);
                let username = connected_users
                    .remove(&addr)
                    .unwrap_or_else(|| "".to_string());
                txs.remove(&addr);
                for tx_ in txs.values() {
                    tx_.send(ServerConnectionCommands::Write(
                        ClientboundPacket::UserLeft(username.clone()),
                    ))
                    .await
                    .ok();
                }
            }
            UsersQuery(addr) => {
                let tx = txs
                    .get(&addr)
                    .unwrap_or_else(|| panic!("Wrong reply addr: {}", addr));
                tx.send(ServerConnectionCommands::Write(
                    ClientboundPacket::UsersOnline(connected_users.values().cloned().collect()),
                ))
                .await
                .unwrap();
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
                        // ping
                        ServerboundPacket::Ping => {
                            // pong
                            let com = ServerConnectionCommands::Write(ClientboundPacket::Pong);
                            connection_sender.send(com).await.unwrap();
                        }
                        // User tries to log in
                        ServerboundPacket::Login {
                            username: un,
                            password,
                        } => {
                            if username.is_some() {
                                println!(
                                    "{} tried to log in while already logged in, ignoring.",
                                    un
                                );
                            } else {
                                let (otx, orx) = oneshot::channel();
                                channel_sender
                                    .send(ChannelCommands::LoginAttempt {
                                        username: un.clone(),
                                        password,
                                        addr,
                                        otx,
                                    })
                                    .await
                                    .unwrap();
                                match orx.await.unwrap() {
                                    LoginOneshotCommand::Success(un) => {
                                        connection_sender
                                            .send(ServerConnectionCommands::Write(
                                                ClientboundPacket::LoginAck,
                                            ))
                                            .await
                                            .unwrap();
                                        channel_sender
                                            .send(ChannelCommands::UserJoined(un.clone()))
                                            .await
                                            .unwrap();
                                        username = Some(un);
                                    }
                                    LoginOneshotCommand::Failed(m) => {
                                        connection_sender
                                            .send(ServerConnectionCommands::Write(
                                                ClientboundPacket::LoginFailed(m),
                                            ))
                                            .await
                                            .unwrap();
                                        connection_sender
                                            .send(ServerConnectionCommands::Close)
                                            .await
                                            .unwrap();
                                        break;
                                    }
                                }
                            }
                        }
                        // rest is only for logged in users
                        p => {
                            if username.is_some() {
                                match p {
                                    // User wants to send a message
                                    ServerboundPacket::Message(m) => {
                                        if verify_message(&m) {
                                            let p = ClientboundPacket::Message {
                                                text: m,
                                                sender: username.clone().unwrap(),
                                                time: current_time_as_sec(),
                                            };
                                            channel_sender
                                                .send(ChannelCommands::Write(p))
                                                .await
                                                .unwrap();
                                        } else {
                                            println!("Invalid message from {:?}: {}", username, m);
                                        }
                                    }
                                    // User issued a commend (i.e "/list")
                                    ServerboundPacket::Command(command) => match command.as_str() {
                                        "list" => {
                                            channel_sender
                                                .send(ChannelCommands::UsersQuery(addr))
                                                .await
                                                .unwrap();
                                        }
                                        c => {
                                            let p = ClientboundPacket::Message {
                                                text: format!("Unknown command: {}", c),
                                                sender: "#SERVER#".to_string(),
                                                time: current_time_as_sec(),
                                            };
                                            connection_sender
                                                .send(ServerConnectionCommands::Write(p))
                                                .await
                                                .unwrap();
                                        }
                                    },
                                    p => {
                                        unreachable!("{:?} should have been handled!", p);
                                    }
                                }
                            } else {
                                println!("Someone tried to do something without being logged in");
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

/// Current time since unix epoch in seconds
#[inline]
fn current_time_as_sec() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[inline]
fn hash_password<T: AsRef<[u8]>>(pass: T) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(pass);
    let mut ret = [0; 32];
    ret.copy_from_slice(&hasher.finalize()[..32]);
    ret
}
