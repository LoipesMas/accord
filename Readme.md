# Accord
Instant messaging program (server and bare-bones tui client).
Written in Rust with tokio-rs.
Packet design inspired partially by Minecraft.

## Setting up accord server
1. Obtain `accord-server` binary.  
Currently only method is to compile it yourself.
```
git clone https://github.com/LoipesMas/accord.git
cd accord
cargo b -p accord-server --release
```
2. Set up postgresql database somewhere.  
  Refer to postgres instructions for how to do that.
4. Launch `accord-server`. It will error something about connecting to the database, but we just need the default config.
5. Edit the config (probably located in `.config/accord-server/config.toml`) with correct postgres credentials.
6. Launch `accord-server` again, this time it should connect.
7. Done!  
  Now clients can connect.
