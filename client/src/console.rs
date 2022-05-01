use std::{cmp::Ordering, collections::BTreeSet};

use chrono::TimeZone;
use console_engine::{pixel, rect_style::BorderStyle, screen::Screen, Color};

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
                screen.print(x, y, &text);
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
                screen.print(x, y, &text);
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
                screen.print_fbg(x, y, message, Color::DarkGrey, Color::Reset);
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
                screen.print_fbg(x, y, message, Color::Red, Color::Reset);
                lines
            }
        }
    }
}

enum LoginWindowState {
    InputUsername,
    InputPassword,
    Finished,
}

pub struct LoginWindow {
    screen: Screen,
    dirty: bool,
    state: LoginWindowState,
    w_username: InputWindow,
    w_password: InputWindow,
}

impl LoginWindow {
    pub fn new(w: u32) -> Self {
        Self {
            screen: Screen::new(w, 7),
            dirty: true,
            state: LoginWindowState::InputUsername,
            w_username: InputWindow::new(w - 2),
            w_password: InputWindow::new(w - 2),
        }
    }
    pub fn resize(&mut self, w: u32) {
        self.dirty = true;
        self.screen.resize(w, 7);
        self.w_username.resize(w - 2);
        self.w_password.resize(w - 2);
    }

    pub fn get_active_prompt(&mut self) -> Option<&mut InputWindow> {
        self.dirty = true;
        match self.state {
            LoginWindowState::InputUsername => Some(&mut self.w_username),
            LoginWindowState::InputPassword => Some(&mut self.w_password),
            LoginWindowState::Finished => None,
        }
    }

    pub fn reset(&mut self) {
        self.state = LoginWindowState::InputUsername;
        self.w_username.clear_input_buffer();
        self.w_password.clear_input_buffer();
        self.dirty = true;
    }

    pub fn submit(&mut self) {
        self.dirty = true;
        self.state = match self.state {
            LoginWindowState::InputUsername => LoginWindowState::InputPassword,
            LoginWindowState::InputPassword => LoginWindowState::Finished,
            LoginWindowState::Finished => LoginWindowState::Finished,
        }
    }

    pub fn is_prompting_username(&self) -> bool {
        matches!(self.state, LoginWindowState::InputUsername)
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.state, LoginWindowState::Finished)
    }

    pub fn get_username(&self) -> &String {
        self.w_username.get_input_buffer()
    }
    pub fn get_password(&self) -> &String {
        self.w_password.get_input_buffer()
    }

    pub fn draw(&mut self, frame_count: usize) -> &Screen {
        if self.dirty {
            self.screen.clear();
            self.screen.rect_border(
                0,
                0,
                self.screen.get_width() as i32 - 1,
                self.screen.get_height() as i32 - 1,
                BorderStyle::new_heavy(),
            );
            self.screen.print(1, 1, "Username:");
            self.screen.print_screen(
                1,
                2,
                self.w_username
                    .draw(if let LoginWindowState::InputUsername = self.state {
                        frame_count
                    } else {
                        0
                    }),
            );
            self.screen.print(1, 4, "Password:");
            self.screen.print_screen(
                1,
                5,
                self.w_password
                    .draw(if let LoginWindowState::InputPassword = self.state {
                        frame_count
                    } else {
                        0
                    }),
            );
        }
        &self.screen
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
            self.screen.clear();
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

pub struct InputWindow {
    screen: Screen,
    dirty: bool,
    input_buffer: String,
    cursor_pos: usize,
}

impl InputWindow {
    pub fn new(w: u32) -> Self {
        Self {
            screen: Screen::new(w, 1),
            dirty: true,
            input_buffer: String::new(),
            cursor_pos: 0,
        }
    }

    pub fn resize(&mut self, w: u32) {
        self.dirty = true;
        self.screen.resize(w, 1)
    }

    // pub fn set_content(&mut self, content: &str) {
    //     self.dirty = self.input_buffer != content;
    //     self.input_buffer = content.to_string();
    //     self.cursor_pos = self.cursor_pos.clamp(0, self.input_buffer.len());
    // }

    pub fn get_input_buffer(&self) -> &String {
        &self.input_buffer
    }

    pub fn clear_input_buffer(&mut self) {
        self.dirty = true;
        self.input_buffer = String::new();
        self.cursor_pos = 0;
    }

    pub fn put_char(&mut self, chr: char) {
        self.dirty = true;
        let mut new_buffer = self
            .input_buffer
            .chars()
            .take(self.cursor_pos)
            .collect::<String>();
        new_buffer.push(chr);
        new_buffer.push_str(
            &self
                .input_buffer
                .chars()
                .skip(self.cursor_pos)
                .collect::<String>(),
        );
        self.input_buffer = new_buffer;
        self.move_cursor(1);
    }

    pub fn remove_char(&mut self, amount: i32) {
        match amount.cmp(&0) {
            Ordering::Greater => {
                self.dirty = true;
                let mut new_buffer = self
                    .input_buffer
                    .chars()
                    .take((self.cursor_pos as i32 - amount) as usize)
                    .collect::<String>();
                new_buffer.push_str(
                    &self
                        .input_buffer
                        .chars()
                        .skip(self.cursor_pos)
                        .collect::<String>(),
                );
                self.input_buffer = new_buffer;
                self.move_cursor(-amount);
            }
            Ordering::Less => {
                self.dirty = true;
                let mut new_buffer = self
                    .input_buffer
                    .chars()
                    .take(self.cursor_pos)
                    .collect::<String>();
                new_buffer.push_str(
                    &self
                        .input_buffer
                        .chars()
                        .skip((self.cursor_pos as i32 - amount) as usize)
                        .collect::<String>(),
                );
                self.input_buffer = new_buffer;
            }
            Ordering::Equal => {}
        }
    }

    pub fn move_cursor(&mut self, amount: i32) {
        self.dirty = true;
        self.cursor_pos = (self.cursor_pos as i64 + amount as i64)
            .clamp(0, self.input_buffer.len() as i64) as usize;
    }

    pub fn draw(&mut self, frame_count: usize) -> &Screen {
        if self.dirty {
            self.screen.clear();
            self.screen.print(0, 0, &self.input_buffer);
            self.dirty = false;
        }
        if frame_count % 8 >= 4 {
            if let Ok(mut cursor_pxl) = self.screen.get_pxl(self.cursor_pos as i32, 0) {
                cursor_pxl.bg = Color::Grey;
                cursor_pxl.fg = Color::Black;
                self.screen.set_pxl(self.cursor_pos as i32, 0, cursor_pxl);
            }
        } else if let Ok(mut cursor_pxl) = self.screen.get_pxl(self.cursor_pos as i32, 0) {
            cursor_pxl.bg = Color::Reset;
            cursor_pxl.fg = Color::Reset;
            self.screen.set_pxl(self.cursor_pos as i32, 0, cursor_pxl);
        }
        &self.screen
    }
}
