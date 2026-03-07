use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;
use vesa_event::Position;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VesaConfig {
    #[serde(default)]
    pub server: Option<ServerConfig>,
    #[serde(default)]
    pub client: Option<ClientConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: SocketAddr,
    #[serde(default)]
    pub clients: Vec<ClientEntry>,
    #[serde(default = "default_hotkey")]
    pub release_hotkey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientEntry {
    pub name: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_addr: SocketAddr,
}

fn default_bind_addr() -> SocketAddr {
    "0.0.0.0:4920".parse().unwrap()
}

fn default_hotkey() -> String {
    "Escape".to_string()
}

impl VesaConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: VesaConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or(VesaConfig {
            server: None,
            client: None,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_server_config() {
        let toml = r#"
[server]
bind_addr = "192.168.1.10:4920"
release_hotkey = "F12"

[[server.clients]]
name = "laptop"
position = "Right"
"#;
        let config: VesaConfig = toml::from_str(toml).unwrap();
        let server = config.server.unwrap();
        assert_eq!(server.bind_addr.port(), 4920);
        assert_eq!(server.release_hotkey, "F12");
        assert_eq!(server.clients.len(), 1);
        assert_eq!(server.clients[0].name, "laptop");
        assert_eq!(server.clients[0].position, Position::Right);
    }

    #[test]
    fn parse_client_config() {
        let toml = r#"
[client]
server_addr = "10.0.0.1:4920"
"#;
        let config: VesaConfig = toml::from_str(toml).unwrap();
        let client = config.client.unwrap();
        assert_eq!(client.server_addr.to_string(), "10.0.0.1:4920");
    }

    #[test]
    fn defaults_applied() {
        let toml = "[server]\n";
        let config: VesaConfig = toml::from_str(toml).unwrap();
        let server = config.server.unwrap();
        assert_eq!(server.bind_addr.to_string(), "0.0.0.0:4920");
        assert_eq!(server.release_hotkey, "Escape");
        assert!(server.clients.is_empty());
    }

    #[test]
    fn load_from_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "[client]\nserver_addr = \"127.0.0.1:4920\"\n").unwrap();
        let config = VesaConfig::load(tmp.path()).unwrap();
        assert!(config.client.is_some());
    }

    #[test]
    fn load_or_default_missing_file() {
        let config = VesaConfig::load_or_default(Path::new("/nonexistent/path/config.toml"));
        assert!(config.server.is_none());
        assert!(config.client.is_none());
    }

    #[test]
    fn parse_all_positions() {
        for (s, expected) in [
            ("Left", Position::Left),
            ("Right", Position::Right),
            ("Top", Position::Top),
            ("Bottom", Position::Bottom),
        ] {
            let toml =
                format!("[server]\n[[server.clients]]\nname = \"test\"\nposition = \"{s}\"\n");
            let config: VesaConfig = toml::from_str(&toml).unwrap();
            assert_eq!(config.server.unwrap().clients[0].position, expected);
        }
    }

    #[test]
    fn invalid_toml_returns_error() {
        let result: Result<VesaConfig, _> = toml::from_str("not valid toml {{{}}}");
        assert!(result.is_err());
    }

    #[test]
    fn multiple_clients() {
        let toml = r#"
[server]
[[server.clients]]
name = "pc1"
position = "Left"
[[server.clients]]
name = "pc2"
position = "Right"
[[server.clients]]
name = "pc3"
position = "Top"
"#;
        let config: VesaConfig = toml::from_str(toml).unwrap();
        let server = config.server.unwrap();
        assert_eq!(server.clients.len(), 3);
    }
}
