use chrono::TimeZone;
use druid::ExtEventSink;
use tokio::net::TcpStream;
use tokio::runtime;
use tokio::time::timeout;

use accord::connection::*;

use accord::packets::*;

use accord::{ENC_TOK_LEN, SECRET_LEN};

use std::net::SocketAddr;

use tokio::sync::{mpsc, oneshot};

use rand::{rngs::OsRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

use rsa::PaddingScheme;
use rsa::PublicKey;

#[derive(Debug)]
pub enum GuiCommand {
    AddMessage(String),
    Connected,
    ConnectionEnded(String),
}

#[derive(Debug)]
pub enum ConnectionHandlerCommand {
    Connect(SocketAddr, String, String),
    Send(String),
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
                    self.connect(&mut rx, addr, username, password, &event_sink).await;
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
        addr: SocketAddr,
        username: String,
        password: String,
        event_sink: &ExtEventSink,
    ) {
        //==================================
        //      Parse args
        //==================================
        println!("Connecting to: {}", addr);
        let socket = if let Ok(Ok(socket)) =
            timeout(std::time::Duration::from_secs(5), TcpStream::connect(addr)).await
        {
            submit_command(
                event_sink,
                GuiCommand::Connected,
            );
            socket
        } else {
            submit_command(
                event_sink,
                GuiCommand::ConnectionEnded("Failed to connect!".to_string()),
            );
            return;
        };

        println!("Connected!");
        let connection = Connection::<ClientboundPacket, ServerboundPacket>::new(socket);
        let (mut reader, mut writer) = connection.split();

        //==================================
        //      Encryption
        //==================================
        println!("Establishing encryption...");
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
                    println!("Encryption step 1 successful");
                    pub_key = rsa::pkcs8::FromPublicKey::from_public_key_der(&pub_key_der).unwrap();
                    assert_eq!(ENC_TOK_LEN, token_.len());
                    token_
                }
                _ => {
                    println!("Encryption failed. Server response: {:?}", p);
                    std::process::exit(1)
                }
            }
        } else {
            println!("Failed to establish encryption");
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
                println!("Encryption handshake successful!");
            }
            Ok(_) => {
                println!("Failed encryption step 2. Server response: {:?}", p);
                std::process::exit(1);
            }
            Err(e) => {
                println!("{}", e);
                std::process::exit(1);
            }
        }

        //==================================
        //      Login
        //==================================
        println!("Logging in...");
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
                    println!("Login successful");
                }
                ClientboundPacket::LoginFailed(m) => {
                    submit_command(event_sink,
                                   GuiCommand::ConnectionEnded(m));
                    return;
                }
                p => {
                    let m = format!("Login failed. Server response: {:?}", p);
                    submit_command(event_sink,
                                   GuiCommand::ConnectionEnded(m));
                    return;
                }
            }
        } else {
            submit_command(event_sink,
                           GuiCommand::ConnectionEnded("Login failed ;/".to_string()));
            return;
        }

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
        'l: loop {
            match reader.read_packet(&secret, nonce_generator.as_mut()).await {
                Ok(Some(ClientboundPacket::Message(Message { text, sender, time }))) => {
                    let time = chrono::Local.timestamp(time as i64, 0);
                    submit_command(
                        event_sink,
                        GuiCommand::AddMessage(format!("{} ({}): {}", sender, time.format("%H:%M %d-%m"), text)),
                    );
                }
                Ok(Some(ClientboundPacket::UserJoined(username))) => {
                    submit_command(event_sink, GuiCommand::AddMessage(format!("{} joined the channel", username)));
                }
                Ok(Some(ClientboundPacket::UserLeft(username))) => {
                    submit_command(event_sink, GuiCommand::AddMessage(format!("{} left the channel", username)));
                }
                Ok(Some(ClientboundPacket::UsersOnline(usernames))) => {
                    let mut s = String::new();
                    s += "-------------\n";
                    s += "Users online:\n";
                    for username in &usernames {
                        s += &format!("  {}\n", username);
                    }
                    s += "-------------";
                    submit_command(event_sink, GuiCommand::AddMessage(s));
                }
                Ok(Some(p)) => {
                    println!("!!Unhandled packet: {:?}", p);
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
                            ConnectionHandlerCommand::Send(s) => {
                                let p = if let Some(command) = s.strip_prefix('/') {
                                    ServerboundPacket::Command(command.to_string())
                                } else {
                                    ServerboundPacket::Message(s.to_string())
                                };
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
        .submit_command(
            druid::Selector::<GuiCommand>::new("gui_command"),
            info,
            druid::Target::Global,
        )
        .unwrap();
}
