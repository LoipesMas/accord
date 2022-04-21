use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, Mutex},
};

use tokio::sync::mpsc;

use druid::{
    im::Vector,
    kurbo::Insets,
    widget::{Button, Either, Flex, Image, Label, List, TextBox, ViewSwitcher},
    AppLauncher, Data, Env, Event, ImageBuf, Lens, Widget, WidgetExt, WindowDesc,
};

mod controllers;
use controllers::*;

mod connection_handler;
use connection_handler::*;

//TODO: Loading up past messages

#[derive(Debug, Data, Lens, Clone, PartialEq, Eq)]
pub struct Message {
    pub sender: String,
    pub date: String,
    pub content: String,
}

impl Message {
    pub fn just_content(content: String) -> Self {
        Self{
            sender: String::new(),
            date: String::new(),
            content
        }
    }
}

#[derive(Debug, Data, Clone, Copy, PartialEq, Eq)]
enum Views {
    Connect,
    Main,
}

#[derive(Debug, Lens, Data, Clone)]
struct AppState {
    current_view: Views,
    info_label_text: Arc<String>,
    input_text1: Arc<String>,
    input_text2: Arc<String>,
    input_text3: Arc<String>,
    input_text4: Arc<String>,
    connection_handler_tx: Arc<mpsc::Sender<ConnectionHandlerCommand>>,
    messages: Vector<Message>,
}

fn main() {
    let connection_handler = ConnectionHandler {};
    let (tx, rx) = mpsc::channel(16);
    let dled_images = Arc::new(Mutex::new(HashMap::new()));
    let main_window = WindowDesc::new(ui_builder(Arc::clone(&dled_images))).title("accord");
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
        .delegate(Delegate { dled_images });

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

fn image_message(dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>) -> impl Widget<Message> {
    let image = Image::new(ImageBuf::empty())
        .fill_mode(druid::widget::FillStrat::Contain)
        .interpolation_mode(druid::piet::InterpolationMode::Bilinear)
        .controller(ImageController { dled_images });
    Flex::column()
        .with_child(
            Label::dynamic(|data: &Message, _env| format!("{} {}:", data.sender, data.date))
                .align_left(),
        )
        .with_default_spacer()
        .with_child(
            image
                .fix_width(400.0)
                .align_left()
                .padding(Insets::uniform_xy(50.0, 0.0)),
        )
}

fn text_message() -> impl Widget<Message> {
    Label::new(|data: &Message, _env: &_| {
        format!("{} {}: {}", data.sender, data.date, data.content)
    })
    .with_line_break_mode(druid::widget::LineBreaking::WordWrap)
    .expand_width()
}

fn main_view(dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>) -> impl Widget<AppState> {
    let info_label = Label::dynamic(|data, _env| format!("{}", data))
        .with_text_color(druid::Color::YELLOW)
        .lens(AppState::info_label_text);

    Flex::column()
        .cross_axis_alignment(druid::widget::CrossAxisAlignment::Start)
        .with_child(info_label)
        .with_flex_child(
            List::new(move || {
                let dled_images_2 = Arc::clone(&dled_images);
                Either::new(
                    move |data: &Message, _env: &_| {
                        dled_images_2
                            .lock()
                            .ok()
                            .map(|d| (*d).contains_key(&data.content))
                            .unwrap_or(false)
                    },
                    image_message(Arc::clone(&dled_images)),
                    text_message(),
                )
                .padding(Insets::uniform_xy(0.0, 3.0))
            })
            .scroll()
            .vertical()
            .controller(ScrollController)
            .expand_height()
            .lens(AppState::messages),
            1.0,
        )
        .with_default_spacer()
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

fn ui_builder(dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>) -> impl Widget<AppState> {
    Flex::column()
        .with_child(Label::new("accord").with_text_size(40.0))
        .with_default_spacer()
        .with_flex_child(
            ViewSwitcher::new(
                |data: &AppState, _env| data.current_view,
                move |selector, _data, _env| match *selector {
                    Views::Connect => Box::new(connect_view()),
                    _ => Box::new(main_view(Arc::clone(&dled_images))),
                },
            ),
            1.0,
        )
}

struct Delegate {
    dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>,
}

impl druid::AppDelegate<AppState> for Delegate {
    fn event(
        &mut self,
        _ctx: &mut druid::DelegateCtx,
        _window_id: druid::WindowId,
        event: Event,
        data: &mut AppState,
        _env: &Env,
    ) -> Option<Event> {
        use druid::keyboard_types::Key;
        match event {
            Event::KeyDown(ref kevent) => match kevent.key {
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
        _env: &Env,
    ) -> druid::Handled {
        if let Some(command) = cmd.get::<GuiCommand>(druid::Selector::new("gui_command")) {
            match command {
                GuiCommand::AddMessage(m) => {
                    data.messages.push_back(m.clone());
                    let link = &m.content;
                    let mut dled_images = self.dled_images.lock().unwrap();
                    if !dled_images.contains_key(link) {
                        //TODO: replace this block with async
                        let client = reqwest::blocking::ClientBuilder::new()
                            .timeout(std::time::Duration::from_secs(3))
                            .build()
                            .unwrap();
                        let req = client.get(link).build();
                        match req.and_then(|req| client.execute(req)) {
                            Ok(resp) => {
                                if resp.status() == reqwest::StatusCode::OK
                                    && resp
                                        .headers()
                                        .get("content-type")
                                        .map(|v| {
                                            v.to_str()
                                                .map(|s| s.starts_with("image/"))
                                                .unwrap_or(false)
                                        })
                                        .unwrap_or(false)
                                {
                                    let img_bytes = resp.bytes().unwrap();
                                    // image create converts jpeg or whatever to raw bytes
                                    let img = image::load_from_memory(&img_bytes).unwrap();
                                    let img2 = img.into_rgba8();

                                    let img_buf = ImageBuf::from_raw(
                                        img2.as_raw().as_slice(),
                                        druid::piet::ImageFormat::RgbaSeparate,
                                        img2.width() as usize,
                                        img2.height() as usize,
                                    );

                                    dled_images.insert(link.to_string(), img_buf);
                                }
                            }
                            Err(e) => {
                                println!("{}", e);
                            }
                        }
                    }
                }
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
        };
        druid::Handled::No
    }
}
