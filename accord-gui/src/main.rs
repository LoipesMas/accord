use std::{net::SocketAddr, str::FromStr, sync::Arc};

use tokio::sync::mpsc;

use druid::{
    im::Vector,
    widget::{Button, Controller, Either, Flex, Label, List, TextBox, ViewSwitcher},
    AppLauncher, Data, Env, Event, EventCtx, ImageBuf, Lens, Widget, WidgetExt, WindowDesc,
};

mod connection_handler;
use connection_handler::*;

#[derive(Debug, Data, Clone, Copy, PartialEq, Eq)]
enum Views {
    Connect,
    Main,
}

#[derive(Debug,Lens, Data, Clone)]
struct AppState {
    current_view: Views,
    info_label_text: Arc<String>,
    input_text1: Arc<String>,
    input_text2: Arc<String>,
    input_text3: Arc<String>,
    input_text4: Arc<String>,
    connection_handler_tx: Arc<mpsc::Sender<ConnectionHandlerCommand>>,
    messages: Vector<String>,
}

fn main() {
    let connection_handler = ConnectionHandler {};
    let (tx, rx) = mpsc::channel(16);
    let main_window = WindowDesc::new(ui_builder());
    let data = AppState {
        current_view: Views::Connect,
        info_label_text: Arc::new("".to_string()),
        input_text1: Arc::new("127.0.0.1".to_string()),
        input_text2: Arc::new("".to_string()),
        input_text3: Arc::new("".to_string()),
        input_text4: Arc::new("".to_string()),
        connection_handler_tx: Arc::new(tx),
        messages: Vector::new(),
    };
    let launcher = AppLauncher::with_window(main_window)
        .log_to_console()
        .delegate(Delegate {});

    let event_sink = launcher.get_external_handle();

    std::thread::spawn(move || {
        connection_handler.main_loop(rx, event_sink);
    });

    launcher.launch(data).unwrap();
}

fn connect_click(data: &mut AppState) {
    if accord::utils::verify_username(&*data.input_text2) {
        data.connection_handler_tx
            .blocking_send(ConnectionHandlerCommand::Connect(
                SocketAddr::from_str(&format!("{}:{}", data.input_text1, accord::DEFAULT_PORT))
                    .unwrap(),
                data.input_text2.to_string(),
                data.input_text3.to_string(),
            ))
            .unwrap();
    } else {
        data.info_label_text = Arc::new("Invalid username".to_string());
    };
}

fn send_message_click(data: &mut AppState) {
    if accord::utils::verify_message(&*data.input_text4) {
        data.connection_handler_tx
            .blocking_send(ConnectionHandlerCommand::Send(data.input_text4.to_string()))
            .unwrap();
        data.input_text4 = Arc::new(String::new());
    } else {
        data.info_label_text = Arc::new("Invalid message".to_string());
    };
}

fn connect_view() -> impl Widget<AppState> {
    let info_label = Label::dynamic(|data, _env| format!("{}", data))
        .with_text_color(druid::Color::YELLOW)
        .lens(AppState::info_label_text);
    let label1 = Label::new("Address:").padding(5.0).center();
    let label2 = Label::new("Username:").padding(5.0).center();
    let label3 = Label::new("Password:").padding(5.0).center();
    let button = Button::new("Connect")
        .on_click(|_, data, _| connect_click(data))
        .padding(5.0);
    let input1 = TextBox::new().lens(AppState::input_text1);
    let input2 = TextBox::new()
        .lens(AppState::input_text2)
        .controller(TakeFocusConnect);
    let input3 = TextBox::new().lens(AppState::input_text3);

    Flex::column()
        .with_child(info_label)
        .with_child(Flex::row().with_child(label1).with_child(input1))
        .with_child(Flex::row().with_child(label2).with_child(input2))
        .with_child(Flex::row().with_child(label3).with_child(input3))
        .with_child(button)
}

fn main_view() -> impl Widget<AppState> {
    let info_label = Label::dynamic(|data, _env| format!("{}", data))
        .with_text_color(druid::Color::YELLOW)
        .lens(AppState::info_label_text);
    Flex::column()
        .cross_axis_alignment(druid::widget::CrossAxisAlignment::Start)
        .with_child(info_label)
        .with_flex_child(
            List::new(|| {
                Label::new(|item: &String, _env: &_| item.clone())
                    .with_line_break_mode(druid::widget::LineBreaking::WordWrap)
                    .expand_width()
            })
            .scroll()
            .vertical()
            .expand_height()
            .lens(AppState::messages),
            1.0,
        )
        .with_child(
            Flex::row()
                .with_flex_child(
                    TextBox::new()
                        .lens(AppState::input_text4)
                        .expand_width()
                        .controller(TakeFocusMain),
                    1.0,
                )
                .with_default_spacer()
                .with_child(
                    Button::new("Send")
                        .on_click(|_ctx, data: &mut AppState, _env| send_message_click(data)),
                ),
        )
        .padding(20.0)
}

fn ui_builder() -> impl Widget<AppState> {
    Flex::column()
        .with_child(Label::new("accord").with_text_size(40.0))
        .with_default_spacer()
        .with_flex_child(
            ViewSwitcher::new(
                |data: &AppState, _env| data.current_view,
                |selector, _data, _env| match *selector {
                    Views::Connect => Box::new(connect_view()),
                    _ => Box::new(main_view()),
                },
            ),
            1.0,
        )
}
struct TakeFocusConnect;

impl<T: std::fmt::Debug, W: Widget<T>> Controller<T, W> for TakeFocusConnect {
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        if let Event::WindowConnected = event {
            ctx.request_focus();
        }
        else if let Event::Command(command) = event {
            if let Some(GuiCommand::ConnectionEnded(_)) = command.get::<GuiCommand>(druid::Selector::new("gui_command")) {
                ctx.request_focus();
            }
        }
        child.event(ctx, event, data, env)
    }
}

struct TakeFocusMain;

impl<T: std::fmt::Debug, W: Widget<T>> Controller<T, W> for TakeFocusMain {
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        if let Event::Command(command) = event {
            if let Some(GuiCommand::Connected) = command.get::<GuiCommand>(druid::Selector::new("gui_command")) {
                ctx.request_focus();
            }
        }
        child.event(ctx, event, data, env)
    }
}

struct Delegate;

impl druid::AppDelegate<AppState> for Delegate {
    fn event(
        &mut self,
        _ctx: &mut druid::DelegateCtx,
        _window_id: druid::WindowId,
        event: druid::Event,
        data: &mut AppState,
        _env: &druid::Env,
    ) -> Option<druid::Event> {
        use druid::keyboard_types::Key;
        use druid::Event;
        match event {
            Event::KeyUp(ref kevent) => match kevent.key {
                Key::Enter => {
                    match data.current_view {
                        Views::Connect => connect_click(data),
                        Views::Main => send_message_click(data),
                    }
                    None
                }
                _ => Some(event),
            },
            _ => Some(event),
        }
    }

    fn command(
        &mut self,
        _ctx: &mut druid::DelegateCtx,
        _target: druid::Target,
        cmd: &druid::Command,
        data: &mut AppState,
        _env: &druid::Env,
    ) -> druid::Handled {
        if let Some(command) = cmd.get::<GuiCommand>(druid::Selector::new("gui_command")) {
            match command {
                GuiCommand::AddMessage(m) => data.messages.push_back(m.to_string()),
                GuiCommand::Connected => {
                    data.info_label_text = Arc::new(String::new());
                    data.current_view = Views::Main;
                }
                GuiCommand::ConnectionEnded(m) => {
                    data.messages = Vector::new();
                    data.info_label_text = Arc::new(m.to_string());
                    data.current_view = Views::Connect;
                }
            };
            druid::Handled::Yes
        } else {
            druid::Handled::No
        }
    }
}
