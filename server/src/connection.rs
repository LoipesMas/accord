use crate::commands::*;
use accord::connection::*;
use accord::packets::*;
use accord::utils::verify_message;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

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

    // TODO: clean this up
    async fn reading_loop(
        mut reader: ConnectionReader<ServerboundPacket>,
        addr: std::net::SocketAddr,
        connection_sender: Sender<ConnectionCommands>,
        channel_sender: Sender<ChannelCommands>,
    ) {
        let mut username = None;
        let mut secret = None;
        let mut nonce_generator = None;
        loop {
            println!("reading packet");
            match reader.read_packet(&secret, nonce_generator.as_mut()).await {
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
                            ServerboundPacket::EncryptionRequest => {
                                let (otx, orx) = oneshot::channel();
                                channel_sender
                                    .send(ChannelCommands::EncryptionRequest(
                                        connection_sender.clone(),
                                        otx,
                                    ))
                                    .await
                                    .unwrap();
                                let expect_token = orx.await.unwrap();
                                match reader.read_packet(&secret, nonce_generator.as_mut()).await {
                                    Ok(Some(ServerboundPacket::EncryptionConfirm(s, t))) => {
                                        let (otx, orx) = oneshot::channel();
                                        channel_sender
                                            .send(ChannelCommands::EncryptionConfirm(
                                                connection_sender.clone(),
                                                otx,
                                                s.clone(),
                                                t,
                                                expect_token,
                                            ))
                                            .await
                                            .unwrap();
                                        match orx.await.unwrap() {
                                            Ok(s) => {
                                                secret = Some(s.clone());
                                                let mut seed = [0u8; accord::SECRET_LEN];
                                                seed.copy_from_slice(&s);

                                                nonce_generator =
                                                    Some(ChaCha20Rng::from_seed(seed));
                                            }
                                            Err(_) => {
                                                connection_sender
                                                    .send(ConnectionCommands::Close)
                                                    .await
                                                    .ok(); // it's ok if already closed
                                            }
                                        }
                                    }
                                    Ok(_) => {
                                        println!(
                                            "Client sent wrong packet during encryption handshake."
                                        );
                                        connection_sender
                                            .send(ConnectionCommands::Close)
                                            .await
                                            .ok(); // it's ok if already closed
                                    }
                                    Err(_) => {
                                        println!("Error during encryption handshake.");
                                        connection_sender
                                            .send(ConnectionCommands::Close)
                                            .await
                                            .ok(); // it's ok if already closed
                                    }
                                };
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
                    connection_sender.send(ConnectionCommands::Close).await.ok(); // it's ok if already closed
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
        let mut secret = None;
        let mut nonce_generator = None;
        loop {
            if let Some(com) = connection_receiver.recv().await {
                use ConnectionCommands::*;
                match com {
                    Close => break,
                    SetSecret(s) => {
                        secret = s.clone();
                        let mut seed = [0u8; accord::SECRET_LEN];
                        seed.copy_from_slice(&s.unwrap());

                        nonce_generator = Some(ChaCha20Rng::from_seed(seed));
                    }
                    Write(p) => writer
                        .write_packet(p, &secret, nonce_generator.as_mut())
                        .await
                        .unwrap(),
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
