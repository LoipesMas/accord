use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Clone, Deserialize, Serialize)]
pub struct Message {
    pub sender: String,
    pub text: String,
    pub time: u64,
}

pub trait Packet {
    fn serialized(&self) -> Vec<u8>;
    fn deserialized(buf: &[u8]) -> Result<(Self, &[u8]), rmp_serde::decode::Error>
    where
        Self: std::marker::Sized;
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum ServerboundPacket {
    Ping,
    EncryptionRequest,
    EncryptionConfirm(Vec<u8>, Vec<u8>), // encrypted secret and token
    Login { username: String, password: String },
    Message(String),
    Command(String),
    FetchMessages(i64, i64),
}

impl Packet for ServerboundPacket {
    fn serialized(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize(&mut Serializer::new(&mut buf)).unwrap();
        buf
    }

    fn deserialized(buf: &[u8]) -> Result<(Self, &[u8]), rmp_serde::decode::Error> {
        let mut d = Deserializer::new(buf);
        Self::deserialize(&mut d).map(|p| (p, d.into_inner()))
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum ClientboundPacket {
    Pong,
    EncryptionResponse(Vec<u8>, Vec<u8>), // channel's public key and token
    EncryptionAck,
    LoginAck,
    LoginFailed(String),
    UserJoined(String),
    UserLeft(String),
    UsersOnline(Vec<String>),
    Message(Message),
}

impl Packet for ClientboundPacket {
    fn serialized(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize(&mut Serializer::new(&mut buf)).unwrap();
        buf
    }

    fn deserialized(buf: &[u8]) -> Result<(Self, &[u8]), rmp_serde::decode::Error> {
        let mut d = Deserializer::new(buf);
        Self::deserialize(&mut d).map(|p| (p, d.into_inner()))
    }
}
