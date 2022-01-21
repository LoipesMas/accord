use std::marker::PhantomData;

use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

use crate::packets::*;

// I = Incoming Packets
// O = Outgoing Packets
pub struct Connection<I, O> {
    stream: TcpStream,
    _marker: PhantomData<(I, O)>,
}

pub struct ConnectionReader<P: Packet> {
    stream: OwnedReadHalf,
    buffer: BytesMut,
    _marker: PhantomData<P>,
}

pub struct ConnectionWriter<P: Packet> {
    stream: BufWriter<OwnedWriteHalf>,
    _marker: PhantomData<P>,
}

impl<I, O> Connection<I, O>
where
    I: Packet,
    O: Packet,
{
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            _marker: PhantomData,
        }
    }

    /// Splits stream to separate handles so they can be used in different tasks
    pub fn split(self) -> (ConnectionReader<I>, ConnectionWriter<O>) {
        let (read, write) = self.stream.into_split();
        let read = ConnectionReader::<I> {
            stream: read,
            buffer: BytesMut::with_capacity(4096),
            _marker: PhantomData,
        };
        let write = ConnectionWriter::<O> {
            stream: BufWriter::new(write),
            _marker: PhantomData,
        };
        (read, write)
    }
}

impl<P: Packet> ConnectionReader<P> {
    pub async fn read_packet(&mut self) -> Result<Option<P>, String> {
        loop {
            if let Ok((p, b)) = P::deserialized(&self.buffer) {
                // Effectively move buffer past what we already read
                self.buffer = BytesMut::from(b);
                return Ok(Some(p));
            }

            if 0 == self
                .stream
                .read_buf(&mut self.buffer)
                .await
                .map_err(|e| e.to_string())?
            {
                return Err("Connection reset by peer".into());
            }
        }
    }
}

impl<P: Packet> ConnectionWriter<P> {
    pub async fn write_packet(&mut self, packet: P) -> std::io::Result<()> {
        let p = packet.serialized();
        self.stream.write_all(&p).await?;
        self.stream.flush().await
    }
}
