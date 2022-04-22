use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub address: String,
    pub username: String,
    pub remember_login: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: Default::default(),
            username: Default::default(),
            remember_login: true,
        }
    }
}

const CONFIG_FILE: &str = "config.toml";
fn config_path() -> PathBuf {
    let mut path = config_path_dir();
    path.push(CONFIG_FILE);
    path
}

#[cfg(unix)]
fn config_path_dir() -> PathBuf {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("accord-gui").unwrap();
    xdg_dirs.get_config_home()
}

#[cfg(windows)]
fn config_path_dir() -> PathBuf {
    todo!("%APPDATA%/accord-gui ?")
}

pub fn save_config(config: Config) -> std::io::Result<()> {
    log::info!("Saving config.");
    let config_path = config_path();
    std::fs::create_dir_all(config_path_dir()).unwrap();

    let toml = toml::to_string(&config).unwrap();
    std::fs::write(config_path, &toml)
}

pub fn load_config() -> Config {
    log::info!("Loading config.");
    let config_path = config_path();
    let toml = std::fs::read_to_string(config_path);
    let config = if let Ok(toml) = toml {
        toml::from_str(&toml).unwrap()
    } else {
        log::info!("Failed to load config, using default and saving default.");
        save_config(Config::default()).unwrap();
        Config::default()
    };
    config
}
