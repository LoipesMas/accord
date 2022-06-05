use std::collections::BTreeSet;

use chrono::TimeZone;
use console_engine::{pixel, screen::Screen, Color};

use crate::{THEME_BG, THEME_FG};

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
    Text(accord::packets::Message),
    Image(accord::packets::ImageMessage),
    System(String),
    Error(String),
}

impl Message {
    /// Prints the stored message, and return how many lines was required for printing it entirely
    pub fn print(&self, screen: &mut Screen, x: i32, y: i32) -> i32 {
        match self {
            Message::Text(message) => {
                let time = chrono::Local.timestamp(message.time as i64, 0);
                let mut lines = 1;
                let text = format!(
                    "[{}] {}: {}",
                    time.format("%H:%M %d-%m"),
                    message.sender,
                    message.text
                )
                .chars()
                .enumerate()
                .flat_map(|(i, chr)| {
                    if i != 0 && i % screen.get_width() as usize == 0 {
                        lines += 1;
                        Some('\n')
                    } else {
                        None
                    }
                    .into_iter()
                    .chain(std::iter::once(chr))
                })
                .collect::<String>();
                screen.print_fbg(x, y, &text, THEME_FG, THEME_BG);
                lines
            }
            Message::Image(message) => {
                let time = chrono::Local.timestamp(message.time as i64, 0);
                let mut lines = 1;
                let text = format!(
                    "[{}] {}: [Image]",
                    time.format("%H:%M %d-%m"),
                    message.sender
                )
                .chars()
                .enumerate()
                .flat_map(|(i, chr)| {
                    if i != 0 && i % screen.get_width() as usize == 0 {
                        lines += 1;
                        Some('\n')
                    } else {
                        None
                    }
                    .into_iter()
                    .chain(std::iter::once(chr))
                })
                .collect::<String>();
                screen.print_fbg(x, y, &text, THEME_FG, THEME_BG);
                lines
            }
            Message::System(message) => {
                let mut lines = 1;
                let message = &message
                    .chars()
                    .enumerate()
                    .flat_map(|(i, chr)| {
                        if i != 0 && i % screen.get_width() as usize == 0 {
                            lines += 1;
                            Some('\n')
                        } else {
                            None
                        }
                        .into_iter()
                        .chain(std::iter::once(chr))
                    })
                    .collect::<String>();
                screen.print_fbg(x, y, message, Color::DarkGrey, THEME_BG);
                lines
            }
            Message::Error(message) => {
                let mut lines = 1;
                let message = &message
                    .chars()
                    .enumerate()
                    .flat_map(|(i, chr)| {
                        if i != 0 && i % screen.get_width() as usize == 0 {
                            lines += 1;
                            Some('\n')
                        } else {
                            None
                        }
                        .into_iter()
                        .chain(std::iter::once(chr))
                    })
                    .collect::<String>();
                screen.print_fbg(x, y, message, Color::Red, THEME_BG);
                lines
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
            self.screen.fill(pixel::pxl_fbg(' ', THEME_FG, THEME_BG));
            self.screen.line(
                0,
                0,
                self.screen.get_width() as i32,
                0,
                pixel::pxl_fbg(' ', THEME_BG, THEME_FG),
            );
            self.screen.print_fbg(0, 0, " Users", THEME_BG, THEME_FG);
            for (index, username) in self.user_list.iter().enumerate() {
                self.screen
                    .print_fbg(0, index as i32 + 1, username, THEME_FG, THEME_BG);
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
        self.message_list.push(message);
        if self.scroll_index + (self.screen.get_height() as usize) == self.message_list.len() {
            self.scroll_index += 1;
        }
    }

    pub fn scroll(&mut self, amount: i32) {
        self.dirty = true;
        self.scroll_index = (self.scroll_index as i32 + amount)
            .clamp(0, self.message_list.len() as i32 - 1) as usize;
    }

    pub fn draw(&mut self) -> &Screen {
        if self.dirty {
            self.screen.fill(pixel::pxl_fbg(' ', THEME_FG, THEME_BG));
            let mut pos = 0;
            for message in self.message_list.iter().skip(self.scroll_index) {
                pos += message.print(&mut self.screen, 0, pos);
                if pos > self.screen.get_height() as i32 {
                    break;
                }
            }
            self.dirty = false;
        }
        &self.screen
    }
}
