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
        ctx: Sender<ChannelCommand>,
    ) {
        let (tx, rx) = mpsc::channel::<ConnectionCommand>(32);
        log::info!("Connection from: {:?}", addr);
        let connection = Connection::<ServerboundPacket, ClientboundPacket>::new(socket);
        let (reader, writer) = connection.split();
        let reader_wrapped = ConnectionReaderWrapper::new(reader, addr, tx, ctx);
        tokio::spawn(reader_wrapped.spawn_loop());
        let writer_wrapped = ConnectionWriterWrapper::new(writer, rx);
        tokio::spawn(writer_wrapped.spawn_loop());
    }
}

pub struct ConnectionReaderWrapper {
    reader: ConnectionReader<ServerboundPacket>,
    addr: std::net::SocketAddr,
    connection_sender: Sender<ConnectionCommand>,
    channel_sender: Sender<ChannelCommand>,
    user_id: Option<i64>,
    username: Option<String>,
    secret: Option<Vec<u8>>,
    nonce_generator: Option<ChaCha20Rng>,
}

impl ConnectionReaderWrapper {
    fn new(
        reader: ConnectionReader<ServerboundPacket>,
        addr: std::net::SocketAddr,
        connection_sender: Sender<ConnectionCommand>,
        channel_sender: Sender<ChannelCommand>,
    ) -> Self {
        Self {
            reader,
            addr,
            connection_sender,
            channel_sender,
            user_id: None,
            username: None,
            secret: None,
            nonce_generator: None,
        }
    }

    async fn handle_login(&mut self, un: String, password: String) {
        let (otx, orx) = oneshot::channel();
        self.channel_sender
            .send(ChannelCommand::LoginAttempt {
                username: un.clone(),
                password,
                addr: self.addr,
                otx,
                tx: self.connection_sender.clone(),
            })
            .await
            .unwrap();
        match orx.await.unwrap() {
            Ok(response) => {
                let mut response_split = response.split('|');
                self.user_id = Some(response_split.next().unwrap().parse().unwrap());
                self.username = Some(response_split.next().unwrap().parse().unwrap());

                self.connection_sender
                    .send(ConnectionCommand::Write(ClientboundPacket::LoginAck))
                    .await
                    .unwrap();
                self.channel_sender
                    .send(ChannelCommand::UserJoined(self.username.clone().unwrap()))
                    .await
                    .unwrap();
            }
            Err(m) => {
                self.connection_sender
                    .send(ConnectionCommand::Write(ClientboundPacket::LoginFailed(m)))
                    .await
                    .unwrap();
                self.connection_sender
                    .send(ConnectionCommand::Close)
                    .await
                    .unwrap();
            }
        }
    }

    async fn handle_encryption_request(&mut self) {
        use ServerboundPacket::*;
        // To send back the token
        let (otx, orx) = oneshot::channel();
        self.channel_sender
            .send(ChannelCommand::EncryptionRequest(
                self.connection_sender.clone(),
                otx,
            ))
            .await
            .unwrap();

        let expect_token = orx.await.unwrap();

        // Now we expect EncryptionConfirm with encrypted secret and token
        match self
            .reader
            .read_packet(&self.secret, self.nonce_generator.as_mut())
            .await
        {
            Ok(Some(EncryptionConfirm(s, t))) => {
                let (otx, orx) = oneshot::channel();
                self.channel_sender
                    .send(ChannelCommand::EncryptionConfirm(
                        self.connection_sender.clone(),
                        otx,
                        s.clone(),
                        t,
                        expect_token,
                    ))
                    .await
                    .unwrap();

                // Get decrypted secret back from channel
                match orx.await.unwrap() {
                    Ok(s) => {
                        self.secret = Some(s.clone());
                        let mut seed = [0u8; accord::SECRET_LEN];
                        seed.copy_from_slice(&s);

                        self.nonce_generator = Some(ChaCha20Rng::from_seed(seed));
                    }
                    Err(_) => {
                        self.connection_sender
                            .send(ConnectionCommand::Close)
                            .await
                            .ok(); // it's ok if already closed
                    }
                }
            }
            Ok(_) => {
                log::warn!("Client sent wrong packet during encryption handshake.");
                self.connection_sender
                    .send(ConnectionCommand::Close)
                    .await
                    .ok(); // it's ok if already closed
            }
            Err(_) => {
                log::warn!("Error during encryption handshake.");
                self.connection_sender
                    .send(ConnectionCommand::Close)
                    .await
                    .ok(); // it's ok if already closed
            }
        };
    }

    async fn handle_packet(&mut self, packet: ServerboundPacket) {
        use ServerboundPacket::*;
        match packet {
            // ping
            Ping => {
                // pong
                let com = ConnectionCommand::Write(ClientboundPacket::Pong);
                self.connection_sender.send(com).await.unwrap();
            }
            // User tries to log in
            Login {
                username: un,
                password,
            } => {
                if self.username.is_some() {
                    log::warn!("{} tried to log in while already logged in, ignoring.", un);
                } else {
                    self.handle_login(un, password).await;
                }
            }
            // Users requests encryption
            EncryptionRequest => self.handle_encryption_request().await,
            // rest is only for logged in users
            p => {
                if self.username.is_some() {
                    match p {
                        // User wants to send a message
                        Message(m) => {
                            if verify_message(&m) {
                                let p = ClientboundPacket::Message(accord::packets::Message {
                                    sender_id: self.user_id.clone().unwrap(),
                                    sender: self.username.clone().unwrap(),
                                    text: m,
                                    time: current_time_as_sec(),
                                });
                                self.channel_sender
                                    .send(ChannelCommand::Write(p))
                                    .await
                                    .unwrap();
                            } else {
                                log::info!("Invalid message from {:?}: {}", self.username, m);
                            }
                        }
                        // User sends an image
                        ImageMessage(im) => {
                            let p =
                                ClientboundPacket::ImageMessage(accord::packets::ImageMessage {
                                    image_bytes: im,
                                    sender_id: self.user_id.clone().unwrap(),
                                    sender: self.username.clone().unwrap(),
                                    time: current_time_as_sec(),
                                });
                            self.channel_sender
                                .send(ChannelCommand::Write(p))
                                .await
                                .unwrap();
                        }
                        // User issued a commend (i.e "/list")
                        Command(command) => {
                            //TODO: abstract this code more
                            let mut split = command.as_str().split(' ');
                            if let Some(command) = split.next() {
                                match command {
                                    "list" => {
                                        self.channel_sender
                                            .send(ChannelCommand::UsersQuery(self.addr))
                                            .await
                                            .unwrap();
                                    }
                                    "kick" => {
                                        let m = if let Some(target) = split.next() {
                                            let perms = self
                                                .get_perms(self.username.to_owned().unwrap())
                                                .await;
                                            if let Ok(perms) = perms {
                                                if perms.operator {
                                                    self.channel_sender
                                                        .send(ChannelCommand::KickUser(
                                                            target.to_owned(),
                                                        ))
                                                        .await
                                                        .unwrap();
                                                    format!("{} kicked.", target)
                                                } else {
                                                    "Not permitted.".to_owned()
                                                }
                                            } else {
                                                "Error.".to_owned()
                                            }
                                        } else {
                                            "No target provided".to_owned()
                                        };
                                        self.respond(m).await;
                                    }
                                    "ban" => {
                                        self.ban_command(split.next(), true).await;
                                    }
                                    "unban" => {
                                        self.ban_command(split.next(), false).await;
                                    }
                                    "whitelist" => {
                                        self.whitelist_command(split.next(), true).await;
                                    }
                                    "unwhitelist" => {
                                        self.whitelist_command(split.next(), false).await;
                                    }
                                    "set_whitelist" => {
                                        let m = if let Some(arg) = split.next() {
                                            match arg {
                                                "on" | "true" => {
                                                    self.channel_sender
                                                        .send(ChannelCommand::SetWhitelist(true))
                                                        .await
                                                        .unwrap();
                                                    "Whitelist on.".to_string()
                                                }
                                                "off" | "false" => {
                                                    self.channel_sender
                                                        .send(ChannelCommand::SetWhitelist(false))
                                                        .await
                                                        .unwrap();
                                                    "Whitelist off.".to_string()
                                                }
                                                _ => {
                                                    format!("Invalid argument: {}.\nExpected \"on\"/\"off\"", arg)
                                                }
                                            }
                                        } else {
                                            "No argument provided".to_string()
                                        };
                                        self.respond(m).await;
                                    }
                                    "set_allow_new_accounts" => {
                                        let m = if let Some(arg) = split.next() {
                                            match arg {
                                                "on" | "true" => {
                                                    self.channel_sender
                                                        .send(ChannelCommand::SetAllowNewAccounts(
                                                            true,
                                                        ))
                                                        .await
                                                        .unwrap();
                                                    "Allow new accounts on.".to_string()
                                                }
                                                "off" | "false" => {
                                                    self.channel_sender
                                                        .send(ChannelCommand::SetAllowNewAccounts(
                                                            false,
                                                        ))
                                                        .await
                                                        .unwrap();
                                                    "Allow new accounts off.".to_string()
                                                }
                                                _ => {
                                                    format!("Invalid argument: {}.\nExpected \"on\"/\"off\"", arg)
                                                }
                                            }
                                        } else {
                                            "No argument provided".to_string()
                                        };
                                        self.respond(m).await;
                                    }
                                    c => {
                                        self.respond(format!("Unknown command: {}", c)).await;
                                    }
                                }
                            }
                        }
                        FetchMessages(o, n) => {
                            let (otx, orx) = oneshot::channel();
                            self.channel_sender
                                .send(ChannelCommand::FetchMessages(o, n, otx))
                                .await
                                .unwrap();
                            let mut messages = orx.await.unwrap();
                            for m in messages.drain(..).rev() {
                                self.connection_sender
                                    .send(ConnectionCommand::Write(m))
                                    .await
                                    .unwrap();
                            }
                        }
                        p => {
                            unreachable!("{:?} should have been handled!", p);
                        }
                    }
                } else {
                    log::warn!("Someone tried to do something without being logged in");
                }
            }
        };
    }

    async fn spawn_loop(mut self) {
        loop {
            match self
                .reader
                .read_packet(&self.secret, self.nonce_generator.as_mut())
                .await
            {
                Ok(p) => {
                    match p {
                        Some(ServerboundPacket::ImageMessage(_)) => {
                            log::info!("Got image packet");
                        }
                        _ => log::info!("Got packet: {:?}", p),
                    }
                    if let Some(p) = p {
                        self.handle_packet(p).await;
                    }
                }
                Err(e) => {
                    self.channel_sender
                        .send(ChannelCommand::UserLeft(self.addr))
                        .await
                        .unwrap();
                    self.connection_sender
                        .send(ConnectionCommand::Close)
                        .await
                        .ok(); // it's ok if already closed

                    // This "error" is expected
                    if e == "Connection reset by peer" {
                        log::info!("{}", e);
                    } else {
                        log::error!("Err: {:?}", e);
                    }
                    break;
                }
            }
        }
    }

    async fn get_perms(
        &mut self,
        username: String,
    ) -> Result<UserPermissions, oneshot::error::RecvError> {
        let (otx, orx) = oneshot::channel();
        self.channel_sender
            .send(ChannelCommand::CheckPermissions(username, otx))
            .await
            .unwrap();
        orx.await
    }

    async fn ban_command(&mut self, target: Option<&str>, switch: bool) {
        let m = if let Some(target) = target {
            let perms = self.get_perms(self.username.to_owned().unwrap()).await;
            if let Ok(perms) = perms {
                if perms.operator {
                    self.channel_sender
                        .send(ChannelCommand::BanUser(target.to_owned(), switch))
                        .await
                        .unwrap();
                    let prefix = if switch { "" } else { "un" };
                    format!("{} {}banned.", target, prefix)
                } else {
                    "Not permitted.".to_owned()
                }
            } else {
                "Error.".to_owned()
            }
        } else {
            "No target provided".to_owned()
        };
        self.respond(m).await;
    }

    async fn whitelist_command(&mut self, target: Option<&str>, switch: bool) {
        let m = if let Some(target) = target {
            let perms = self.get_perms(self.username.to_owned().unwrap()).await;
            if let Ok(perms) = perms {
                if perms.operator {
                    self.channel_sender
                        .send(ChannelCommand::WhitelistUser(target.to_owned(), switch))
                        .await
                        .unwrap();
                    let prefix = if switch { "" } else { "un" };
                    format!("{} {}whitelisted.", target, prefix)
                } else {
                    "Not permitted.".to_owned()
                }
            } else {
                "Error.".to_owned()
            }
        } else {
            "No target provided".to_owned()
        };
        self.respond(m).await;
    }

    /// Sends `message` to the user of this channel as a reply from the server.
    async fn respond(&mut self, message: String) {
        let p = ClientboundPacket::Message(accord::packets::Message {
            sender_id: 0,
            sender: "#SERVER#".to_string(),
            text: message,
            time: current_time_as_sec(),
        });
        self.connection_sender
            .send(ConnectionCommand::Write(p))
            .await
            .unwrap();
    }
}

pub struct ConnectionWriterWrapper {
    writer: ConnectionWriter<ClientboundPacket>,
    connection_receiver: Receiver<ConnectionCommand>,
    secret: Option<Vec<u8>>,
    nonce_generator: Option<ChaCha20Rng>,
}
impl ConnectionWriterWrapper {
    fn new(
        writer: ConnectionWriter<ClientboundPacket>,
        connection_receiver: Receiver<ConnectionCommand>,
    ) -> Self {
        Self {
            writer,
            connection_receiver,
            secret: None,
            nonce_generator: None,
        }
    }

    async fn spawn_loop(mut self) {
        loop {
            if let Some(com) = self.connection_receiver.recv().await {
                use ConnectionCommand::*;
                match com {
                    Close => break,
                    SetSecret(s) => {
                        self.secret = s.clone();
                        let mut seed = [0u8; accord::SECRET_LEN];
                        seed.copy_from_slice(&s.unwrap());

                        self.nonce_generator = Some(ChaCha20Rng::from_seed(seed));
                    }
                    Write(p) => self
                        .writer
                        .write_packet(p, &self.secret, self.nonce_generator.as_mut())
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
