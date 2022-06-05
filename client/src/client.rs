use std::{error::Error, net::SocketAddr};

use accord::{
    connection::{Connection, ConnectionReader, ConnectionWriter},
    packets::{ClientboundPacket, ServerboundPacket},
};
use accord::{ENC_TOK_LEN, SECRET_LEN};

use rand::{rngs::OsRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use rsa::{PaddingScheme, PublicKey};
use tokio::net::TcpStream;

#[derive(Debug)]
struct ClientError(String);

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Client error: {}", self.0)
    }
}

impl Error for ClientError {}

pub struct Client {
    pub reader: ConnectionReader<ClientboundPacket>,
    pub writer: ConnectionWriter<ServerboundPacket>,
    pub secret: Option<Vec<u8>>,
    pub nonce_generator_read: Option<ChaCha20Rng>,
    pub nonce_generator_write: Option<ChaCha20Rng>,
}

impl Client {
    pub async fn init(addr: SocketAddr) -> Result<Self, Box<dyn Error>> {
        let socket = TcpStream::connect(addr).await?;

        let connection = Connection::<ClientboundPacket, ServerboundPacket>::new(socket);
        let (mut reader, mut writer) = connection.split();

        //==================================
        //      Encryption
        //==================================
        // Establishing encryption
        let secret = None;
        let mut nonce_generator_write = None;
        let mut nonce_generator_read = None;

        // Request encryption
        writer
            .write_packet(
                ServerboundPacket::EncryptionRequest,
                &secret,
                nonce_generator_write.as_mut(),
            )
            .await?;

        // Handle encryption response
        let pub_key: rsa::RsaPublicKey;
        let token = if let Ok(Some(p)) = reader
            .read_packet(&secret, nonce_generator_read.as_mut())
            .await
        {
            match p {
                ClientboundPacket::EncryptionResponse(pub_key_der, token_) => {
                    pub_key = rsa::pkcs8::FromPublicKey::from_public_key_der(&pub_key_der)?;
                    assert_eq!(ENC_TOK_LEN, token_.len());
                    token_
                }
                _ => {
                    return Err(Box::new(ClientError(format!(
                        "Encryption failed. Server response: {:?}",
                        p
                    ))));
                }
            }
        } else {
            return Err(Box::new(ClientError(String::from(
                "Failed to establish encryption",
            ))));
        };

        // Generate secret
        let mut secret = [0u8; SECRET_LEN];
        OsRng.fill(&mut secret);

        // Encrypt and send
        let padding = PaddingScheme::new_pkcs1v15_encrypt();
        let enc_secret = pub_key
            .encrypt(&mut OsRng, padding, &secret[..])
            .expect("failed to encrypt");
        let padding = PaddingScheme::new_pkcs1v15_encrypt();
        let enc_token = pub_key
            .encrypt(&mut OsRng, padding, &token[..])
            .expect("failed to encrypt");
        writer
            .write_packet(
                ServerboundPacket::EncryptionConfirm(enc_secret, enc_token),
                &None,
                nonce_generator_write.as_mut(),
            )
            .await?;

        // From this point onward we assume everything is encrypted
        let secret = Some(secret.to_vec());
        let mut seed = [0u8; accord::SECRET_LEN];
        seed.copy_from_slice(&secret.as_ref().unwrap()[..]);
        nonce_generator_write = Some(ChaCha20Rng::from_seed(seed));
        nonce_generator_read = Some(ChaCha20Rng::from_seed(seed));

        // Expect EncryptionAck (should be encrypted)
        let p = reader
            .read_packet(&secret, nonce_generator_read.as_mut())
            .await;
        match p {
            Ok(Some(ClientboundPacket::EncryptionAck)) => {}
            Ok(_) => {
                return Err(Box::new(ClientError(format!(
                    "Failed encryption step 2. Server response: {:?}",
                    p
                ))));
            }
            Err(e) => {
                return Err(Box::new(ClientError(e)));
            }
        }
        Ok(Self {
            reader,
            writer,
            secret,
            nonce_generator_read,
            nonce_generator_write,
        })
    }

    pub async fn login(
        &mut self,
        username: String,
        password: String,
    ) -> Result<(), Box<dyn Error>> {
        self.writer
            .write_packet(
                ServerboundPacket::Login {
                    username: username.to_string(),
                    password: password.to_string(),
                },
                &self.secret,
                self.nonce_generator_write.as_mut(),
            )
            .await?;

        // Next packet must be login related
        if let Ok(Some(p)) = self
            .reader
            .read_packet(&self.secret, self.nonce_generator_read.as_mut())
            .await
        {
            match p {
                ClientboundPacket::LoginAck => Ok(()),
                ClientboundPacket::LoginFailed(m) => {
                    Err(Box::new(ClientError(format!("Login failed: {}", m))))
                }
                _ => Err(Box::new(ClientError(format!(
                    "Login failed. Server response: {:?}",
                    p
                )))),
            }
        } else {
            Err(Box::new(ClientError(String::from("Failed to login"))))
        }
    }

    #[allow(dead_code)]
    pub async fn send(&mut self, packet: ServerboundPacket) -> Result<(), std::io::Error> {
        self.writer
            .write_packet(packet, &self.secret, self.nonce_generator_write.as_mut())
            .await
    }
    #[allow(dead_code)]
    pub async fn read(&mut self) -> Result<Option<ClientboundPacket>, String> {
        self.reader
            .read_packet(&self.secret, self.nonce_generator_read.as_mut())
            .await
    }

    pub fn breakdown(self) -> (ClientReader, ClientWriter) {
        (
            ClientReader {
                reader: self.reader,
                secret: self.secret.clone(),
                nonce_generator: self.nonce_generator_read,
            },
            ClientWriter {
                writer: self.writer,
                secret: self.secret,
                nonce_generator: self.nonce_generator_write,
            },
        )
    }
}

pub struct ClientWriter {
    pub writer: ConnectionWriter<ServerboundPacket>,
    pub secret: Option<Vec<u8>>,
    pub nonce_generator: Option<ChaCha20Rng>,
}
impl ClientWriter {
    pub async fn send(&mut self, packet: ServerboundPacket) -> Result<(), std::io::Error> {
        self.writer
            .write_packet(packet, &self.secret, self.nonce_generator.as_mut())
            .await
    }
}

pub struct ClientReader {
    pub reader: ConnectionReader<ClientboundPacket>,
    pub secret: Option<Vec<u8>>,
    pub nonce_generator: Option<ChaCha20Rng>,
}
impl ClientReader {
    pub async fn read(&mut self) -> Result<Option<ClientboundPacket>, String> {
        self.reader
            .read_packet(&self.secret, self.nonce_generator.as_mut())
            .await
    }
}
