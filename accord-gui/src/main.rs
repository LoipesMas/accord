use std::{net::SocketAddr, str::FromStr, sync::Arc};

use tokio::sync::mpsc;

use druid::{
    im::Vector,
    widget::{Button, Flex, Label, List, TextBox, ViewSwitcher},
    AppLauncher, Data, Lens, Widget, WidgetExt, WindowDesc,
};

mod connection_handler;
use connection_handler::*;

#[derive(Data, Clone, Copy, PartialEq, Eq)]
enum Views {
    Connect,
    Main,
}

#[derive(Lens, Data, Clone)]
struct AppState {
    current_view: Views,
    input_text1: Arc<String>,
    input_text2: Arc<String>,
    input_text3: Arc<String>,
    connection_handler_tx: Arc<mpsc::Sender<ConnectionHandlerCommand>>,
    messages: Vector<String>,
}

fn main() {
    let connection_handler = ConnectionHandler {};
    let (tx, rx) = mpsc::channel(16);
    let main_window = WindowDesc::new(ui_builder());
    let data = AppState {
        current_view: Views::Connect,
        input_text1: Arc::new("127.0.0.1".to_string()),
        input_text2: Arc::new("".to_string()),
        input_text3: Arc::new("".to_string()),
        connection_handler_tx: Arc::new(tx),
        messages: Vector::new(),
    };
    let launcher = AppLauncher::with_window(main_window).log_to_console();

    let event_sink = launcher.get_external_handle();
    std::thread::spawn(move || {
        connection_handler.main_loop(rx, event_sink);
    });

    launcher.launch(data).unwrap();
}

fn connect_view() -> impl Widget<AppState> {
    let label1 = Label::new("Address:").padding(5.0).center();
    let label2 = Label::new("Username:").padding(5.0).center();
    let label3 = Label::new("Password:").padding(5.0).center();
    let button = Button::new("Connect")
        .on_click(|_ctx, data: &mut AppState, _env| {
            if !accord::utils::verify_username(&*data.input_text2) {
                return;
            }
            data.connection_handler_tx
                .blocking_send(ConnectionHandlerCommand::Connect(
                    SocketAddr::from_str(&format!("{}:{}", data.input_text1, accord::DEFAULT_PORT))
                        .unwrap(),
                    data.input_text2.to_string(),
                    data.input_text3.to_string(),
                ))
                .unwrap();
            data.current_view = Views::Main;
            data.input_text1 = Arc::new(String::new());
            data.input_text2 = Arc::new(String::new());
            data.input_text3 = Arc::new(String::new());
        })
        .padding(5.0);
    let input1 = TextBox::new().lens(AppState::input_text1);
    let input2 = TextBox::new().lens(AppState::input_text2);
    let input3 = TextBox::new().lens(AppState::input_text3);

    Flex::column()
        .with_child(Flex::row().with_child(label1).with_child(input1))
        .with_child(Flex::row().with_child(label2).with_child(input2))
        .with_child(Flex::row().with_child(label3).with_child(input3))
        .with_child(button)
}

fn main_view() -> impl Widget<AppState> {
    Flex::column()
        .cross_axis_alignment(druid::widget::CrossAxisAlignment::Start)
        .with_child(MessageAdder {}.lens(AppState::messages))
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
                    TextBox::new().lens(AppState::input_text1).expand_width(),
                    1.0,
                )
                .with_default_spacer()
                .with_child(
                    Button::new("Send").on_click(|_ctx, data: &mut AppState, _env| {
                        if accord::utils::verify_message(&*data.input_text1) {
                            data.connection_handler_tx
                                .blocking_send(ConnectionHandlerCommand::Send(
                                    data.input_text1.to_string(),
                                ))
                                .unwrap();
                        }
                        data.input_text1 = Arc::new(String::new());
                    }),
                ),
        )
        .padding(20.0)
}

struct MessageAdder {}
impl Widget<Vector<String>> for MessageAdder {
    fn event(
        &mut self,
        _ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut Vector<String>,
        _env: &druid::Env,
    ) {
        if let druid::Event::Command(command) = event {
            if let Some(message) = command.get::<String>(druid::Selector::new("add_message")) {
                data.push_back(message.to_owned());
            }
        };
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut druid::LifeCycleCtx,
        _event: &druid::LifeCycle,
        _data: &Vector<String>,
        _env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut druid::UpdateCtx,
        _old_data: &Vector<String>,
        _data: &Vector<String>,
        _env: &druid::Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut druid::LayoutCtx,
        _bc: &druid::BoxConstraints,
        _data: &Vector<String>,
        _env: &druid::Env,
    ) -> druid::Size {
        druid::Size::ZERO
    }

    fn paint(&mut self, _ctx: &mut druid::PaintCtx, _data: &Vector<String>, _env: &druid::Env) {}
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
