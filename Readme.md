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


## GUI
### Requirements
Because accord's gui client uses `druid`, it requires gtk on Linux and BSD.  
See [druid's Readme notes](https://github.com/linebender/druid#platform-notes) for more information.

### Configuration
GUI's theme (and some saved data) can be edited in `config.toml` file.
- On Unix system it's in `$XDG_CONFIG_HOME/accord-gui/config.toml`
- On Windows system it's in `$LOCALAPPDATA/accord-gui/config.toml`  

Colors are in hexadecimal format (`#rrggbb`, `#rrggbbaa`, `#rbg` or `#rbga`).

### Images from links
GUI client can automatically try to load an image from a message with a link, however this is a potential security risk (e.g. IP grabbing), so it's disabled by default.  
(If you're using a VPN or a proxy, then the risk should be nonexistent and in worst-case scenario it's still less risky than clicking on a random link.)

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

### Using docker container
1. Clone docker compose repo
```
git clone https://github.com/LoipesMas/accord-docker.git
cd accord-docker
```
2. Edit `config.toml` (you probably only want to change operators)
3. `docker compose up -d` to run the server in the background

### From source
1. Compile accord-server
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
