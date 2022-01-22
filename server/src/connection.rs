use crate::commands::*;
use accord::connection::*;
use accord::packets::*;
use accord::utils::verify_message;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;

// Maybe this shouldn't be a struct?
pub struct ConnectionWrapper;

impl ConnectionWrapper {
    pub async fn spawn(
        socket: tokio::net::TcpStream,
        addr: std::net::SocketAddr,
        ctx: Sender<ChannelCommands>,
    ) {
        let (tx, rx) = mpsc::channel::<ConnectionCommands>(32);
        ctx.send(ChannelCommands::NewConnection(tx.clone(), addr))
            .await
            .unwrap();
        let connection = Connection::<ServerboundPacket, ClientboundPacket>::new(socket);
        let (reader, writer) = connection.split();
        tokio::spawn(Self::reading_loop(reader, addr, tx, ctx));
        tokio::spawn(Self::writing_loop(writer, rx));
    }

    async fn reading_loop(
        mut reader: ConnectionReader<ServerboundPacket>,
        addr: std::net::SocketAddr,
        connection_sender: Sender<ConnectionCommands>,
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
                                let com = ConnectionCommands::Write(ClientboundPacket::Pong);
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
                                                .send(ConnectionCommands::Write(
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
                                                .send(ConnectionCommands::Write(
                                                    ClientboundPacket::LoginFailed(m),
                                                ))
                                                .await
                                                .unwrap();
                                            connection_sender
                                                .send(ConnectionCommands::Close)
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
                                                println!(
                                                    "Invalid message from {:?}: {}",
                                                    username, m
                                                );
                                            }
                                        }
                                        // User issued a commend (i.e "/list")
                                        ServerboundPacket::Command(command) => {
                                            match command.as_str() {
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
                                                        .send(ConnectionCommands::Write(p))
                                                        .await
                                                        .unwrap();
                                                }
                                            }
                                        }
                                        p => {
                                            unreachable!("{:?} should have been handled!", p);
                                        }
                                    }
                                } else {
                                    println!(
                                        "Someone tried to do something without being logged in"
                                    );
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
                        .send(ConnectionCommands::Close)
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
        mut connection_receiver: Receiver<ConnectionCommands>,
    ) {
        loop {
            if let Some(com) = connection_receiver.recv().await {
                use ConnectionCommands::*;
                match com {
                    Close => break,
                    Write(p) => writer.write_packet(p).await.unwrap(),
                }
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
