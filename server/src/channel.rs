use accord::packets::*;
use accord::utils::verify_username;
use accord::{ENC_TOK_LEN, RSA_BITS};

use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};

use super::commands::*;

use rand::rngs::OsRng;
use rand::Rng;
use rsa::{pkcs8::ToPublicKey, PaddingScheme, RsaPrivateKey, RsaPublicKey};

#[derive(Debug)]
pub struct AccordChannel {
    receiver: Receiver<ChannelCommand>,
    txs: HashMap<std::net::SocketAddr, Sender<ConnectionCommand>>,
    connected_users: HashMap<std::net::SocketAddr, String>,
    accounts: HashMap<String, [u8; 32]>,
    priv_key: RsaPrivateKey,
    pub_key: RsaPublicKey,
}

impl AccordChannel {
    pub fn spawn(receiver: Receiver<ChannelCommand>) {
        // Setup
        let txs: HashMap<std::net::SocketAddr, Sender<ConnectionCommand>> = HashMap::new();
        let connected_users: HashMap<std::net::SocketAddr, String> = HashMap::new();
        let accounts: HashMap<String, [u8; 32]> = HashMap::new();
        let mut rng = OsRng;
        let priv_key = RsaPrivateKey::new(&mut rng, RSA_BITS).expect("failed to generate a key");
        let pub_key = RsaPublicKey::from(&priv_key);
        let s = Self {
            receiver,
            txs,
            connected_users,
            accounts,
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
                    println!("Message: {:?}", &p);
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
                        println!("Encryption handshake failed!");
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
                    println!("Connection ended from: {}", addr);
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
            let pass_hash = hash_password(password);
            let res = if !verify_username(&username) {
                Err("Invalid username!".to_string())
            } else if let Some(pass_hash_existing) = self.accounts.get(&username) {
                if &pass_hash == pass_hash_existing {
                    if self.connected_users.values().any(|u| u == &username) {
                        Err("Already logged in.".to_string())
                    } else {
                        println!("Logged in: {}", username);
                        Ok(username.clone())
                    }
                } else {
                    Err("Incorrect password".to_string())
                }
            } else {
                self.accounts.insert(username.clone(), pass_hash);
                println!("New account: {}", username);
                Ok(username.clone())
            };
            if res.is_ok() {
                self.connected_users.insert(addr, username);
                self.txs.insert(addr, tx);
            } else {
                println!("Failed to log in: {}", username);
            }
            otx.send(res).unwrap();
        } else {
            panic!("Provided not login packet to handle_login")
        }
    }
}

#[inline]
fn hash_password<T: AsRef<[u8]>>(pass: T) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(pass);
    let mut ret = [0; 32];
    ret.copy_from_slice(&hasher.finalize()[..32]);
    ret
}
