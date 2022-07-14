use accord_server::commands::ChannelCommand;
use futures::{FutureExt, StreamExt};
use tokio::sync::mpsc;

use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use std::io::{self, Stdout};
use tui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};

use crate::logging::LogEntry;

/// Main TUI struct
pub struct Tui {
    logs_rx: mpsc::Receiver<LogEntry>,
    logs: Vec<LogEntry>,
    scroll: usize,
    event_stream: EventStream,
    commandline: String,
    channel_sender: mpsc::Sender<ChannelCommand>,
    terminal: Option<Terminal<CrosstermBackend<Stdout>>>,
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Restore terminal on drop
        disable_raw_mode().unwrap();
        if let Some(terminal) = &mut self.terminal {
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )
            .unwrap();
        }
    }
}

impl Tui {
    pub fn new(
        logs_rx: mpsc::Receiver<LogEntry>,
        channel_sender: mpsc::Sender<ChannelCommand>,
    ) -> Self {
        Self {
            logs_rx,
            channel_sender,
            logs: Vec::new(),
            scroll: 0,
            event_stream: EventStream::new(),
            commandline: String::new(),
            terminal: None,
        }
    }

    /// Launches the TUI, starting the main loop in new thread
    /// and returns a handle to that task.
    pub fn launch(mut self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            enable_raw_mode().unwrap();

            let mut stdout = io::stdout();
            execute!(stdout, EnterAlternateScreen, EnableMouseCapture).unwrap();
            let backend = CrosstermBackend::new(stdout);
            let terminal = Terminal::new(backend).unwrap();
            self.terminal.replace(terminal);
            loop {
                if self.main_loop().await {
                    break;
                };
            }
            drop(self);
        })
    }

    /// Main loop of TUI
    /// Handles incoming terminal events and log updates.
    ///
    /// Returns whether the loop should be stopped.
    async fn main_loop(&mut self) -> bool {
        let incoming_log = self.logs_rx.recv();
        let event = self.event_stream.next().fuse();
        let exit_event = KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
        };
        tokio::select! {
            maybe_log = incoming_log =>  {
                match maybe_log {
                    Some(log_entry) => {
                        self.logs.push(log_entry);
                    }
                    None => panic!("Log writer dropped before TUI!"),
                }
            },
            maybe_event = event => {
                match maybe_event {
                    Some(Ok(event)) => {
                        if let Event::Key(kevent) = event {
                            if kevent == exit_event {
                                self.respond("Enter 'exit' command to exit.");
                                return false;
                            }
                            if let KeyEvent{code: KeyCode::Char(c), modifiers: _} = kevent {
                                self.commandline.push(c);
                            }
                            if kevent == KeyCode::Backspace.into() {
                                self.commandline.pop();
                            }
                            if kevent == KeyCode::Enter.into() {
                                return self.try_command().await;
                            }
                            if kevent == KeyCode::Up.into() {
                                self.scroll = self.scroll.saturating_sub(1);
                            }
                            if kevent == KeyCode::Down.into() {
                                self.scroll = self.scroll.saturating_add(1).min(self.logs.len()-1);
                            }
                            if kevent == KeyCode::PageUp.into() {
                                self.scroll = self.scroll.saturating_sub(10);
                            }
                            if kevent == KeyCode::PageDown.into() {
                                self.scroll = self.scroll.saturating_add(10).min(self.logs.len()-1);
                            }
                            if kevent == KeyCode::Home.into() {
                                self.scroll = 0;
                            }
                            if kevent == KeyCode::End.into() {
                                self.scroll = self.logs.len().saturating_sub(1);
                            }
                            if kevent == KeyCode::Up.into() {
                                self.scroll = self.scroll.saturating_sub(1);
                            }

                        }
                    }
                    Some(Err(e)) => log::error!("Error while getting event: {}", e),
                    None => return true,
            }
            }
        };

        if let Some(mut terminal) = self.terminal.take() {
            terminal.draw(|f| self.draw(f)).unwrap();
            self.terminal.replace(terminal);
        }

        false
    }

    /// Draws TUI
    fn draw(&mut self, frame: &mut Frame<CrosstermBackend<io::Stdout>>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(frame.size().height - 3),
                    Constraint::Min(3),
                ]
                .as_ref(),
            )
            .split(frame.size());

        // Log items
        let items: Vec<ListItem> = self
            .logs
            .iter()
            .skip(self.scroll)
            .map(|l| {
                let mut spans = vec![];
                let style = style_from_level(l.level);
                let def_style = Style::default().fg(Color::Gray);
                spans.push(Span::styled(
                    l.level.to_string(),
                    style.add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(" [", def_style));
                spans.push(Span::styled(&l.target, def_style));
                spans.push(Span::styled("] ", def_style));
                spans.push(Span::styled(&l.args, style));
                let spans = Spans::from(spans);
                ListItem::new(spans)
            })
            .collect();
        let items = List::new(items).block(
            Block::default()
                .borders(Borders::ALL.difference(Borders::BOTTOM))
                .title("Log"),
        );
        frame.render_widget(items, chunks[0]);
        let input = Paragraph::new(self.commandline.as_str())
            .block(Block::default().borders(Borders::ALL).title("Commandline"));
        frame.set_cursor(
            chunks[1].x + 1 + self.commandline.len() as u16,
            chunks[1].y + 1,
        );
        frame.render_widget(input, chunks[1]);
    }

    /// Consumes the commandline input and tries to use it as a command.
    ///
    /// Returns whether the command was an exit command.
    async fn try_command(&mut self) -> bool {
        if self.commandline.is_empty() {
            return false;
        }
        let mut command = String::new();
        std::mem::swap(&mut command, &mut self.commandline);
        let command = command.trim_start_matches('/');
        //TODO: abstract this code more
        let mut split = command.split(' ');
        if let Some(command) = split.next() {
            match command {
                "exit" => {
                    log::info!("Exiting...");
                    return true;
                }
                "list" => {
                    let (otx, orx) = tokio::sync::oneshot::channel();

                    self.channel_sender
                        .send(ChannelCommand::UsersQueryTUI(otx))
                        .await
                        .unwrap();

                    match orx.await {
                        Ok(list) => log::info!("Connected users: {:?}", list),
                        Err(e) => log::error!("Error while receiving user list in TUI: {}", e),
                    }
                }
                "kick" => {
                    let m = if let Some(target) = split.next() {
                        self.channel_sender
                            .send(ChannelCommand::KickUser(target.to_owned()))
                            .await
                            .unwrap();
                        format!("Kicking {}.", target)
                    } else {
                        "No target provided".to_owned()
                    };
                    self.respond(m);
                }
                "ban" => {
                    self.ban_command(split.next(), true).await;
                }
                "unban" => {
                    self.ban_command(split.next(), false).await;
                }
                "whitelist" => {
                    self.whitelist_command(split.next(), true).await;
                }
                "unwhitelist" => {
                    self.whitelist_command(split.next(), false).await;
                }
                "set_whitelist" => {
                    let m = if let Some(arg) = split.next() {
                        match arg {
                            "on" | "true" => {
                                self.channel_sender
                                    .send(ChannelCommand::SetWhitelist(true))
                                    .await
                                    .unwrap();
                                "Whitelist on.".to_string()
                            }
                            "off" | "false" => {
                                self.channel_sender
                                    .send(ChannelCommand::SetWhitelist(false))
                                    .await
                                    .unwrap();
                                "Whitelist off.".to_string()
                            }
                            _ => {
                                format!("Invalid argument: {}.\nExpected \"on\"/\"off\"", arg)
                            }
                        }
                    } else {
                        "No argument provided".to_string()
                    };
                    self.respond(m);
                }
                "set_allow_new_accounts" => {
                    let m = if let Some(arg) = split.next() {
                        match arg {
                            "on" | "true" => {
                                self.channel_sender
                                    .send(ChannelCommand::SetAllowNewAccounts(true))
                                    .await
                                    .unwrap();
                                "Allow new accounts on.".to_string()
                            }
                            "off" | "false" => {
                                self.channel_sender
                                    .send(ChannelCommand::SetAllowNewAccounts(false))
                                    .await
                                    .unwrap();
                                "Allow new accounts off.".to_string()
                            }
                            _ => {
                                format!("Invalid argument: {}.\nExpected \"on\"/\"off\"", arg)
                            }
                        }
                    } else {
                        "No argument provided".to_string()
                    };
                    self.respond(m);
                }
                c => {
                    self.respond(format!("Unknown command: {}", c));
                }
            }
        };
        false
    }

    /// switch == true => ban
    /// switch == false => unban
    async fn ban_command(&mut self, target: Option<&str>, switch: bool) {
        let m = if let Some(target) = target {
            self.channel_sender
                .send(ChannelCommand::BanUser(target.to_owned(), switch))
                .await
                .unwrap();
            if switch {
                format!("Banning {}", target)
            } else {
                format!("Unbanning {}.", target)
            }
        } else {
            "No target provided".to_owned()
        };
        self.respond(m);
    }

    /// switch == true => add to whitelist
    /// switch == false => remove from whitelist
    async fn whitelist_command(&mut self, target: Option<&str>, switch: bool) {
        let m = if let Some(target) = target {
            self.channel_sender
                .send(ChannelCommand::WhitelistUser(target.to_owned(), switch))
                .await
                .unwrap();
            if switch {
                format!("Whitelisting {}.", target)
            } else {
                format!("Unwhitelisting {}.", target)
            }
        } else {
            "No target provided".to_owned()
        };
        self.respond(m);
    }

    // I don't remember why does this exist
    fn respond<T: std::fmt::Display>(&mut self, s: T) {
        log::info!("{}", s);
    }
}

fn style_from_level(level: log::Level) -> Style {
    match level {
        flexi_logger::Level::Error => Style::default().fg(Color::Red),
        flexi_logger::Level::Warn => Style::default().fg(Color::Yellow),
        flexi_logger::Level::Info => Style::default(),
        flexi_logger::Level::Debug => Style::default().fg(Color::Green),
        flexi_logger::Level::Trace => Style::default().fg(Color::Cyan),
    }
}
