use accord::packets::*;
use std::net::SocketAddr;

#[derive(Debug)]
pub enum ServerConnectionCommands {
    Write(ClientboundPacket),
    Close,
}

#[derive(Debug)]
pub enum ChannelCommands {
    Write(ClientboundPacket),
    NewConnection(
        tokio::sync::mpsc::Sender<ServerConnectionCommands>,
        SocketAddr,
    ),
    UserJoined(String, SocketAddr),
    UserLeft(SocketAddr),
}
