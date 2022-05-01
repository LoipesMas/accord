use console::InputWindow;
use console::LoginWindow;
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
use tokio::net::TcpStream;
use tokio::sync::mpsc::error::SendError;

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

    let mut console = ConsoleEngine::init_fill_require(40, 10, 10).unwrap();
    console.set_title("Accord TUI");
    let (reader, mut writer, secret, nonce_generator_read, mut nonce_generator_write) =
        login(&mut console, addr).await;

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

    if let Err(e) = tokio::try_join!(
        reading_loop(reader, console_tx, rx, secret.clone(), nonce_generator_read),
        //writing_loop(writer, rx, secret.clone(), nonce_generator_write),
        console_loop(
            console,
            console_rx,
            writer,
            tx,
            secret.clone(),
            nonce_generator_write
        )
    ) {
        if e.downcast_ref::<SendError<ConsoleMessage>>().is_none() {
            panic!("{:?}", e);
        }
    }

    Ok(())
}

async fn login(
    console: &mut ConsoleEngine,
    addr: SocketAddr,
) -> (
    ConnectionReader<ClientboundPacket>,
    ConnectionWriter<ServerboundPacket>,
    Option<Vec<u8>>,
    Option<ChaCha20Rng>,
    Option<ChaCha20Rng>,
) {
    // let w_log = MessageWindow::new(console.get_width(), console.get_height());
    let mut login_width = std::cmp::max(console.get_width() / 6, 20);
    let mut w_login = LoginWindow::new(login_width);

    // println!("Connecting to: {}", addr);
    let socket = TcpStream::connect(addr).await.unwrap();

    // println!("Connected!");
    let connection = Connection::<ClientboundPacket, ServerboundPacket>::new(socket);
    let (mut reader, mut writer) = connection.split();

    //==================================
    //      Encryption
    //==================================
    // println!("Establishing encryption...");
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
                pub_key = rsa::pkcs8::FromPublicKey::from_public_key_der(&pub_key_der).unwrap();
                assert_eq!(ENC_TOK_LEN, token_.len());
                token_
            }
            _ => {
                panic!("Encryption failed. Server response: {:?}", p);
            }
        }
    } else {
        panic!("Failed to establish encryption");
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
        Ok(Some(ClientboundPacket::EncryptionAck)) => {}
        Ok(_) => {
            panic!("Failed encryption step 2. Server response: {:?}", p);
        }
        Err(e) => {
            panic!("{}", e);
        }
    }

    //==================================
    //      Get credentials
    //==================================

    while !w_login.is_finished() {
        match console.poll() {
            console_engine::events::Event::Key(KeyEvent { code, modifiers }) => {
                if let Some(prompt) = w_login.get_active_prompt() {
                    match code {
                        KeyCode::Enter => {
                            if w_login.is_prompting_username() {
                                w_login.submit();
                                console.print_fbg(
                                    0,
                                    0,
                                    "                                      ",
                                    Color::Red,
                                    Color::Reset,
                                );
                                if w_login.get_username().is_empty() {
                                    w_login.reset();
                                    console.print_fbg(
                                        0,
                                        0,
                                        "Username can't be empty!              ",
                                        Color::Red,
                                        Color::Reset,
                                    )
                                }
                                if w_login.get_username().len() > 17 {
                                    w_login.reset();
                                    console.print_fbg(
                                        0,
                                        0,
                                        "Username too long. (Max 17 characters)",
                                        Color::Red,
                                        Color::Reset,
                                    )
                                }
                            } else {
                                w_login.submit();
                            }
                        }
                        KeyCode::Esc => std::process::exit(0),
                        KeyCode::Backspace => prompt.remove_char(1),
                        KeyCode::Delete => prompt.remove_char(-1),
                        KeyCode::Left => prompt.move_cursor(-1),
                        KeyCode::Right => prompt.move_cursor(1),
                        KeyCode::Home => prompt.move_cursor(i32::MIN),
                        KeyCode::End => prompt.move_cursor(i32::MAX),
                        KeyCode::Char(c) => {
                            if c.is_alphanumeric() {
                                if modifiers.is_empty() {
                                    prompt.put_char(c);
                                }
                                if modifiers == KeyModifiers::SHIFT {
                                    // I don't understand why it works this way but not the other
                                    if c.is_ascii_uppercase() {
                                        prompt.put_char(c.to_ascii_uppercase());
                                    } else {
                                        prompt.put_char(c.to_ascii_lowercase());
                                    }
                                }
                            } else {
                                console.print_fbg(
                                    0,
                                    0,
                                    "Input must be alphanumeric",
                                    Color::Red,
                                    Color::Reset,
                                )
                            }
                            if modifiers == KeyModifiers::CONTROL && c == 'c' {
                                std::process::exit(0);
                            }
                        }
                        _ => {}
                    }
                }
            }
            console_engine::events::Event::Mouse(_) => {}
            console_engine::events::Event::Resize(_, _) => {
                console.clear_screen();
                console.check_resize();
                login_width = std::cmp::max(console.get_width() / 6, 20);
                w_login.resize(login_width);
            }
            console_engine::events::Event::Frame => {}
        }
        console.print_screen(
            (console.get_width() as i32 - login_width as i32) / 2,
            (console.get_height() as i32 - 7) / 2,
            w_login.draw(console.frame_count),
        );
        console.draw();
    }
    let username = w_login.get_username();
    let password = w_login.get_password();

    //==================================
    //      Login
    //==================================
    // println!("Logging in...");
    writer
        .write_packet(
            ServerboundPacket::Login {
                username: username.to_string(),
                password: password.to_string(),
            },
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
            ClientboundPacket::LoginAck => {}
            ClientboundPacket::LoginFailed(m) => {
                panic!("Login failed: {}", m);
            }
            _ => {
                panic!("Login failed. Server response: {:?}", p);
            }
        }
    } else {
        panic!("Failed to login ;/");
    }

    (
        reader,
        writer,
        secret,
        nonce_generator_read,
        nonce_generator_write,
    )
}

async fn console_loop(
    mut console: ConsoleEngine,
    mut msg_channel: mpsc::UnboundedReceiver<ConsoleMessage>,
    mut writer: ConnectionWriter<ServerboundPacket>,
    close_sender: oneshot::Sender<()>,
    secret: Option<Vec<u8>>,
    mut nonce_generator: Option<ChaCha20Rng>,
) -> Result<(), Box<dyn Error>> {
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
                    w_messages.add_message(console::Message::Text(message))
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
                        let p = if let Some(command) = w_input.get_input_buffer().strip_prefix('/')
                        {
                            ServerboundPacket::Command(command.to_string())
                        } else {
                            ServerboundPacket::Message(w_input.get_input_buffer().to_string())
                        };
                        writer
                            .write_packet(p, &secret, nonce_generator.as_mut())
                            .await?;
                        w_input.clear_input_buffer();
                    }
                    KeyCode::Backspace => w_input.remove_char(1),
                    KeyCode::Delete => w_input.remove_char(-1),
                    KeyCode::Up => w_messages.scroll(-1),
                    KeyCode::Down => w_messages.scroll(1),
                    KeyCode::Left => w_input.move_cursor(-1),
                    KeyCode::Right => w_input.move_cursor(1),
                    KeyCode::PageUp => w_messages.scroll(-(console.get_height() as i32) - 3),
                    KeyCode::PageDown => w_messages.scroll((console.get_height() as i32) - 3),
                    KeyCode::Home => w_input.move_cursor(i32::MIN),
                    KeyCode::End => w_input.move_cursor(i32::MAX),
                    KeyCode::Esc => {
                        close_sender.send(()).unwrap();
                        break;
                    }
                    KeyCode::Char(c) => {
                        if modifiers.is_empty() {
                            w_input.put_char(c);
                        }
                        if modifiers == KeyModifiers::SHIFT {
                            // I don't understand why it works this way but not the other
                            if c.is_ascii_uppercase() {
                                w_input.put_char(c.to_ascii_uppercase());
                            } else {
                                w_input.put_char(c.to_ascii_lowercase());
                            }
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
            w_input.draw(console.frame_count),
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
