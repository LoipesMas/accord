/// Commands used internally for communication between connections and channel loop
use accord::packets::*;
use std::net::SocketAddr;

use tokio::sync::{mpsc::Sender, oneshot::Sender as OSender};

#[derive(Debug)]
pub enum ConnectionCommand {
    Write(ClientboundPacket),
    SetSecret(Option<Vec<u8>>),
    Close,
}

#[derive(Debug)]
pub enum ChannelCommand {
    Write(ClientboundPacket),
    EncryptionRequest(Sender<ConnectionCommand>, OSender<Vec<u8>>),
    // Maybe this should be a struct?
    EncryptionConfirm(
        Sender<ConnectionCommand>,
        OSender<Result<Vec<u8>, ()>>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
    ), // encrypted secret, encrypted token and expected token
    LoginAttempt {
        username: String,
        password: String,
        addr: SocketAddr,
        otx: OSender<LoginResult>,
        tx: Sender<ConnectionCommand>,
    },
    UserJoined(String),
    UserLeft(SocketAddr),
    UsersQuery(SocketAddr),
}

pub type LoginResult = Result<String, String>;
