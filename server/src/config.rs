use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub db_host: String,
    pub db_port: String,
    pub db_user: String,
    pub db_pass: String,
    pub db_dbname: String,
    pub port: Option<u16>,
    pub operators: HashSet<String>,
    pub whitelist_on: bool,
    pub whitelist: HashSet<String>,
    pub banned_users: HashSet<String>,
    pub allow_new_accounts: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            db_host: Default::default(),
            db_port: Default::default(),
            db_user: Default::default(),
            db_pass: Default::default(),
            db_dbname: Default::default(),
            port: Some(accord::DEFAULT_PORT),
            operators: Default::default(),
            whitelist_on: false,
            whitelist: Default::default(),
            banned_users: Default::default(),
            allow_new_accounts: true,
        }
    }
}

const CONFIG_FILE: &str = "config.toml";

fn config_path() -> PathBuf {
    let mut path = config_path_dir();
    path.push(CONFIG_FILE);
    log::info!("config path: {:?}", path);
    path
}

#[cfg(unix)]
fn config_path_dir() -> PathBuf {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("accord-server").unwrap();
    xdg_dirs.get_config_home()
}

#[cfg(windows)]
fn config_path_dir() -> PathBuf {
    let local_app_data = std::env::var("LOCALAPPDATA").unwrap();
    let mut path = PathBuf::from(local_app_data);
    path.push("accord-server");
    path
}

pub fn save_config(config: &Config) -> std::io::Result<()> {
    log::info!("Saving config.");
    let config_path = config_path();
    std::fs::create_dir_all(config_path_dir()).unwrap();

    let toml = toml::to_string(config).unwrap();
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
        save_config(&Config::default()).unwrap();
        Config::default()
    };
    config
}
