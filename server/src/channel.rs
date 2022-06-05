use accord::packets::*;
use accord::utils::verify_username;
use accord::{ENC_TOK_LEN, RSA_BITS};

use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};

use tokio_postgres::{Client as DBClient, NoTls};

use crate::config::{save_config, Config};

use super::commands::*;

use rand::rngs::OsRng;
use rand::Rng;
use rand::RngCore;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use rsa::{pkcs8::ToPublicKey, PaddingScheme, RsaPrivateKey, RsaPublicKey};

use anyhow::{Context, Result};

pub struct AccordChannel {
    receiver: Receiver<ChannelCommand>,
    txs: HashMap<std::net::SocketAddr, Sender<ConnectionCommand>>,
    connected_users: HashMap<std::net::SocketAddr, String>,
    salt_generator: ChaCha20Rng,
    db_client: DBClient,
    priv_key: RsaPrivateKey,
    pub_key: RsaPublicKey,
    config: Config,
}

impl AccordChannel {
    pub async fn spawn(receiver: Receiver<ChannelCommand>, config: Config) -> Result<()> {
        // Setup
        let txs: HashMap<std::net::SocketAddr, Sender<ConnectionCommand>> = HashMap::new();
        let connected_users: HashMap<std::net::SocketAddr, String> = HashMap::new();
        let mut rng = OsRng;
        let priv_key =
            RsaPrivateKey::new(&mut rng, RSA_BITS).with_context(|| "Failed to generate a key.")?;
        let pub_key = RsaPublicKey::from(&priv_key);

        let database_config = format!(
            "host='{}' port='{}' user='{}' password='{}' dbname='{}'",
            config.db_host, config.db_port, config.db_user, config.db_pass, config.db_dbname,
        );

        let (db_client, db_connection) = tokio_postgres::connect(&database_config, NoTls)
            .await
            .with_context(|| format!("Postgres connection ({}) error.", database_config))?;

        tokio::spawn(async move {
            if let Err(e) = db_connection.await {
                log::error!("Database connection error: {}.", e);
            };
        });

        // Prepare Database, panic if it fails and gives us the reason. Without this, the server will be useless anyway, so it is ok to panic here.
        // Friendly reminder @LoipesMas never silence errors, otherwise debugging will be a pain.
        log::info!("Preparing database...");

        // Create accord schema if not exists, handle errors
        let _ = db_client
            .execute("CREATE SCHEMA IF NOT EXISTS accord", &[])
            .await
            .with_context(|| "Failed to create schema 'accord'.")?;

        // Create account table if not exists
        let _ = db_client
            .execute(
                "CREATE TABLE IF NOT EXISTS accord.accounts (
                    user_id serial8 NOT null PRIMARY KEY, 
                    username varchar(255) NOT NULL UNIQUE, 
                    password varchar(44) NOT NULL, 
                    salt varchar(88) NOT NULL,
                    banned bool NOT NULL DEFAULT false,
                    whitelisted bool NOT NULL DEFAULT false
                    );",
                &[],
            )
            .await
            .with_context(|| "Failed to create table 'accounts'.")?;

        // Create images table if not exists
        let _ = db_client
            .execute(
                "CREATE TABLE IF NOT EXISTS accord.images ( image_hash INT PRIMARY KEY, data BYTEA NOT NULL);",
                &[],
            )
            .await
            .with_context(|| "Failed to create table 'images'.")?;

        // Create messages table if not exists
        let _ = db_client
            .execute(
        "CREATE TABLE IF NOT EXISTS accord.messages ( 
                        sender_id int8 NOT NULL, sender varchar(255) NOT NULL DEFAULT '*deleted_user*', content varchar(1023), send_time bigint NOT NULL, image_hash INT DEFAULT NULL, 
                        CONSTRAINT fk_image_hash FOREIGN KEY(image_hash) REFERENCES accord.images(image_hash) ON DELETE SET DEFAULT ON UPDATE CASCADE, 
                        CONSTRAINT fk_username FOREIGN KEY(sender) REFERENCES accord.accounts(username) ON DELETE SET DEFAULT ON UPDATE CASCADE
                    );",
        &[],
        ).await
        .with_context(|| "Failed to create table 'messages'.")?;

        log::info!("DONE: Preparing database.");

        let s = Self {
            receiver,
            txs,
            connected_users,
            salt_generator: ChaCha20Rng::from_entropy(),
            db_client,
            priv_key,
            pub_key,
            config,
        };
        // Launch channel loop
        tokio::spawn(s.channel_loop());
        Ok(())
    }

    async fn channel_loop(mut self) {
        loop {
            use ChannelCommand::*;
            let p = match self.receiver.recv().await {
                Some(p) => p,
                None => break,
            };
            match p {
                Close => {
                    break;
                }
                Write(p) => {
                    match p {
                        ClientboundPacket::ImageMessage(ref im) => {
                            log::info!("Image from {}.", im.sender);
                        }
                        _ => log::info!("Message: {:?}.", &p),
                    }
                    match &p {
                        ClientboundPacket::Message(message) => {
                            self.insert_message(message).await;
                        }
                        ClientboundPacket::ImageMessage(im) => {
                            self.insert_image_message(im).await;
                        }
                        _ => (),
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
                            .expect("Failed to decrypt.")
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
                                .expect("Failed to decrypt.")
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
                    self.txs.remove(&addr);
                    if let Some(username) = self.connected_users.remove(&addr) {
                        log::info!("Connection ended from: {} ({}).", username, addr);
                        for tx_ in self.txs.values() {
                            tx_.send(ConnectionCommand::Write(ClientboundPacket::UserLeft(
                                username.clone(),
                            )))
                            .await
                            .ok();
                        }
                    } else {
                        log::info!("Connection ended from: {}", addr);
                    }
                }
                UsersQueryTUI(otx) => {
                    if otx
                        .send(self.connected_users.values().cloned().collect())
                        .is_err()
                    {
                        log::error!("Error while getting user list in TUI");
                    }
                }
                UsersQuery(addr) => {
                    let tx = self
                        .txs
                        .get(&addr)
                        .unwrap_or_else(|| panic!("Wrong reply addr: {}.", addr));
                    tx.send(ConnectionCommand::Write(ClientboundPacket::UsersOnline(
                        self.connected_users.values().cloned().collect(),
                    )))
                    .await
                    .unwrap();
                }
                FetchMessages(o, n, otx) => {
                    let n = n.min(64); // Clamp so we don't query and send too much
                    let messages_rows = self.fetch_messages(o, n).await;
                    let messages = messages_rows.iter().map(|r| async {
                        if let Some(hash) = r.get::<_, Option<i32>>("image_hash") {
                            let image_bytes = self.fetch_image(hash).await;
                            ClientboundPacket::ImageMessage(accord::packets::ImageMessage {
                                sender_id: r.get("sender_id"),
                                sender: r.get("sender"),
                                image_bytes,
                                time: r.get::<_, i64>("send_time") as u64,
                            })
                        } else {
                            ClientboundPacket::Message(accord::packets::Message {
                                sender_id: r.get("sender_id"),
                                sender: r.get("sender"),
                                text: r.get("content"),
                                time: r.get::<_, i64>("send_time") as u64,
                            })
                        }
                    });
                    let messages = futures::future::join_all(messages).await;
                    otx.send(messages).unwrap();
                }
                CheckPermissions(username, otx) => {
                    let perms = self.get_user_perms(&username).await;
                    otx.send(perms).unwrap();
                }
                KickUser(username) => {
                    self.kick_user(&username).await;
                }
                BanUser(username, switch) => {
                    if switch {
                        self.kick_user(&username).await;
                    }
                    self.ban_user(&username, switch).await;
                }
                WhitelistUser(username, switch) => {
                    self.whitelist_user(&username, switch).await;
                }
                SetWhitelist(state) => {
                    self.config.whitelist_on = state;
                    log::info!("Set whitelist: {}", state);
                    save_config(&self.config).unwrap();
                }
                SetAllowNewAccounts(state) => {
                    self.config.allow_new_accounts = state;
                    log::info!("Set allow_new_accounts: {}", state);
                    save_config(&self.config).unwrap();
                }
            };
        }
    }

    /// Disconnects user from the channel
    async fn kick_user(&mut self, username: &str) {
        log::info!("Kicked user {}", username);
        for (addr, un) in self.connected_users.iter() {
            if un == username {
                self.txs
                    .get(addr)
                    .unwrap()
                    .send(ConnectionCommand::Close)
                    .await
                    .unwrap();
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
            let perms = self.get_user_perms(&username).await;
            let res = if !verify_username(&username) {
                Err("Invalid username!".to_string())
            } else if perms.banned {
                Err("User banned.".to_string())
            } else if self.config.whitelist_on && !perms.whitelisted {
                Err("User not on whitelist.".to_string())
            } else if let Some(row) = self.get_user(&username).await {
                // Account exists
                let salt_s: String = row.get("salt");
                let salt = base64::decode(salt_s).unwrap();
                let pass_hash = hash_password(password, salt);
                let acc_pass_s: String = row.get("password");
                let acc_pass = base64::decode(acc_pass_s).unwrap();
                if pass_hash == acc_pass.as_slice() {
                    if self.connected_users.values().any(|u| u == &username) {
                        Err("Already logged in.".to_string())
                    } else {
                        let user_id: i64 = row.get("user_id");
                        let username: String = row.get("username");
                        log::info!(
                            "Logged in: {} (user_id: {}) from {}.",
                            username,
                            user_id,
                            addr
                        );
                        Ok(format!("{}|{}", user_id, username))
                    }
                } else {
                    Err("Incorrect password.".to_string())
                }
            } else {
                // New account
                if self.config.allow_new_accounts {
                    let mut salt = [0; 64];
                    self.salt_generator.fill_bytes(&mut salt);
                    let pass_hash = hash_password(password, salt);

                    if let Some(row) = self.insert_user(&username, &pass_hash, &salt).await {
                        log::info!("New account: {}.", username);
                        let user_id: i64 = row.get("user_id");
                        let username: String = row.get("username");

                        Ok(format!("{}|{}", user_id, username))
                    } else {
                        Err("Failed to create account.".to_string())
                    }
                } else {
                    Err("Account creation disabled.".to_string())
                }
            };
            if let Err(ref e) = res {
                log::info!("Failed to log in: {}, reason: {}", username, e);
            } else {
                self.connected_users.insert(addr, username);
                self.txs.insert(addr, tx);
            }
            otx.send(res).unwrap();
        } else {
            panic!("Provided not login packet to handle_login.")
        }
    }

    async fn insert_user(
        &self,
        username: &str,
        pass_hash: &[u8],
        salt: &[u8],
    ) -> Option<tokio_postgres::Row> {
        self.db_client
            .query_opt(
                "INSERT INTO accord.accounts(username, password, salt) VALUES ($1, $2, $3) RETURNING *",
                &[&username, &base64::encode(pass_hash), &base64::encode(salt)],
            )
            .await
            .unwrap()
    }
    async fn get_user(&self, username: &str) -> Option<tokio_postgres::Row> {
        self.db_client
            .query_opt(
                "SELECT user_id, username, password, salt FROM accord.accounts WHERE username=$1",
                &[&username],
            )
            .await
            .unwrap()
    }

    async fn insert_message(&self, message: &accord::packets::Message) {
        self.db_client
            .execute(
                "INSERT INTO accord.messages(sender_id, sender, content, send_time) VALUES ($1, $2, $3, $4)",
                &[&message.sender_id, &message.sender, &message.text, &(message.time as i64)],
            )
            .await
            .unwrap();
    }

    async fn insert_image_message(&self, message: &accord::packets::ImageMessage) {
        use sha2::{Digest, Sha256};
        use tokio_postgres::types::private::read_be_i32;

        // Get hash of the image as i32
        let mut hasher = Sha256::new();
        hasher.update(&message.image_bytes);
        let hash = read_be_i32(&mut &hasher.finalize()[..4]).unwrap();

        // Insert image into db
        self.db_client
            .execute(
                "INSERT INTO accord.images VALUES ($1, $2) ON CONFLICT DO NOTHING",
                &[&hash, &message.image_bytes],
            )
            .await
            .unwrap();

        // Inser message with hash as a foreign key
        self.db_client
            .execute(
                "INSERT INTO accord.messages (sender_id, sender, content, send_time, image_hash) VALUES ($1, $2, '', $3, $4)",
                &[&message.sender_id, &message.sender, &(message.time as i64), &hash],
            )
            .await
            .unwrap();
    }

    async fn fetch_messages(&self, offset: i64, count: i64) -> Vec<tokio_postgres::Row> {
        self.db_client
            .query(
                "SELECT sender_id, sender, content, send_time, image_hash FROM accord.messages ORDER BY send_time DESC OFFSET $1 ROWS FETCH FIRST $2 ROW ONLY;",
                &[&offset, &count],
            )
            .await
            .unwrap()
    }

    /// Given hash, fetch image bytes from db
    async fn fetch_image(&self, hash: i32) -> Vec<u8> {
        let r = self
            .db_client
            .query(
                "SELECT data FROM accord.images WHERE image_hash=$1",
                &[&hash],
            )
            .await
            .unwrap();
        r.get(0).unwrap().get::<_, Vec<u8>>("data")
    }

    /// Returns permissions of a user
    /// Default if user not in accounts
    async fn get_user_perms(&self, username: &str) -> UserPermissions {
        let r = self
            .db_client
            .query(
                "SELECT banned, whitelisted FROM accord.accounts WHERE username=$1",
                &[&username],
            )
            .await
            .unwrap();

        r.get(0)
            .map(|r| UserPermissions {
                operator: self.config.operators.contains(username),
                banned: r.get::<_, bool>("banned"),
                whitelisted: r.get::<_, bool>("whitelisted"),
            })
            .unwrap_or_default()
    }

    /// Bans (or unbans) a user
    async fn ban_user(&self, username: &str, switch: bool) {
        if switch {
            log::info!("Banned user {}", username);
        } else {
            log::info!("Unbanned user {}", username);
        }
        self.db_client
            .execute(
                "UPDATE accord.accounts SET banned = $1 WHERE username = $2",
                &[&switch, &username],
            )
            .await
            .unwrap();
    }

    /// Whitelists (or unwhitelists) a user
    async fn whitelist_user(&self, username: &str, switch: bool) {
        let n = self.db_client
            .execute(
                "UPDATE accord.accounts SET whitelisted = $1 WHERE username = $2",
                &[&switch, &username],
            )
            .await
            .unwrap();
        if n == 0 {
            log::warn!("User {} not in database!", &username);
        }
        else if switch {
            log::info!("Whitelisted user {}", username);
        } else {
            log::info!("Unwhitelisted user {}", username);
        }
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
