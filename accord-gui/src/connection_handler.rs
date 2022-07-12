use chrono::TimeZone;
use druid::ExtEventSink;

use tokio::{
    net::TcpStream,
    runtime,
    sync::{mpsc, oneshot},
    time::timeout,
};

use accord::{connection::*, packets::*, ENC_TOK_LEN, SECRET_LEN};

use std::sync::Arc;

use rand::{rngs::OsRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

use rsa::{PaddingScheme, PublicKey};

use crate::Message as GMessage;

use log::{error, info};

#[derive(Debug)]
pub enum GuiCommand {
    AddMessage(GMessage),
    Connected,
    ConnectionEnded(String),
    SendImage(Arc<Vec<u8>>),
    StoreImage(String, Arc<Vec<u8>>),
    UpdateUserList(Vec<String>),
}

#[derive(Debug)]
pub enum ConnectionHandlerCommand {
    Connect(String, String, String),
    Write(accord::packets::ServerboundPacket),
}

pub struct ConnectionHandler;

impl ConnectionHandler {
    pub fn main_loop(
        self,
        mut rx: mpsc::Receiver<ConnectionHandlerCommand>,
        event_sink: ExtEventSink,
    ) {
        let rt = runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            loop {
                match rx.recv().await {
                    Some(ConnectionHandlerCommand::Connect(addr, username, password)) => {
                        self.connect(&mut rx, addr, username, password, &event_sink)
                            .await;
                    }
                    c => {
                        panic!("Expected ConnectionHandlerCommand::Connect, got {:?}", c);
                    }
                }
            }
        });
    }
    pub async fn connect(
        &self,
        gui_rx: &mut mpsc::Receiver<ConnectionHandlerCommand>,
        addr: String,
        username: String,
        password: String,
        event_sink: &ExtEventSink,
    ) {
        //==================================
        //      Parse args
        //==================================
        info!("Connecting to: {}", addr);
        let socket = if let Ok(Ok(socket)) =
            timeout(std::time::Duration::from_secs(5), TcpStream::connect(addr)).await
        {
            socket
        } else {
            submit_command(
                event_sink,
                GuiCommand::ConnectionEnded("Failed to connect!".to_string()),
            );
            return;
        };

        info!("Connected!");
        let connection = Connection::<ClientboundPacket, ServerboundPacket>::new(socket);
        let (mut reader, mut writer) = connection.split();

        //==================================
        //      Encryption
        //==================================
        info!("Establishing encryption...");
        let secret = None;
        let mut nonce_generator_write = None;
        let mut nonce_generator_read = None;

        // Request encryption
        writer
            .write_packet(
                ServerboundPacket::EncryptionRequest,
                &secret,
                nonce_generator_write.as_mut(),
            )
            .await
            .unwrap();

        // Handle encryption response
        let pub_key: rsa::RsaPublicKey;
        let token = if let Ok(Some(p)) = reader
            .read_packet(&secret, nonce_generator_read.as_mut())
            .await
        {
            match p {
                ClientboundPacket::EncryptionResponse(pub_key_der, token_) => {
                    info!("Encryption step 1 successful");
                    pub_key = rsa::pkcs8::FromPublicKey::from_public_key_der(&pub_key_der).unwrap();
                    assert_eq!(ENC_TOK_LEN, token_.len());
                    token_
                }
                _ => {
                    error!("Encryption failed. Server response: {:?}", p);
                    std::process::exit(1)
                }
            }
        } else {
            error!("Failed to establish encryption");
            std::process::exit(1)
        };

        // Generate secret
        let mut secret = [0u8; SECRET_LEN];
        OsRng.fill(&mut secret);

        // Encrypt and send
        let padding = PaddingScheme::new_pkcs1v15_encrypt();
        let enc_secret = pub_key
            .encrypt(&mut OsRng, padding, &secret[..])
            .expect("failed to encrypt");
        let padding = PaddingScheme::new_pkcs1v15_encrypt();
        let enc_token = pub_key
            .encrypt(&mut OsRng, padding, &token[..])
            .expect("failed to encrypt");
        writer
            .write_packet(
                ServerboundPacket::EncryptionConfirm(enc_secret, enc_token),
                &None,
                nonce_generator_write.as_mut(),
            )
            .await
            .unwrap();

        // From this point onward we assume everything is encrypted
        let secret = Some(secret.to_vec());
        let mut seed = [0u8; accord::SECRET_LEN];
        seed.copy_from_slice(&secret.as_ref().unwrap()[..]);
        nonce_generator_write = Some(ChaCha20Rng::from_seed(seed));
        nonce_generator_read = Some(ChaCha20Rng::from_seed(seed));

        // Expect EncryptionAck (should be encrypted)
        let p = reader
            .read_packet(&secret, nonce_generator_read.as_mut())
            .await;
        match p {
            Ok(Some(ClientboundPacket::EncryptionAck)) => {
                info!("Encryption handshake successful!");
            }
            Ok(_) => {
                error!("Failed encryption step 2. Server response: {:?}", p);
                std::process::exit(1);
            }
            Err(e) => {
                error!("{}", e);
                std::process::exit(1);
            }
        }

        //==================================
        //      Login
        //==================================
        info!("Logging in...");
        writer
            .write_packet(
                ServerboundPacket::Login { username, password },
                &secret,
                nonce_generator_write.as_mut(),
            )
            .await
            .unwrap();

        // Next packet must be login related
        if let Ok(Some(p)) = reader
            .read_packet(&secret, nonce_generator_read.as_mut())
            .await
        {
            match p {
                ClientboundPacket::LoginAck => {
                    info!("Login successful");
                }
                ClientboundPacket::LoginFailed(m) => {
                    submit_command(event_sink, GuiCommand::ConnectionEnded(m));
                    return;
                }
                p => {
                    let m = format!("Login failed. Server response: {:?}", p);
                    submit_command(event_sink, GuiCommand::ConnectionEnded(m));
                    return;
                }
            }
        } else {
            submit_command(
                event_sink,
                GuiCommand::ConnectionEnded("Login failed ;/".to_string()),
            );
            return;
        }
        submit_command(event_sink, GuiCommand::Connected);

        // Get last 50 messages
        writer
            .write_packet(
                ServerboundPacket::FetchMessages(0, 50),
                &secret,
                nonce_generator_write.as_mut(),
            )
            .await
            .unwrap();

        // Get player list on join
        writer
            .write_packet(
                ServerboundPacket::Command("list".to_string()),
                &secret,
                nonce_generator_write.as_mut(),
            )
            .await
            .unwrap();

        // To send close command when tcpstream is closed
        let (tx, rx) = oneshot::channel::<()>();

        tokio::join!(
            Self::reading_loop(reader, tx, secret.clone(), nonce_generator_read, event_sink),
            Self::writing_loop(writer, rx, secret.clone(), nonce_generator_write, gui_rx)
        );
    }

    async fn reading_loop(
        mut reader: ConnectionReader<ClientboundPacket>,
        close_sender: oneshot::Sender<()>,
        secret: Option<Vec<u8>>,
        mut nonce_generator: Option<ChaCha20Rng>,
        event_sink: &ExtEventSink,
    ) {
        let mut user_list = vec![];
        'l: loop {
            match reader.read_packet(&secret, nonce_generator.as_mut()).await {
                Ok(Some(ClientboundPacket::Message(Message {
                    text,
                    sender_id,
                    sender,
                    time,
                }))) => {
                    let time = chrono::Local.timestamp(time as i64, 0);
                    submit_command(
                        event_sink,
                        GuiCommand::AddMessage(GMessage {
                            sender_id,
                            sender,
                            date: time.format("(%H:%M %d-%m)").to_string(),
                            content: text,
                            is_image: false,
                        }),
                    );
                }
                Ok(Some(ClientboundPacket::UserJoined(username))) => {
                    user_list.push(username);
                    submit_command(event_sink, GuiCommand::UpdateUserList(user_list.clone()));
                }
                Ok(Some(ClientboundPacket::UserLeft(username))) => {
                    user_list
                        .iter()
                        .position(|u| *u == username)
                        .map(|p| user_list.remove(p));
                    submit_command(event_sink, GuiCommand::UpdateUserList(user_list.clone()));
                }
                Ok(Some(ClientboundPacket::UsersOnline(usernames))) => {
                    user_list = usernames;
                    submit_command(event_sink, GuiCommand::UpdateUserList(user_list.clone()));
                }
                Ok(Some(ClientboundPacket::ImageMessage(im))) => {
                    use sha2::{Digest, Sha256};
                    let mut hasher = Sha256::new();
                    hasher.update(&im.image_bytes);

                    // Hash to string
                    let hash = hasher.finalize()[..16]
                        .iter()
                        .fold("".to_string(), |accum, item| {
                            accum + &format!("{:02x}", item)
                        });

                    let time = chrono::Local.timestamp(im.time as i64, 0);
                    submit_command(
                        event_sink,
                        GuiCommand::StoreImage(hash.clone(), Arc::new(im.image_bytes)),
                    );
                    let m = GMessage {
                        content: hash,
                        sender_id: im.sender_id,
                        sender: im.sender,
                        date: time.format("(%H:%M %d-%m)").to_string(),
                        is_image: true,
                    };
                    submit_command(event_sink, GuiCommand::AddMessage(m));
                }
                Ok(Some(p)) => {
                    error!("!!Unhandled packet: {:?}", p);
                }
                _ => {
                    submit_command(
                        event_sink,
                        GuiCommand::ConnectionEnded("Connection closed.".to_string()),
                    );
                    close_sender.send(()).unwrap();
                    break 'l;
                }
            }
        }
    }

    async fn writing_loop(
        mut writer: ConnectionWriter<ServerboundPacket>,
        mut close_receiver: oneshot::Receiver<()>,
        secret: Option<Vec<u8>>,
        mut nonce_generator: Option<ChaCha20Rng>,
        gui_rx: &mut mpsc::Receiver<ConnectionHandlerCommand>,
    ) {
        loop {
            tokio::select!(
                r = gui_rx.recv() => {
                    if let Some(c) = r {
                        match c {
                            ConnectionHandlerCommand::Write(p) => {
                                writer.write_packet(p, &secret, nonce_generator.as_mut()).await.unwrap();
                            },
                            c => {
                                panic!("Got unexpected {:?}", c);
                            }
                        }
                    }
                },
                _ = &mut close_receiver => {
                    break;
                }
            );
        }
    }
}

fn submit_command(event_sink: &ExtEventSink, info: GuiCommand) {
    event_sink
        .submit_command(crate::GUI_COMMAND, info, druid::Target::Global)
        .unwrap();
}
