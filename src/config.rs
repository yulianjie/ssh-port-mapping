use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardKind {
    #[default]
    Remote,
    Local,
}

impl ForwardKind {
    pub const ALL: [Self; 2] = [Self::Remote, Self::Local];

    pub fn ssh_flag(self) -> &'static str {
        match self {
            Self::Remote => "-R",
            Self::Local => "-L",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Remote => "远程转发 (-R)",
            Self::Local => "本地转发 (-L)",
        }
    }
}

impl fmt::Display for ForwardKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub user: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    #[serde(default)]
    pub kind: ForwardKind,
    #[serde(default = "default_loopback")]
    pub bind_address: String,
    pub bind_port: u16,
    #[serde(default = "default_loopback")]
    pub target_host: String,
    pub target_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_jump: Option<String>,
    #[serde(default)]
    pub autostart: bool,
}

impl TunnelConfig {
    pub fn destination(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.ssh_port)
    }

    pub fn mapping(&self) -> String {
        format!(
            "{}:{} → {}:{}",
            self.bind_address, self.bind_port, self.target_host, self.target_port
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("请输入名称".into());
        }
        if self.host.trim().is_empty() {
            return Err("请输入服务器地址".into());
        }
        if self.user.trim().is_empty() {
            return Err("请输入 SSH 用户名".into());
        }
        validate_destination_part("服务器地址", &self.host, false)?;
        validate_destination_part("SSH 用户名", &self.user, true)?;
        if self.bind_address.trim().is_empty() {
            return Err("请输入监听地址".into());
        }
        if self.target_host.trim().is_empty() {
            return Err("请输入目标地址".into());
        }
        if self.ssh_port == 0 || self.bind_port == 0 || self.target_port == 0 {
            return Err("端口必须介于 1 到 65535 之间".into());
        }
        if let Some(proxy_jump) = self.proxy_jump.as_deref() {
            validate_proxy_jump(proxy_jump)?;
        }
        Ok(())
    }
}

fn validate_destination_part(label: &str, value: &str, reject_at: bool) -> Result<(), String> {
    if value.starts_with('-')
        || value.chars().any(char::is_whitespace)
        || (reject_at && value.contains('@'))
    {
        return Err(format!("{label}包含不支持的字符"));
    }
    Ok(())
}

fn validate_proxy_jump(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.chars().any(char::is_whitespace)
        || value
            .split(',')
            .any(|hop| hop.is_empty() || hop.starts_with('-'))
    {
        return Err("跳板机必须使用英文逗号分隔，且不能包含空格".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "config_version")]
    pub version: u32,
    #[serde(default)]
    pub tunnels: Vec<TunnelConfig>,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default)]
    pub start_minimized: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            tunnels: Vec::new(),
            minimize_to_tray: true,
            start_minimized: false,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<(Self, PathBuf), ConfigError> {
        let path = config_path()?;
        if !path.exists() {
            return Ok((Self::default(), path));
        }
        let bytes = fs::read(&path).map_err(ConfigError::Io)?;
        let config = serde_json::from_slice(&bytes).map_err(ConfigError::Json)?;
        Ok((config, path))
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(ConfigError::Json)?;
        fs::write(path, data).map_err(ConfigError::Io)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    NoConfigDirectory,
    Io(io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoConfigDirectory => formatter.write_str("无法确定配置目录"),
            Self::Io(error) => write!(formatter, "配置文件 I/O 错误：{error}"),
            Self::Json(error) => write!(formatter, "配置文件格式无效：{error}"),
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    ProjectDirs::from("dev", "PortWeave", "PortWeave")
        .map(|dirs| dirs.config_dir().join("config.json"))
        .ok_or(ConfigError::NoConfigDirectory)
}

const fn config_version() -> u32 {
    CONFIG_VERSION
}

const fn default_ssh_port() -> u16 {
    22
}

fn default_loopback() -> String {
    "127.0.0.1".into()
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TunnelConfig {
        TunnelConfig {
            id: Uuid::nil(),
            name: "example-remote".into(),
            host: "203.0.113.10".into(),
            user: "developer".into(),
            ssh_port: 2222,
            kind: ForwardKind::Remote,
            bind_address: "127.0.0.1".into(),
            bind_port: 7897,
            target_host: "127.0.0.1".into(),
            target_port: 7897,
            identity_file: None,
            proxy_jump: None,
            autostart: false,
        }
    }

    #[test]
    fn serializes_and_loads_config() {
        let config = AppConfig {
            tunnels: vec![sample()],
            ..AppConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.tunnels, config.tunnels);
    }

    #[test]
    fn validates_required_fields() {
        let mut tunnel = sample();
        assert!(tunnel.validate().is_ok());
        tunnel.host.clear();
        assert_eq!(tunnel.validate().unwrap_err(), "请输入服务器地址");
    }

    #[test]
    fn rejects_destination_values_that_could_be_parsed_as_options() {
        let mut tunnel = sample();
        tunnel.host = "-oProxyCommand=bad".into();
        assert_eq!(tunnel.validate().unwrap_err(), "服务器地址包含不支持的字符");

        tunnel.host = "example.com".into();
        tunnel.user = "name@example".into();
        assert_eq!(tunnel.validate().unwrap_err(), "SSH 用户名包含不支持的字符");
    }

    #[test]
    fn validates_proxy_jump_chains() {
        let mut tunnel = sample();
        tunnel.proxy_jump = Some("bastion,ops@edge.example:2222".into());
        assert!(tunnel.validate().is_ok());

        tunnel.proxy_jump = Some("bastion, -oBad=yes".into());
        assert_eq!(
            tunnel.validate().unwrap_err(),
            "跳板机必须使用英文逗号分隔，且不能包含空格"
        );
    }
}
