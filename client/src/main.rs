use client::{Client, ClientReader, ClientWriter};
use console::{MessageWindow, UserListWindow};
use console_engine::{
    crossterm::event::{KeyEvent, MouseEvent, MouseEventKind},
    forms::{
        self,
        constraints::{self, Callback},
        Form, FormField, FormOptions, FormStyle, FormValue,
    },
    pixel,
    rect_style::BorderStyle,
    Color, ConsoleEngine, KeyCode, KeyModifiers,
};
use std::error::Error;
use std::str::FromStr;
use std::time::Duration;

use tokio::sync::mpsc::error::SendError;

use accord::packets::*;

use std::net::SocketAddr;

use tokio::sync::{mpsc, oneshot};

use crate::console::ConsoleMessage;

use clap::Parser;

mod client;
mod console;

/// Accord client - Terminal User Interface for the instant messaging chat system over TCP
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {}

// TODO: config file?
const THEME_BG: Color = Color::Rgb { r: 32, g: 7, b: 47 };
const THEME_FG: Color = Color::Cyan;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    //==================================
    //      Parse args
    //==================================
    let _args = Args::parse();

    let mut console = ConsoleEngine::init_fill_require(40, 10, 10).unwrap();
    console.set_title("Accord TUI");

    let mut client = login(&mut console).await?;

    // Get player list on join
    client
        .send(ServerboundPacket::Command("list".to_string()))
        .await?;

    // Get last 20 messages
    client.send(ServerboundPacket::FetchMessages(0, 20)).await?;

    // To send close command when tcpstream is closed
    let (tx, rx) = oneshot::channel::<()>();

    let (console_tx, console_rx) = mpsc::unbounded_channel::<ConsoleMessage>();

    let (client_r, client_w) = client.breakdown();

    if let Err(e) = tokio::try_join!(
        reading_loop(client_r, console_tx, rx),
        //writing_loop(writer, rx, secret.clone(), nonce_generator_write),
        console_loop(client_w, console, console_rx, tx,)
    ) {
        if e.downcast_ref::<SendError<ConsoleMessage>>().is_none() {
            panic!("{:?}", e);
        }
    }

    Ok(())
}

async fn login(console: &mut ConsoleEngine) -> Result<Client, Box<dyn Error>> {
    let mut login_width = std::cmp::max(console.get_width() / 6, 20);

    let form_theme = FormStyle {
        border: Some(BorderStyle::new_light().with_colors(THEME_FG, THEME_BG)),
        fg: THEME_FG,
        bg: THEME_BG,
    };

    let mut login_form = Form::new(
        login_width,
        8,
        FormOptions {
            style: form_theme,
            ..Default::default()
        },
    );

    let mut address_field = forms::Text::new(
        1,
        FormOptions {
            style: form_theme,
            label: Some("Address"),
            constraints: vec![Callback::new(
                "Please input a valid address",
                &|value: &FormValue| -> bool {
                    if let FormValue::String(address) = value {
                        SocketAddr::from_str(address).is_ok()
                    } else {
                        false
                    }
                },
            )],
            ..Default::default()
        },
    );
    address_field.set_input_buffer(&format!("127.0.0.1:{}", accord::DEFAULT_PORT));

    login_form.add_field("address", address_field);

    login_form.build_field::<forms::Text>(
        "username",
        FormOptions {
            style: form_theme,
            label: Some("Username"),
            constraints: vec![
                constraints::NotBlank::new("Please input a username"),
                constraints::Alphanumeric::new("Username must be alphanumeric"),
            ],
            ..Default::default()
        },
    );
    login_form.build_field::<forms::HiddenText>(
        "password",
        FormOptions {
            style: form_theme,
            label: Some("Password"),
            constraints: vec![constraints::NotBlank::new("Password shouldn't be blank")],
            ..Default::default()
        },
    );
    login_form.set_active(true);
    let mut login_x = (console.get_width() as i32 - login_width as i32) / 2;
    let mut login_y = (console.get_height() as i32 - 9) / 2;

    //==================================
    //      Get credentials
    //==================================

    'login: loop {
        while !login_form.is_finished() {
            match console.poll() {
                // exit with escape
                console_engine::events::Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    modifiers: KeyModifiers::NONE,
                }) => {
                    break 'login;
                }
                // exit with ctrl+C
                console_engine::events::Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                }) => {
                    break 'login;
                }
                // handle terminal resizing
                console_engine::events::Event::Resize(_, _) => {
                    console.fill(pixel::pxl_fbg(' ', THEME_FG, THEME_BG));
                    console.check_resize();
                    login_width = std::cmp::max(console.get_width() / 6, 20);
                    login_form.resize(login_width, 8);
                    login_x = (console.get_width() as i32 - login_width as i32) / 2;
                    login_y = (console.get_height() as i32 - 9) / 2;
                }
                // other events are passed to the form
                event => login_form.handle_event(event),
            }
            console.print_screen(
                login_x,
                login_y,
                login_form.draw((console.frame_count % 10 < 5) as usize),
            );
            console.draw();
        }
        if login_form.is_valid() {
            if let Ok(FormValue::String(address)) = login_form.get_validated_field_output("address")
            {
                let addr = SocketAddr::from_str(&address)?;

                match Client::init(addr).await {
                    Ok(mut client) => {
                        if let (Ok(FormValue::String(username)), Ok(FormValue::String(password))) = (
                            login_form.get_validated_field_output("username"),
                            login_form.get_validated_field_output("password"),
                        ) {
                            if let Err(error) = client.login(username, password).await {
                                console.fill(pixel::pxl_fbg(' ', THEME_FG, THEME_BG));
                                console.print_fbg(0, 0, &error.to_string(), Color::Red, THEME_BG);
                                console.draw();
                                login_form.reset();
                                login_form.set_active(true);
                            } else {
                                return Ok(client);
                            }
                        }
                    }
                    Err(error) => {
                        console.fill(pixel::pxl_fbg(' ', THEME_FG, THEME_BG));
                        console.print_fbg(0, 0, &format!("{:?}", error), Color::Red, THEME_BG);
                        login_form.reset();
                    }
                }
            }
        } else {
            console.fill(pixel::pxl_fbg(' ', THEME_FG, THEME_BG));
            let mut pos = 0;
            if let Some(messages) = login_form.validate_field("address") {
                for message in messages.iter() {
                    console.print_fbg(0, pos, message, Color::Red, THEME_BG);
                    pos += 1;
                }
            }
            if let Some(messages) = login_form.validate_field("username") {
                for message in messages.iter() {
                    console.print_fbg(0, pos, message, Color::Red, THEME_BG);
                    pos += 1;
                }
            }
            if let Some(messages) = login_form.validate_field("password") {
                for message in messages.iter() {
                    console.print_fbg(0, pos, message, Color::Red, THEME_BG);
                    pos += 1;
                }
            }
            login_form.reset();
        }
    }
    Err("User cancelled login")?
}

async fn console_loop(
    mut client: ClientWriter,
    mut console: ConsoleEngine,
    mut msg_channel: mpsc::UnboundedReceiver<ConsoleMessage>,
    close_sender: oneshot::Sender<()>,
) -> Result<(), Box<dyn Error>> {
    let mut col2 = std::cmp::max(console.get_width() / 8, 10) - 1;
    let mut w_userlist = UserListWindow::new(col2 + 1, console.get_height());
    let mut w_messages = MessageWindow::new(console.get_width() - col2, console.get_height() - 2);
    let mut w_input = forms::Text::new(
        console.get_width() - col2,
        FormOptions {
            style: FormStyle {
                fg: THEME_FG,
                bg: THEME_BG,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    w_input.set_active(true);

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
                        if let FormValue::String(message) = w_input.get_output() {
                            let p = if let Some(command) = message.strip_prefix('/') {
                                ServerboundPacket::Command(command.to_string())
                            } else {
                                ServerboundPacket::Message(message)
                            };
                            client.send(p).await?;
                            w_input.clear_input_buffer();
                        }
                    }
                    KeyCode::Up => w_messages.scroll(-1),
                    KeyCode::Down => w_messages.scroll(1),
                    KeyCode::PageUp => w_messages.scroll(-(console.get_height() as i32) - 3),
                    KeyCode::PageDown => w_messages.scroll((console.get_height() as i32) - 3),
                    KeyCode::Esc => {
                        close_sender.send(()).unwrap();
                        break;
                    }
                    KeyCode::Char('c') if modifiers == KeyModifiers::CONTROL => {
                        close_sender.send(()).unwrap();
                        break;
                    }
                    _ => w_input.handle_event(console_engine::events::Event::Key(KeyEvent {
                        code,
                        modifiers,
                    })),
                }
            }
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
                col2 = std::cmp::max(console.get_width() / 8, 10) - 1;
                w_userlist.resize(col2 + 1, console.get_height());
                w_messages.resize(console.get_width() - col2, console.get_height() - 2);
                w_input.resize(console.get_width() - col2, 1);
            }
            event => w_input.handle_event(event),
        }
        // update screen
        console.print_screen(0, 0, w_userlist.draw());
        console.line(
            (col2 - 1) as i32,
            0,
            (col2 - 1) as i32,
            (console.get_height() - 1) as i32,
            pixel::pxl_fbg(' ', THEME_BG, THEME_FG),
        );
        console.print_screen(col2 as i32, 0, w_messages.draw());
        console.line(
            col2 as i32,
            (console.get_height() - 2) as i32,
            console.get_width() as i32,
            (console.get_height() - 2) as i32,
            pixel::pxl_fbg(' ', THEME_BG, THEME_FG),
        );
        console.print_screen(
            col2 as i32,
            (console.get_height() - 1) as i32,
            w_input.draw((console.frame_count % 10 < 5) as usize),
        );
        console.draw();
    }
    Ok(())
}

async fn reading_loop(
    mut client: ClientReader,
    console_channel: mpsc::UnboundedSender<ConsoleMessage>,
    mut close_receiver: oneshot::Receiver<()>,
) -> Result<(), Box<dyn Error>> {
    'l: loop {
        match client.read().await {
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
                console_channel.send(ConsoleMessage::AddSystemMessage(String::from(
                    "-- Users online --",
                )))?;
                for name in usernames.iter() {
                    console_channel.send(ConsoleMessage::AddSystemMessage(String::from(name)))?;
                }
                console_channel.send(ConsoleMessage::AddSystemMessage(String::from(
                    "------------------",
                )))?;
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
