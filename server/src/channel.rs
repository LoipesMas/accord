use accord::packets::*;
use accord::utils::verify_username;
use accord::{ENC_TOK_LEN, RSA_BITS};

use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};

use tokio_postgres::{Client as DBClient, NoTls};

use super::commands::*;

use rand::rngs::OsRng;
use rand::Rng;
use rand::RngCore;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use rsa::{pkcs8::ToPublicKey, PaddingScheme, RsaPrivateKey, RsaPublicKey};

#[derive(Debug)]
pub struct AccordChannel {
    receiver: Receiver<ChannelCommand>,
    txs: HashMap<std::net::SocketAddr, Sender<ConnectionCommand>>,
    connected_users: HashMap<std::net::SocketAddr, String>,
    salt_generator: ChaCha20Rng,
    db_client: DBClient,
    priv_key: RsaPrivateKey,
    pub_key: RsaPublicKey,
}

impl AccordChannel {
    pub async fn spawn(receiver: Receiver<ChannelCommand>) {
        // Setup
        let txs: HashMap<std::net::SocketAddr, Sender<ConnectionCommand>> = HashMap::new();
        let connected_users: HashMap<std::net::SocketAddr, String> = HashMap::new();
        let mut rng = OsRng;
        let priv_key = RsaPrivateKey::new(&mut rng, RSA_BITS).expect("failed to generate a key");
        let pub_key = RsaPublicKey::from(&priv_key);
        let config = crate::config::load_config();
        let (db_client, db_connection) = match tokio_postgres::connect(
            &format!(
                "host={} user={} password={}",
                config.db_host, config.db_user, config.db_pass
            ),
            NoTls,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                log::error!("Postgres connection error: {}\n(Make sure that the postgres server is running!)", e);
                std::process::exit(-1)
            }
        };

        tokio::spawn(async move {
            if let Err(e) = db_connection.await {
                log::error!("database connection error: {}", e);
            };
        });

        // Try to create account table
        // (we just error if it already exists :) )
        let _ = db_client
            .execute(
                "CREATE TABLE accounts (username varchar(255) NOT NULL UNIQUE, password varchar(44) NOT NULL, salt varchar(88) NOT NULL);",
                &[],
            )
            .await;

        // Try to create messages table
        // (we just error if it already exists :) )
        let _ = db_client
            .execute(
        "CREATE TABLE messages ( sender varchar(255) NOT NULL, content varchar(1023) NOT NULL, send_time bigint NOT NULL, CONSTRAINT fk_username FOREIGN KEY(sender) REFERENCES accounts(username));",
        &[],
        ).await;

        let s = Self {
            receiver,
            txs,
            connected_users,
            salt_generator: ChaCha20Rng::from_entropy(),
            db_client,
            priv_key,
            pub_key,
        };
        // Launch channel loop
        tokio::spawn(s.channel_loop());
    }

    async fn channel_loop(mut self) {
        loop {
            use ChannelCommand::*;
            let p = self.receiver.recv().await.unwrap();
            match p {
                Write(p) => {
                    log::info!("Message: {:?}", &p);
                    if let ClientboundPacket::Message(message) = &p {
                        self.insert_message(message).await;
                    }
                    for (addr, tx_) in &self.txs {
                        // Only send to logged in users
                        // Maybe there is a prettier way to achieve that? Seems suboptimal
                        if self.connected_users.contains_key(addr) {
                            tx_.send(ConnectionCommand::Write(p.clone())).await.ok();
                        }
                    }
                }
                EncryptionRequest(tx, otx) => {
                    let mut token = [0u8; ENC_TOK_LEN];
                    OsRng.fill(&mut token);
                    tx.send(ConnectionCommand::Write(
                        ClientboundPacket::EncryptionResponse(
                            self.pub_key.to_public_key_der().unwrap().as_ref().to_vec(),
                            token.to_vec(),
                        ),
                    ))
                    .await
                    .unwrap();
                    otx.send(token.to_vec()).unwrap();
                }
                EncryptionConfirm(tx, otx, enc_s, enc_t, exp_t) => {
                    let t = {
                        let padding = PaddingScheme::new_pkcs1v15_encrypt();
                        self.priv_key
                            .decrypt(padding, &enc_t)
                            .expect("failed to decrypt")
                    };
                    if t != exp_t {
                        log::error!("Encryption handshake failed!");
                        tx.send(ConnectionCommand::Close).await.ok();
                        otx.send(Err(())).unwrap();
                    } else {
                        let s = {
                            let padding = PaddingScheme::new_pkcs1v15_encrypt();
                            self.priv_key
                                .decrypt(padding, &enc_s)
                                .expect("failed to decrypt")
                        };
                        otx.send(Ok(s.clone())).unwrap();
                        tx.send(ConnectionCommand::SetSecret(Some(s.clone())))
                            .await
                            .unwrap();
                        tx.send(ConnectionCommand::Write(ClientboundPacket::EncryptionAck))
                            .await
                            .unwrap();
                    }
                }
                LoginAttempt { .. } => {
                    self.handle_login(p).await;
                }
                UserJoined(username) => {
                    for tx_ in self.txs.values() {
                        tx_.send(ConnectionCommand::Write(ClientboundPacket::UserJoined(
                            username.clone(),
                        )))
                        .await
                        .ok();
                    }
                }
                UserLeft(addr) => {
                    log::info!("Connection ended from: {}", addr);
                    self.txs.remove(&addr);
                    if let Some(username) = self.connected_users.remove(&addr) {
                        for tx_ in self.txs.values() {
                            tx_.send(ConnectionCommand::Write(ClientboundPacket::UserLeft(
                                username.clone(),
                            )))
                            .await
                            .ok();
                        }
                    }
                }
                UsersQuery(addr) => {
                    let tx = self
                        .txs
                        .get(&addr)
                        .unwrap_or_else(|| panic!("Wrong reply addr: {}", addr));
                    tx.send(ConnectionCommand::Write(ClientboundPacket::UsersOnline(
                        self.connected_users.values().cloned().collect(),
                    )))
                    .await
                    .unwrap();
                }
                FetchMessages(o, n, otx) => {
                    let n = n.min(64); // Clamp so we don't query and send too much
                    let messages_rows = self.fetch_messages(o, n).await;
                    let messages = messages_rows
                        .iter()
                        .map(|r| accord::packets::Message {
                            text: r.get("content"),
                            sender: r.get("sender"),
                            time: r.get::<_, i64>("send_time") as u64,
                        })
                        .collect();
                    otx.send(messages).unwrap();
                }
            }
        }
    }

    async fn handle_login(&mut self, p: ChannelCommand) {
        if let ChannelCommand::LoginAttempt {
            username,
            password,
            addr,
            otx,
            tx,
        } = p
        {
            let res = if !verify_username(&username) {
                Err("Invalid username!".to_string())
            } else if let Some(row) = self.get_user(&username).await {
                let salt_s: String = row.get("salt");
                let salt = base64::decode(salt_s).unwrap();
                let pass_hash = hash_password(password, salt);
                let acc_pass_s: String = row.get("password");
                let acc_pass = base64::decode(acc_pass_s).unwrap();
                if pass_hash == acc_pass.as_slice() {
                    if self.connected_users.values().any(|u| u == &username) {
                        Err("Already logged in.".to_string())
                    } else {
                        log::info!("Logged in: {}", username);
                        Ok(username.clone())
                    }
                } else {
                    Err("Incorrect password".to_string())
                }
            } else {
                let mut salt = [0; 64];
                self.salt_generator.fill_bytes(&mut salt);
                let pass_hash = hash_password(password, salt);
                self.insert_user(&username, &pass_hash, &salt).await;
                log::info!("New account: {}", username);
                Ok(username.clone())
            };
            if res.is_ok() {
                self.connected_users.insert(addr, username);
                self.txs.insert(addr, tx);
            } else {
                log::info!("Failed to log in: {}", username);
            }
            otx.send(res).unwrap();
        } else {
            panic!("Provided not login packet to handle_login")
        }
    }

    async fn insert_user(&self, username: &str, pass_hash: &[u8], salt: &[u8]) {
        self.db_client
            .execute(
                "INSERT INTO accounts VALUES ($1, $2, $3)",
                &[&username, &base64::encode(pass_hash), &base64::encode(salt)],
            )
            .await
            .unwrap();
    }
    async fn get_user(&self, username: &str) -> Option<tokio_postgres::Row> {
        self.db_client
            .query_opt("SELECT * FROM accounts WHERE username=$1", &[&username])
            .await
            .unwrap()
    }

    async fn insert_message(&self, message: &accord::packets::Message) {
        self.db_client
            .execute(
                "INSERT INTO messages VALUES ($1, $2, $3)",
                &[&message.sender, &message.text, &(message.time as i64)],
            )
            .await
            .unwrap();
    }

    async fn fetch_messages(&self, offset: i64, count: i64) -> Vec<tokio_postgres::Row> {
        self.db_client
            .query(
                "SELECT * FROM messages ORDER BY send_time DESC OFFSET $1 ROWS FETCH FIRST $2 ROW ONLY;",
                &[&offset, &count],
            )
            .await
            .unwrap()
    }
}

#[inline]
fn hash_password<P: AsRef<[u8]>, S: AsRef<[u8]>>(pass: P, salt: S) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(pass);
    hasher.update(salt);
    let mut ret = [0; 32];
    ret.copy_from_slice(&hasher.finalize()[..32]);
    ret
}
