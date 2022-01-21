use chrono::TimeZone;
use std::str::FromStr;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use accord::connection::*;

use accord::packets::*;

use std::net::SocketAddr;

use tokio::sync::oneshot;

#[tokio::main]
async fn main() {
    let mut args = std::env::args();
    let addr = SocketAddr::from_str(&format!(
        "{}:{}",
        args.nth(1).unwrap_or_else(|| "127.0.0.1".to_string()),
        accord::DEFAULT_PORT
    ))
    .unwrap();
    let mut stdio = tokio::io::stdin();
    let username = loop {
        println!("Username:");
        let mut buf = bytes::BytesMut::new();
        match stdio.read_buf(&mut buf).await {
            Ok(0 | 1) => println!("Username can't be empty!"),
            Ok(_) => {
                break String::from_utf8_lossy(buf.strip_suffix(b"\n").unwrap()).to_string();
            }
            Err(e) => println!("Error: {:?}", e),
        };
    };
    println!("Hello {}!", username);
    println!("Connecting to: {}", addr);
    let socket = TcpStream::connect(addr).await.unwrap();

    println!("Connected!");
    let connection = Connection::<ClientboundPacket, ServerboundPacket>::new(socket);
    let (mut reader, mut writer) = connection.split();
    println!("Logging in...");
    writer
        .write_packet(ServerboundPacket::Login {
            username,
            password: "".to_string(),
        })
        .await
        .unwrap();

    if let Ok(Some(p)) = reader.read_packet().await {
        match p {
            ClientboundPacket::LoginAck => {
                println!("Login succesful");
            }
            _ => {
                println!("Login failed. Server response: {:?}", p);
            }
        }
    } else {
        println!("Failed to login ;/");
        std::process::exit(1);
    }

    // To send close command when tcpstream is closed
    let (tx, rx) = oneshot::channel::<()>();

    tokio::join!(reading_loop(reader, tx), writing_loop(writer, rx));
}

async fn reading_loop(
    mut reader: ConnectionReader<ClientboundPacket>,
    close_sender: oneshot::Sender<()>,
) {
    'l: loop {
        //println!("reading packet");
        match reader.read_packet().await {
            Ok(Some(ClientboundPacket::Message { text, sender, time })) => {
                let time = chrono::Local.timestamp(time as i64, 0);
                println!("{} ({}): {}", sender, time.format("%H:%M %d-%m"), text);
            }
            Ok(Some(ClientboundPacket::UserJoined(username))) => {
                println!("{} joined the channel", username);
            }
            Ok(Some(ClientboundPacket::UserLeft(username))) => {
                println!("{} left the channel", username);
            }
            Ok(Some(p)) => {
                println!("!!Unhandled packet: {:?}", p);
            }
            Err(e) => {
                println!("{}", e);
                close_sender.send(()).unwrap();
                break 'l;
            }
            _ => {
                println!("Connection closed(?)\nPress Enter to exit.");
                close_sender.send(()).unwrap();
                break 'l;
            }
        }
    }
}
async fn writing_loop(
    mut writer: ConnectionWriter<ServerboundPacket>,
    mut close_receiver: oneshot::Receiver<()>,
) {
    let mut stdio = tokio::io::stdin();
    let mut buf = bytes::BytesMut::new();
    loop {
        buf.clear();
        tokio::select!(
            r = stdio.read_buf(&mut buf) => {
                if r.is_ok() {
                    let s = String::from_utf8_lossy(buf.strip_suffix(b"\n").unwrap()).to_string();
                    if s.chars().any(|c| c.is_control()) {
                        println!("Invalid message text!");
                        continue;
                    }

                    if !s.is_empty() {
                        let p = ServerboundPacket::Message(s);
                        writer.write_packet(p).await.unwrap();
                        // Clear input line
                        print!("\r\u{1b}[A");
                    }
                }
            }
            _ = &mut close_receiver => {
                break;
            }
        );
    }
}
