<img src="https://user-images.githubusercontent.com/46327403/165834531-901c95d3-a932-4059-90f9-b1bcfa7178ad.svg" height="100">

---


**Instant messaging chat system over TCP.**  
Written in Rust with tokio-rs.
Packet design and handshake inspired partially by Minecraft.

<img src="https://user-images.githubusercontent.com/46327403/165838116-f0b38ddf-f1f8-4c59-9474-580397daf443.png" width="100%">


## Features
- Standalone server
- GUI client (using `druid` UI toolkit) with customizations via config file
- TUI client
- Encryption
- Sending images (via clipboard)
- Server management (banning, whitelists, etc)


## GUI requirements
Because accord's gui client uses `druid`, it requires gtk on Linux and BSD.  
See [druid's Readme notes](https://github.com/linebender/druid#platform-notes) for more information.


## Short-term goals
- Improve GUI experience (sidebar with active users, loading up past messages and more)
- Verify that the encryption is secure
- Add more features

## Long-term goals
- Figure out long-term goals

## The Stack
- Server:
  - tokio-rs
  - postgres
- GUI:
  - tokio-rs
  - druid

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
5. Edit the config (probably located in `~/.config/accord-server/config.toml`) with correct postgres credentials.
6. Launch `accord-server` again, this time it should connect.
7. Done!  
  Now clients can connect.

## Contributing
Contributions are very welcome! Features, ideas, bug fixes, anything.
