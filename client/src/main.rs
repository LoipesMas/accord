use chrono::TimeZone;
use std::str::FromStr;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use accord::connection::*;

use accord::packets::*;

use accord::{ENC_TOK_LEN, SECRET_LEN};

use std::net::SocketAddr;

use tokio::sync::oneshot;

use rand::{rngs::OsRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

use rsa::PaddingScheme;
use rsa::PublicKey;

// TODO: config file?

#[tokio::main(flavor = "current_thread")]
async fn main() {
    //==================================
    //      Parse args
    //==================================
    let mut args = std::env::args();
    let addr = SocketAddr::from_str(&format!(
        "{}:{}",
        args.nth(1).unwrap_or_else(|| "127.0.0.1".to_string()),
        accord::DEFAULT_PORT
    ))
    .unwrap();
    println!("Connecting to: {}", addr);
    let socket = TcpStream::connect(addr).await.unwrap();

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
    //      Get credentials
    //==================================
    let mut stdio = tokio::io::stdin();
    let username = loop {
        println!("Username:");
        let mut buf = bytes::BytesMut::new();
        match stdio.read_buf(&mut buf).await {
            Ok(0 | 1) => println!("Username can't be empty!"),
            Ok(l) => {
                if l > 18 {
                    println!("Username too long. (Max 17 characters)");
                    continue;
                }
                let s = String::from_utf8_lossy(buf.strip_suffix(b"\n").unwrap()).to_string();
                if s.chars().any(|c| !c.is_alphanumeric()) {
                    println!("Invalid characters in username.");
                } else {
                    break s;
                }
            }
            Err(e) => println!("Error: {:?}", e),
        };
    };
    let password = loop {
        println!("Password:");
        let mut buf = bytes::BytesMut::new();
        match stdio.read_buf(&mut buf).await {
            Ok(0 | 1) => println!("Password can't be empty!"),
            Ok(_) => {
                let s = String::from_utf8_lossy(buf.strip_suffix(b"\n").unwrap()).to_string();
                if s.chars().any(|c| !c.is_alphanumeric()) {
                    println!("Invalid characters in password.");
                } else {
                    break s;
                }
            }
            Err(e) => println!("Error: {:?}", e),
        };
    };

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
                println!("{}", m);
                std::process::exit(1);
            }
            _ => {
                println!("Login failed. Server response: {:?}", p);
                std::process::exit(1);
            }
        }
    } else {
        println!("Failed to login ;/");
        std::process::exit(1);
    }

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
        reading_loop(reader, tx, secret.clone(), nonce_generator_read),
        writing_loop(writer, rx, secret.clone(), nonce_generator_write)
    );
}

async fn reading_loop(
    mut reader: ConnectionReader<ClientboundPacket>,
    close_sender: oneshot::Sender<()>,
    secret: Option<Vec<u8>>,
    mut nonce_generator: Option<ChaCha20Rng>,
) {
    'l: loop {
        match reader.read_packet(&secret, nonce_generator.as_mut()).await {
            Ok(Some(ClientboundPacket::Message { text, sender, time })) => {
                let time = chrono::Local.timestamp(time as i64, 0);
                println!("{} ({}): {}", sender, time.format("%H:%M %d-%m"), text);
            }
            Ok(Some(ClientboundPacket::UserJoined(username))) => {
                println!("{} joined the channel", username);
            }
            Ok(Some(ClientboundPacket::UserLeft(username))) => {
                println!("{} left the channel", username);
            }
            Ok(Some(ClientboundPacket::UsersOnline(usernames))) => {
                println!("-------------");
                println!("Users online:");
                for username in &usernames {
                    println!("  {}", username);
                }
                println!("-------------");
            }
            Ok(Some(p)) => {
                println!("!!Unhandled packet: {:?}", p);
            }
            Err(e) => {
                println!("{}", e);
                close_sender.send(()).unwrap();
                break 'l;
            }
            _ => {
                println!("Connection closed(?)\nPress Enter to exit.");
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
) {
    let mut stdio = tokio::io::stdin();
    let mut buf = bytes::BytesMut::new();
    loop {
        tokio::select!(
            r = stdio.read_buf(&mut buf) => {
                if r.is_ok() {
                    let s = String::from_utf8_lossy(&buf).to_string();

                    if let Some(s) = s.strip_suffix('\n') {
                        buf.clear();
                        // Clear input line
                        print!("\r\u{1b}[A");
                        if s.chars().any(|c| c.is_control()) {
                            println!("Invalid message text!");
                            continue;
                        }

                        if s.is_empty() {
                            print!("\u{1b}[A\u{1b}[A");
                            continue;
                        }

                        let p = if let Some(command) = s.strip_prefix('/') {
                            ServerboundPacket::Command(command.to_string())
                        } else {
                            ServerboundPacket::Message(s.to_string())
                        };
                        writer.write_packet(p, &secret, nonce_generator.as_mut()).await.unwrap();
                    }
                }
            }
            _ = &mut close_receiver => {
                break;
            }
        );
    }
}
