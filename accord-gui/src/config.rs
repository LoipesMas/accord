use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Represents config file loaded into memory
#[derive(Serialize, Deserialize)]
pub struct Config {
    pub address: String,
    pub username: String,
    pub remember_login: bool,
    pub images_from_links: bool,
    pub theme: Option<crate::Theme>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: Default::default(),
            username: Default::default(),
            remember_login: true,
            images_from_links: false,
            theme: Some(Default::default()),
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
    let local_app_data = std::env::var("LOCALAPPDATA").unwrap();
    let mut path = PathBuf::from(local_app_data);
    path.push("accord-gui");
    path
}

/// Saves config.
/// If [`Config::theme`] is `None`, it loads it from saved config.
pub fn save_config(mut config: Config) -> std::io::Result<()> {
    log::info!("Saving config.");
    let config_path = config_path();
    std::fs::create_dir_all(config_path_dir()).unwrap();

    if config.theme.is_none() {
        // This _shouldn't_ create an infinite loop, because if `load_config` doesn't load a theme,
        // it uses default
        config.theme = load_config().theme;
    }

    let toml = toml::to_string(&config).unwrap();
    std::fs::write(config_path, &toml)
}

pub fn load_config() -> Config {
    log::info!("Loading config.");
    let config_path = config_path();
    let toml = std::fs::read_to_string(config_path);
    let mut config = if let Ok(toml) = toml {
        match toml::from_str(&toml) {
            Ok(config) => config,
            Err(e) => {
                log::error!("Failed to parse config: {e}.");
                std::process::exit(-1)
            }
        }
    } else {
        log::info!("Failed to load config, using default and saving default.");
        save_config(Config::default()).unwrap();
        Config::default()
    };
    if config.theme.is_none() {
        log::warn!("No `theme` field in config! Using default.");
        config.theme = Some(Default::default());
    }
    config
}
