use std::collections::BTreeSet;

use chrono::TimeZone;
use console_engine::{pixel, screen::Screen, Color};

#[derive(Debug)]
pub enum ConsoleMessage {
    AddMessage(accord::packets::Message),
    AddImageMessage(accord::packets::ImageMessage),
    AddSystemMessage(String),
    AddErrorMessage(String),
    RefreshUserList(Vec<String>),
    AddUser(String),
    RemoveUser(String),
    Close,
}

pub enum Message {
    Message(accord::packets::Message),
    Image(accord::packets::ImageMessage),
    System(String),
    Error(String),
}

impl Message {
    pub fn print(&self, screen: &mut Screen, x: i32, y: i32) {
        match self {
            Message::Message(message) => {
                let time = chrono::Local.timestamp(message.time as i64, 0);
                screen.print(
                    x,
                    y,
                    &format!(
                        "[{}] {}: {}",
                        time.format("%H:%M %d-%m"),
                        message.sender,
                        message.text
                    ),
                )
            }
            Message::Image(message) => {
                let time = chrono::Local.timestamp(message.time as i64, 0);

                screen.print(
                    x,
                    y,
                    &format!(
                        "[{}] {}: [Image]",
                        time.format("%H:%M %d-%m"),
                        message.sender
                    ),
                )
            }
            Message::System(message) => {
                screen.print_fbg(x, y, &message.to_string(), Color::DarkGrey, Color::Reset)
            }
            Message::Error(message) => {
                screen.print_fbg(x, y, &message.to_string(), Color::Red, Color::Reset)
            }
        }
    }
}

pub struct UserListWindow {
    screen: Screen,
    dirty: bool,
    user_list: BTreeSet<String>,
}

impl UserListWindow {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            screen: Screen::new(w, h),
            user_list: BTreeSet::new(),
            dirty: true,
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.dirty = true;
        self.screen.resize(w, h)
    }

    pub fn set_list(&mut self, new_list: Vec<String>) {
        self.user_list.clear();
        for entry in new_list {
            self.add_user(entry)
        }
    }

    pub fn add_user(&mut self, username: String) {
        if !self.user_list.contains(&username) {
            self.dirty = true;
            self.user_list.insert(username);
        }
    }

    pub fn rm_user(&mut self, username: String) {
        if self.user_list.contains(&username) {
            self.dirty = true;
            self.user_list.remove(&username);
        }
    }

    pub fn draw(&mut self) -> &Screen {
        if self.dirty {
            self.screen.clear();
            self.screen.line(
                0,
                0,
                self.screen.get_width() as i32,
                0,
                pixel::pxl_fbg(' ', Color::Black, Color::Grey),
            );
            self.screen
                .print_fbg(0, 0, " Users", Color::Black, Color::Grey);
            for (index, username) in self.user_list.iter().enumerate() {
                self.screen.print(0, index as i32 + 1, username);
            }
            self.dirty = false;
        }
        &self.screen
    }
}

pub struct MessageWindow {
    screen: Screen,
    dirty: bool,
    message_list: Vec<Message>,
    scroll_index: usize,
}

impl MessageWindow {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            screen: Screen::new(w, h),
            dirty: true,
            message_list: vec![],
            scroll_index: 0,
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.dirty = true;
        self.screen.resize(w, h)
    }

    pub fn add_message(&mut self, message: Message) {
        self.dirty = true;
        self.message_list.push(message)
    }

    pub fn scroll(&mut self, amount: i32) {
        self.dirty = true;
        self.scroll_index = std::cmp::min(
            self.message_list.len() as i32 - 1,
            std::cmp::max(0, self.scroll_index as i32 + amount),
        ) as usize
    }

    pub fn draw(&mut self) -> &Screen {
        if self.dirty {
            self.screen.clear();
            for (index, message) in self.message_list.iter().enumerate() {
                if index >= self.scroll_index {
                    message.print(&mut self.screen, 0, (index - self.scroll_index) as i32)
                }
            }
            self.dirty = false;
        }
        &self.screen
    }
}

pub struct InputWindow {
    screen: Screen,
    dirty: bool,
    input_buffer: String,
}

impl InputWindow {
    pub fn new(w: u32) -> Self {
        Self {
            screen: Screen::new(w, 1),
            dirty: true,
            input_buffer: String::new(),
        }
    }

    pub fn resize(&mut self, w: u32) {
        self.dirty = true;
        self.screen.resize(w, 1)
    }

    pub fn set_content(&mut self, content: &str) {
        self.dirty = self.input_buffer != content;
        self.input_buffer = content.to_string();
    }

    pub fn draw(&mut self) -> &Screen {
        if self.dirty {
            self.screen.clear();
            self.screen.print(0, 0, &self.input_buffer);
            self.dirty = false;
        }
        &self.screen
    }
}
