use accord::packets::*;
use std::net::SocketAddr;

use tokio::sync::{mpsc::Sender, oneshot::Sender as OSender};

#[derive(Debug)]
pub enum ConnectionCommands {
    Write(ClientboundPacket),
    SetSecret(Option<Vec<u8>>),
    Close,
}

#[derive(Debug)]
pub enum ChannelCommands {
    Write(ClientboundPacket),
    NewConnection(Sender<ConnectionCommands>, SocketAddr),
    EncryptionRequest(Sender<ConnectionCommands>, OSender<Vec<u8>>),
    // Maybe this should be a struct?
    EncryptionConfirm(
        Sender<ConnectionCommands>,
        OSender<Result<Vec<u8>, ()>>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
    ), // encrypted secret, encrypted token and expected token
    LoginAttempt {
        username: String,
        password: String,
        addr: SocketAddr,
        otx: OSender<LoginOneshotCommand>,
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
