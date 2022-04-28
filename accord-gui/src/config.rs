use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
            theme: Some(crate::Theme::default()),
            images_from_links: false,
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

pub fn save_config(mut config: Config) -> std::io::Result<()> {
    log::info!("Saving config.");
    let config_path = config_path();
    std::fs::create_dir_all(config_path_dir()).unwrap();

    if config.theme.is_none() {
        // This _shouldn't_ create an infinite loop, because theme shouldn't be None when saving
        // new config
        config.theme = load_config().theme;
    }

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
