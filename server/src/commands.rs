use accord::packets::*;
use std::net::SocketAddr;

use tokio::sync::oneshot::Sender;

#[derive(Debug)]
pub enum ConnectionCommands {
    Write(ClientboundPacket),
    Close,
}

#[derive(Debug)]
pub enum ChannelCommands {
    Write(ClientboundPacket),
    NewConnection(tokio::sync::mpsc::Sender<ConnectionCommands>, SocketAddr),
    LoginAttempt {
        username: String,
        password: String,
        addr: SocketAddr,
        otx: Sender<LoginOneshotCommand>,
    },
    UserJoined(String),
    UserLeft(SocketAddr),
    UsersQuery(SocketAddr),
}

#[derive(Debug)]
pub enum LoginOneshotCommand {
    Success(String),
    Failed(String),
}
