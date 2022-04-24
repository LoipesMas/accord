use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, Mutex},
};

use accord::packets::ServerboundPacket;
use config::Config;
use tokio::sync::mpsc;

use druid::{
    im::Vector,
    kurbo::Insets,
    widget::{Button, Checkbox, Flex, Label, List, TextBox, ViewSwitcher},
    AppLauncher, Color, Data, Env, Event, FontDescriptor, FontFamily, ImageBuf, Lens, UnitPoint,
    Widget, WidgetExt, WindowDesc,
};

use serde::{Deserialize, Serialize};

use flexi_logger::Logger;

mod controllers;
use controllers::*;

mod connection_handler;
use connection_handler::*;

mod config;

//TODO: Loading up past messages

#[derive(Serialize, Deserialize)]
pub struct Theme {
    pub background1: String,
    pub background2: String,
    pub text_color1: String,
    pub color1: String,
    pub highlight: String,
    pub border: f64,
    pub rounding: f64,
}

impl Theme {
    pub fn default() -> Self {
        Self {
            background1: "#200730".to_string(),
            background2: "#030009".to_string(),
            text_color1: "#6ef3e7".to_string(),
            color1: "#7521ee29".to_string(),
            highlight: "#ffffff".to_string(),
            border: 0.0,
            rounding: 2.0,
        }
    }
}

#[derive(Debug, Data, Lens, Clone, PartialEq, Eq)]
pub struct Message {
    pub sender: String,
    pub date: String,
    pub content: String,
    pub is_image: bool,
}

impl Message {
    pub fn just_content(content: String) -> Self {
        Self {
            sender: String::new(),
            date: String::new(),
            content,
            is_image: false,
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
    remember_login: bool,
    input_text4: Arc<String>,
    connection_handler_tx: Arc<mpsc::Sender<ConnectionHandlerCommand>>,
    messages: Vector<Message>,
    images_from_links: bool,
}

fn init_logger() {
    Logger::try_with_env_or_str("warn")
        .unwrap()
        .start()
        .unwrap();
}

// This could be not static, but oh well
static mut THEME: Option<Theme> = None;

pub const GUI_COMMAND: druid::Selector<GuiCommand> = druid::Selector::new("gui_command");

fn main() {
    init_logger();

    let config = config::load_config();

    // I solemnly swear this is the only place in which we mutate THEME
    unsafe {
        THEME = Some(config.theme.expect("Theme should be loaded from config!"));
    }

    let connection_handler = ConnectionHandler {};
    let (tx, rx) = mpsc::channel(16);
    let dled_images = Arc::new(Mutex::new(HashMap::new()));
    let main_window = WindowDesc::new(ui_builder(Arc::clone(&dled_images))).title("accord");
    let data = AppState {
        current_view: Views::Connect,
        info_label_text: Arc::new("".to_string()),
        input_text1: Arc::new(config.address.clone()),
        input_text2: Arc::new(config.username.clone()),
        input_text3: Arc::new("".to_string()),
        remember_login: config.remember_login,
        input_text4: Arc::new("".to_string()),
        connection_handler_tx: Arc::new(tx),
        messages: Vector::new(),
        images_from_links: config.images_from_links,
    };
    let launcher = AppLauncher::with_window(main_window).delegate(Delegate {
        dled_images,
        rt: tokio::runtime::Runtime::new().unwrap(),
    });

    let event_sink = launcher.get_external_handle();

    std::thread::spawn(move || {
        connection_handler.main_loop(rx, event_sink);
    });

    launcher.launch(data).unwrap();
}

fn connect_click(data: &mut AppState) {
    let addr = match try_parse_addr(&data.input_text1) {
        Ok(addr) => addr,
        Err(e) => {
            log::warn!("{}", e);
            data.info_label_text = Arc::new("Invalid address".to_string());
            return;
        }
    };
    if accord::utils::verify_username(&*data.input_text2) {
        data.info_label_text = Arc::new("Connecting...".to_string());
        data.connection_handler_tx
            .blocking_send(ConnectionHandlerCommand::Connect(
                addr,
                data.input_text2.to_string(),
                data.input_text3.to_string(),
            ))
            .unwrap();
        config::save_config(config_from_appstate(data)).unwrap();
    } else {
        log::warn!("Invalid username");
        data.info_label_text = Arc::new("Invalid username".to_string());
    };
}

fn send_message_click(data: &mut AppState) {
    let s = data.input_text4.clone();
    if accord::utils::verify_message(&*s) {
        let p = if let Some(command) = s.strip_prefix('/') {
            ServerboundPacket::Command(command.to_string())
        } else {
            ServerboundPacket::Message(s.to_string())
        };
        data.connection_handler_tx
            .blocking_send(ConnectionHandlerCommand::Write(p))
            .unwrap();
        data.input_text4 = Arc::new(String::new());
    } else {
        data.info_label_text = Arc::new("Invalid message".to_string());
    };
}

// Less typing
fn unwrap_from_hex(s: &str) -> Color {
    Color::from_hex_str(s).unwrap()
}

fn connect_view() -> impl Widget<AppState> {
    let font = FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(20.0);
    let theme = unsafe {
        // We only read
        THEME.as_ref().unwrap()
    };

    let input_label_c = |s: &str| -> druid::widget::Align<AppState> {
        Label::new(s)
            .with_font(font.clone())
            .with_text_color(unwrap_from_hex(&theme.text_color1))
            .padding(7.0)
            .center()
    };
    let input_box_c = || -> TextBox<Arc<String>> {
        TextBox::new()
            .with_font(font.clone())
            .with_text_color(unwrap_from_hex(&theme.text_color1))
    };

    let info_label = Label::dynamic(|data, _env| format!("{}", data))
        .with_text_color(Color::YELLOW)
        .with_font(font.clone())
        .padding(5.0)
        .lens(AppState::info_label_text);
    let label1 = input_label_c("Address:");
    let label2 = input_label_c("Username:");
    let label3 = input_label_c("Password:");
    let button = Button::new("Connect")
        .on_click(|_, data, _| connect_click(data))
        .padding(5.0);
    let input1 = input_box_c().lens(AppState::input_text1);
    let input2 = input_box_c().lens(AppState::input_text2);
    let input3 = input_box_c()
        .lens(AppState::input_text3)
        .controller(TakeFocusConnect);
    let checkbox = Checkbox::new("Remember login").lens(AppState::remember_login);

    let checkbox2 = Checkbox::new("Images from links").lens(AppState::images_from_links);

    Flex::column()
        .with_child(info_label)
        .with_child(
            Flex::column()
                .with_child(Flex::row().with_child(label1).with_child(input1))
                .with_child(Flex::row().with_child(label2).with_child(input2))
                .with_child(Flex::row().with_child(label3).with_child(input3))
                .with_child(checkbox)
                .with_child(button)
                .with_child(checkbox2)
                .padding(10.0)
                .fix_width(300.0)
                .background(unwrap_from_hex(&theme.color1))
                .border(unwrap_from_hex(&theme.highlight), theme.border)
                .rounded(theme.rounding),
        )
        .align_vertical(UnitPoint::new(0.0, 0.25))
}

fn message(dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>) -> impl Widget<Message> {
    let theme = unsafe {
        // We only read
        THEME.as_ref().unwrap()
    };

    let font = FontDescriptor::new(FontFamily::SYSTEM_UI).with_size(17.0);
    let content_label = Label::dynamic(|d: &String, _e: &_| d.clone())
        .with_font(font.clone())
        .with_text_color(unwrap_from_hex(&theme.text_color1))
        .with_line_break_mode(druid::widget::LineBreaking::WordWrap)
        .lens(Message::content);
    let image_from_link = ImageFromLink::new(content_label, dled_images);
    Flex::row()
        .cross_axis_alignment(druid::widget::CrossAxisAlignment::Start)
        .with_child(
            Label::dynamic(|data: &Message, _env| {
                if data.sender.is_empty() {
                    "".to_string()
                } else {
                    format!("{} {}:", data.sender, data.date)
                }
            })
            .with_text_color(unwrap_from_hex(&theme.text_color1))
            .with_font(font.with_weight(druid::FontWeight::BOLD)),
        )
        .with_default_spacer()
        .with_flex_child(Flex::column().with_child(image_from_link), 1.0)
        .padding(Insets::uniform_xy(3.0, 5.0))
        .background(unwrap_from_hex(&theme.color1))
        .rounded(theme.rounding * 2.0)
        .border(unwrap_from_hex(&theme.highlight), theme.border)
        .padding(Insets::uniform_xy(0.0, 3.0))
}

fn try_parse_addr(s: &str) -> Result<SocketAddr, std::net::AddrParseError> {
    if s.contains(':') {
        SocketAddr::from_str(s)
    } else {
        SocketAddr::from_str(&format!("{}:{}", s, accord::DEFAULT_PORT))
    }
}

fn main_view(dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>) -> impl Widget<AppState> {
    let info_label = Label::dynamic(|data, _env| format!("{}", data))
        .with_text_color(Color::YELLOW)
        .lens(AppState::info_label_text);

    Flex::column()
        .cross_axis_alignment(druid::widget::CrossAxisAlignment::Start)
        .with_child(info_label)
        .with_flex_child(
            List::new(move || {
                let dled_images_2 = Arc::clone(&dled_images);
                message(dled_images_2)
            })
            .controller(ListController)
            .scroll()
            .vertical()
            .controller(ScrollController::new())
            .expand_height()
            .lens(AppState::messages),
            1.0,
        )
        .with_default_spacer()
        .with_child(
            Flex::row()
                .with_flex_child(
                    TextBox::multiline()
                        .lens(AppState::input_text4)
                        .expand_width()
                        .controller(TakeFocusMain)
                        .controller(MessageTextBoxController),
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
    let theme = unsafe {
        // We only read
        THEME.as_ref().unwrap()
    };
    Flex::column()
        .with_child(Label::new("accord").with_text_size(43.0).padding(5.0))
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
        .background(druid::LinearGradient::new(
            UnitPoint::BOTTOM,
            UnitPoint::TOP,
            (
                unwrap_from_hex(&theme.background2),
                unwrap_from_hex(&theme.background1),
            ),
        ))
}

struct Delegate {
    dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>,
    rt: tokio::runtime::Runtime,
}

fn config_from_appstate(data: &AppState) -> Config {
    let (address, username) = if data.remember_login {
        (data.input_text1.to_string(), data.input_text2.to_string())
    } else {
        Default::default()
    };
    Config {
        address,
        username,
        remember_login: data.remember_login,
        images_from_links: data.images_from_links,
        theme: None,
    }
}

impl druid::AppDelegate<AppState> for Delegate {
    fn event(
        &mut self,
        ctx: &mut druid::DelegateCtx,
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
                Key::PageUp => {
                    ctx.submit_command(controllers::SCROLL.with(-1.0));
                    None
                }
                Key::PageDown => {
                    ctx.submit_command(controllers::SCROLL.with(1.0));
                    None
                }
                _ => Some(event),
            },
            _ => Some(event),
        }
    }

    fn command(
        &mut self,
        ctx: &mut druid::DelegateCtx,
        _target: druid::Target,
        cmd: &druid::Command,
        data: &mut AppState,
        _env: &Env,
    ) -> druid::Handled {
        if let Some(command) = cmd.get(GUI_COMMAND) {
            match command {
                GuiCommand::AddMessage(m) => {
                    data.messages.push_back(m.clone());

                    // Try to get image from message link
                    //
                    // Note: Now that I think about it, this could be a pretty big vulnerability.
                    //  Maybe a better solution would be hosting images on the server?
                    if data.images_from_links {
                        let dled_images = Arc::clone(&self.dled_images);
                        let link = m.content.clone();
                        let event_sink = ctx.get_external_handle();
                        self.rt.spawn(async move {
                            try_get_image_from_link(&link, dled_images, event_sink).await;
                        });
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
                GuiCommand::SendImage(image_bytes) => {
                    let v = image_bytes.to_vec();
                    let p = ServerboundPacket::ImageMessage(v);
                    data.connection_handler_tx
                        .blocking_send(ConnectionHandlerCommand::Write(p))
                        .unwrap();
                }
                GuiCommand::StoreImage(hash, img_bytes) => {
                    let img_buf = ImageBuf::from_data(img_bytes).unwrap();

                    let mut dled_images = self.dled_images.lock().unwrap();
                    dled_images.insert(hash.to_string(), img_buf);
                    ctx.submit_command(
                        druid::Selector::<String>::new("image_downloaded").with(hash.to_string()),
                    );
                }
            };
        };
        druid::Handled::No
    }
}

async fn try_get_image_from_link(
    link: &str,
    dled_images: Arc<Mutex<HashMap<String, ImageBuf>>>,
    event_sink: druid::ExtEventSink,
) -> bool {
    if !dled_images.lock().unwrap().contains_key(link) {
        let client = reqwest::ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();

        // We get just head first to see if it's an image
        let req = client.head(link).build();
        let resp = match req {
            Ok(req) => client.execute(req).await,
            Err(_) => return false,
        };
        match resp {
            Ok(resp) => {
                if resp.status() == reqwest::StatusCode::OK
                    && resp.headers().get("content-type").map_or(false, |v| {
                        v.to_str().map_or(false, |s| s.starts_with("image/"))
                    })
                    && resp.headers().get("content-length").map_or(false, |v| {
                        v.to_str().map_or(false, |s| {
                            s.parse::<u32>().map_or(false, |l| {
                                l < 31457280 // 30 MB
                            })
                        })
                    })
                {
                    let req = client.get(link).build().unwrap();

                    let resp = match client.execute(req).await {
                        Ok(resp) => resp,
                        Err(_) => return false,
                    };

                    let img_bytes = resp.bytes().await.unwrap();
                    let img_buf = ImageBuf::from_data(&img_bytes).unwrap();

                    let mut dled_images = dled_images.lock().unwrap();
                    dled_images.insert(link.to_string(), img_buf);
                    event_sink
                        .submit_command(
                            druid::Selector::<String>::new("image_downloaded"),
                            link.to_string(),
                            druid::Target::Auto,
                        )
                        .unwrap();
                }
            }
            Err(e) => {
                log::warn!("Error when getting image: {}", e);
                return false;
            }
        };
    };

    true
}
