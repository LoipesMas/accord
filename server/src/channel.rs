use accord::packets::*;
use accord::utils::verify_username;

use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};

use super::commands::*;

#[derive(Debug)]
pub struct AccordChannel {
    receiver: Receiver<ChannelCommands>,
    txs: HashMap<std::net::SocketAddr, Sender<ConnectionCommands>>,
    connected_users: HashMap<std::net::SocketAddr, String>,
    accounts: HashMap<String, [u8; 32]>,
}

impl AccordChannel {
    pub fn spawn(receiver: Receiver<ChannelCommands>) {
        // Setup
        let txs: HashMap<std::net::SocketAddr, Sender<ConnectionCommands>> = HashMap::new();
        let connected_users: HashMap<std::net::SocketAddr, String> = HashMap::new();
        let accounts: HashMap<String, [u8; 32]> = HashMap::new();
        let s = Self {
            receiver,
            txs,
            connected_users,
            accounts,
        };
        // Launch channel loop
        tokio::spawn(s.channel_loop());
    }

    async fn channel_loop(mut self) {
        loop {
            use ChannelCommands::*;
            let p = self.receiver.recv().await.unwrap();
            match p {
                NewConnection(tx, addr) => {
                    println!("Connection from: {:?}", addr);
                    self.txs.insert(addr, tx);
                }
                Write(p) => {
                    println!("Message: {:?}", &p);
                    for (addr, tx_) in &self.txs {
                        // Only send to logged in users
                        // Maybe there is a prettier way to achieve that? Seems suboptimal
                        if self.connected_users.contains_key(addr) {
                            tx_.send(ConnectionCommands::Write(p.clone())).await.ok();
                        }
                    }
                }
                LoginAttempt { .. } => {
                    self.handle_login(p).await;
                }
                UserJoined(username) => {
                    for tx_ in self.txs.values() {
                        tx_.send(ConnectionCommands::Write(ClientboundPacket::UserJoined(
                            username.clone(),
                        )))
                        .await
                        .ok();
                    }
                }
                UserLeft(addr) => {
                    println!("Connection ended from: {}", addr);
                    let username = self
                        .connected_users
                        .remove(&addr)
                        .unwrap_or_else(|| "".to_string());
                    self.txs.remove(&addr);
                    for tx_ in self.txs.values() {
                        tx_.send(ConnectionCommands::Write(ClientboundPacket::UserLeft(
                            username.clone(),
                        )))
                        .await
                        .ok();
                    }
                }
                UsersQuery(addr) => {
                    let tx = self
                        .txs
                        .get(&addr)
                        .unwrap_or_else(|| panic!("Wrong reply addr: {}", addr));
                    tx.send(ConnectionCommands::Write(ClientboundPacket::UsersOnline(
                        self.connected_users.values().cloned().collect(),
                    )))
                    .await
                    .unwrap();
                }
            }
        }
    }

    async fn handle_login(&mut self, p: ChannelCommands) {
        if let ChannelCommands::LoginAttempt {
            username,
            password,
            addr,
            otx,
        } = p
        {
            let pass_hash = hash_password(password);
            let res;
            if !verify_username(&username) {
                res = LoginOneshotCommand::Failed("Invalid username!".to_string());
            } else if let Some(pass_hash_existing) = self.accounts.get(&username) {
                if &pass_hash == pass_hash_existing {
                    if self.connected_users.values().any(|u| u == &username) {
                        res = LoginOneshotCommand::Failed("Already logged in.".to_string());
                    } else {
                        println!("Logged in: {}", username);
                        res = LoginOneshotCommand::Success(username.clone());
                    }
                } else {
                    res = LoginOneshotCommand::Failed("Incorrect password".to_string());
                }
            } else {
                self.accounts.insert(username.clone(), pass_hash);
                println!("New account: {}", username);
                res = LoginOneshotCommand::Success(username.clone());
            }
            if let LoginOneshotCommand::Success(_) = res {
                self.connected_users.insert(addr, username);
            } else {
                println!("Logged in: {}", username);
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
