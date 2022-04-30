use console::InputWindow;
use console::MessageWindow;
use console::UserListWindow;
use console_engine::crossterm::event::KeyEvent;
use console_engine::crossterm::event::MouseEvent;
use console_engine::crossterm::event::MouseEventKind;
use console_engine::pixel;
use console_engine::Color;
use console_engine::ConsoleEngine;
use console_engine::KeyCode;
use console_engine::KeyModifiers;
use std::error::Error;
use std::str::FromStr;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use accord::connection::*;

use accord::packets::*;

use accord::{ENC_TOK_LEN, SECRET_LEN};

use std::net::SocketAddr;

use tokio::sync::{mpsc, oneshot};

use rand::{rngs::OsRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

use rsa::PaddingScheme;
use rsa::PublicKey;

use crate::console::ConsoleMessage;

#[cfg(target_os = "unix")]
const OS_EOL: &[u8] = b"\n";
#[cfg(target_os = "windows")]
const OS_EOL: &[u8] = b"\r\n";

mod console;

// TODO: config file?

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
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
                let s = String::from_utf8_lossy(buf.strip_suffix(OS_EOL).unwrap()).to_string();
                println!("{:?}", s);
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
                let s = String::from_utf8_lossy(buf.strip_suffix(OS_EOL).unwrap()).to_string();
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

    // Get last 20 messages
    writer
        .write_packet(
            ServerboundPacket::FetchMessages(0, 20),
            &secret,
            nonce_generator_write.as_mut(),
        )
        .await
        .unwrap();

    // To send close command when tcpstream is closed
    let (tx, rx) = oneshot::channel::<()>();

    let (console_tx, console_rx) = mpsc::unbounded_channel::<ConsoleMessage>();

    tokio::try_join!(
        reading_loop(reader, console_tx, rx, secret.clone(), nonce_generator_read),
        //writing_loop(writer, rx, secret.clone(), nonce_generator_write),
        console_loop(
            console_rx,
            writer,
            tx,
            secret.clone(),
            nonce_generator_write
        )
    )?;

    Ok(())
}

async fn console_loop(
    mut msg_channel: mpsc::UnboundedReceiver<ConsoleMessage>,
    mut writer: ConnectionWriter<ServerboundPacket>,
    close_sender: oneshot::Sender<()>,
    secret: Option<Vec<u8>>,
    mut nonce_generator: Option<ChaCha20Rng>,
) -> Result<(), Box<dyn Error>> {
    let mut input_buffer = String::new();
    let mut console = ConsoleEngine::init_fill_require(40, 10, 10).unwrap();
    console.set_title("Accord TUI");
    let mut col2 = (console.get_width() / 8) - 1;
    let mut w_userlist = UserListWindow::new(
        std::cmp::max(console.get_width() / 8, 10),
        console.get_height(),
    );
    let mut w_messages = MessageWindow::new(console.get_width() - col2, console.get_height() - 2);
    let mut w_input = InputWindow::new(console.get_width() - col2);

    loop {
        // force awaiting because this loop is mostly synchronous
        tokio::time::sleep(Duration::from_micros(1)).await;
        // process all received messages
        while let Ok(msg) = msg_channel.try_recv() {
            match msg {
                ConsoleMessage::Close => {
                    break;
                }
                ConsoleMessage::AddMessage(message) => {
                    w_messages.add_message(console::Message::Message(message))
                }
                ConsoleMessage::AddImageMessage(message) => {
                    w_messages.add_message(console::Message::Image(message))
                }
                ConsoleMessage::AddSystemMessage(message) => {
                    w_messages.add_message(console::Message::System(message))
                }
                ConsoleMessage::AddErrorMessage(message) => {
                    w_messages.add_message(console::Message::Error(message))
                }
                ConsoleMessage::RefreshUserList(usernames) => w_userlist.set_list(usernames),
                ConsoleMessage::AddUser(username) => w_userlist.add_user(username),
                ConsoleMessage::RemoveUser(username) => w_userlist.rm_user(username),
            }
        }
        // Process inputs
        match console.poll() {
            console_engine::events::Event::Key(KeyEvent { code, modifiers }) => {
                match code {
                    KeyCode::Enter => {
                        // send message
                        let p = if let Some(command) = input_buffer.strip_prefix('/') {
                            ServerboundPacket::Command(command.to_string())
                        } else {
                            ServerboundPacket::Message(input_buffer.to_string())
                        };
                        writer
                            .write_packet(p, &secret, nonce_generator.as_mut())
                            .await?;
                        input_buffer = String::new();
                    }
                    KeyCode::Backspace => {
                        let mut chars = input_buffer.chars();
                        chars.next_back();
                        input_buffer = chars.as_str().to_owned();
                    }
                    KeyCode::Up => w_messages.scroll(-1),
                    KeyCode::Down => w_messages.scroll(1),
                    KeyCode::Char(c) => {
                        if modifiers.is_empty() {
                            input_buffer.push(c);
                        }
                        if modifiers == KeyModifiers::CONTROL {
                            match c {
                                'c' => {
                                    close_sender.send(()).unwrap();
                                    break;
                                }
                                'p' => {}
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            console_engine::events::Event::Frame => {}
            console_engine::events::Event::Mouse(MouseEvent {
                kind,
                column: _,
                row: _,
                modifiers: _,
            }) => {
                if kind == MouseEventKind::ScrollUp {
                    w_messages.scroll(-1);
                } else if kind == MouseEventKind::ScrollDown {
                    w_messages.scroll(1)
                }
            }
            console_engine::events::Event::Resize(_, _) => {
                console.check_resize();
                col2 = (console.get_width() / 8) - 1;
                w_userlist.resize(
                    std::cmp::max(console.get_width() / 8, 10),
                    console.get_height(),
                );
                w_messages.resize(console.get_width() - col2, console.get_height() - 2);
                w_input.resize(console.get_width() - col2);
                // panic!("This program doesn't support terminal resizing yet!");
            }
        }
        // update screen
        w_input.set_content(&input_buffer);

        console.print_screen(0, 0, w_userlist.draw());
        console.line(
            (col2 - 1) as i32,
            0,
            (col2 - 1) as i32,
            (console.get_height() - 1) as i32,
            pixel::pxl_fbg(' ', Color::Black, Color::Grey),
        );
        console.print_screen(col2 as i32, 0, w_messages.draw());
        console.line(
            col2 as i32,
            (console.get_height() - 2) as i32,
            console.get_width() as i32,
            (console.get_height() - 2) as i32,
            pixel::pxl_fbg(' ', Color::Black, Color::Grey),
        );
        console.print_screen(
            col2 as i32,
            (console.get_height() - 1) as i32,
            w_input.draw(),
        );
        console.draw();
    }
    Ok(())
}

async fn reading_loop(
    mut reader: ConnectionReader<ClientboundPacket>,
    console_channel: mpsc::UnboundedSender<ConsoleMessage>,
    mut close_receiver: oneshot::Receiver<()>,
    secret: Option<Vec<u8>>,
    mut nonce_generator: Option<ChaCha20Rng>,
) -> Result<(), Box<dyn Error>> {
    'l: loop {
        match reader.read_packet(&secret, nonce_generator.as_mut()).await {
            Ok(Some(ClientboundPacket::Message(message))) => {
                console_channel.send(ConsoleMessage::AddMessage(message))?;
            }
            Ok(Some(ClientboundPacket::UserJoined(username))) => {
                console_channel.send(ConsoleMessage::AddSystemMessage(format!(
                    "{} joined the channel",
                    username
                )))?;
                console_channel.send(ConsoleMessage::AddUser(username))?;
            }
            Ok(Some(ClientboundPacket::UserLeft(username))) => {
                console_channel.send(ConsoleMessage::AddSystemMessage(format!(
                    "{} left the channel",
                    username
                )))?;
                console_channel.send(ConsoleMessage::RemoveUser(username))?;
            }
            Ok(Some(ClientboundPacket::UsersOnline(usernames))) => {
                console_channel.send(ConsoleMessage::RefreshUserList(usernames))?;
            }
            Ok(Some(ClientboundPacket::ImageMessage(im))) => {
                console_channel.send(ConsoleMessage::AddImageMessage(im))?;
            }
            Ok(Some(p)) => {
                console_channel.send(ConsoleMessage::AddErrorMessage(format!(
                    "!!Unhandled packet: {:?}",
                    p
                )))?;
            }
            Err(e) => {
                console_channel.send(ConsoleMessage::AddErrorMessage(e))?;
                console_channel.send(ConsoleMessage::Close)?;
                break 'l;
            }
            _ => {
                console_channel.send(ConsoleMessage::AddErrorMessage(
                    "Connection closed(?)\nPress Enter to exit.".to_owned(),
                ))?;
                console_channel.send(ConsoleMessage::Close)?;
                break 'l;
            }
        }
        if let Ok(()) = close_receiver.try_recv() {
            break 'l;
        }
    }
    Ok(())
}
