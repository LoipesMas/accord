use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};

pub trait Packet {
    fn serialized(&self) -> Vec<u8>;
    fn deserialized(buf: &[u8]) -> Result<(Self, &[u8]), rmp_serde::decode::Error>
    where
        Self: std::marker::Sized;
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerboundPacket {
    Ping,
    Login { username: String, password: String },
    Message(String),
    Command(String),
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClientboundPacket {
    Pong,
    LoginAck,
    UserJoined(String),
    UserLeft(String),
    UsersOnline(Vec<String>),
    Message {
        text: String,
        sender: String,
        time: u64,
    },
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
