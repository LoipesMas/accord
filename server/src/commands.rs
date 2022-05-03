/// Commands used internally for communication between connections and channel loop
use accord::packets::*;
use std::net::SocketAddr;

use tokio::sync::{mpsc::Sender, oneshot::Sender as OSender};

#[derive(Debug)]
pub struct UserPermissions {
    pub operator: bool,
    pub whitelisted: bool,
    pub banned: bool,
}

#[derive(Debug)]
pub enum ConnectionCommand {
    Write(ClientboundPacket),
    SetSecret(Option<Vec<u8>>),
    Close,
}

#[derive(Debug)]
pub enum ChannelCommand {
    Close,
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
    UsersQueryTUI(OSender<Vec<String>>),
    FetchMessages(i64, i64, OSender<Vec<ClientboundPacket>>),
    CheckPermissions(String, OSender<UserPermissions>),
    KickUser(String),
    BanUser(String, bool),
    WhitelistUser(String, bool),
    SetWhitelist(bool),
    SetAllowNewAccounts(bool),
}

pub type LoginResult = Result<String, String>;
