use directories::UserDirs;
use glob::glob;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConnection {
    pub alias: String,
    pub host_name: String,
    pub user: String,
    pub port: u16,
    pub proxy_jump: Option<String>,
}

impl SshConnection {
    pub fn destination(&self) -> String {
        format!("{}@{}:{}", self.user, self.host_name, self.port)
    }
}

#[derive(Debug, Clone)]
pub struct SshConfigImport {
    pub config_path: PathBuf,
    pub connections: Vec<SshConnection>,
    pub warnings: Vec<String>,
}

pub fn import_default_ssh_config() -> Result<SshConfigImport, String> {
    let user_dirs = UserDirs::new().ok_or_else(|| "无法确定当前用户的主目录".to_string())?;
    let home_dir = user_dirs.home_dir();
    let config_path = home_dir.join(".ssh").join("config");
    import_ssh_config(&config_path, home_dir)
}

fn import_ssh_config(config_path: &Path, home_dir: &Path) -> Result<SshConfigImport, String> {
    if !config_path.is_file() {
        return Err(format!("未找到 SSH 配置文件：{}", config_path.display()));
    }

    let aliases = discover_host_aliases(config_path, home_dir)?;
    if aliases.is_empty() {
        return Err(format!(
            "SSH 配置文件中没有可导入的具体 Host 别名：{}",
            config_path.display()
        ));
    }

    let mut connections = Vec::new();
    let mut warnings = Vec::new();
    for alias in aliases {
        match resolve_connection(config_path, &alias) {
            Ok(connection) => connections.push(connection),
            Err(error) => warnings.push(format!("{alias}: {error}")),
        }
    }

    if connections.is_empty() {
        let details = warnings
            .first()
            .map(|warning| format!(": {warning}"))
            .unwrap_or_default();
        return Err(format!("OpenSSH 无法解析任何 Host 别名{details}"));
    }

    Ok(SshConfigImport {
        config_path: config_path.to_owned(),
        connections,
        warnings,
    })
}

fn discover_host_aliases(config_path: &Path, home_dir: &Path) -> Result<Vec<String>, String> {
    let include_root = config_path
        .parent()
        .map(Path::to_owned)
        .unwrap_or_else(|| home_dir.to_owned());
    let mut visited = HashSet::new();
    let mut seen_aliases = HashSet::new();
    let mut aliases = Vec::new();
    discover_file(
        config_path,
        home_dir,
        &include_root,
        true,
        &mut visited,
        &mut seen_aliases,
        &mut aliases,
    )?;
    Ok(aliases)
}

#[allow(clippy::too_many_arguments)]
fn discover_file(
    path: &Path,
    home_dir: &Path,
    include_root: &Path,
    required: bool,
    visited: &mut HashSet<PathBuf>,
    seen_aliases: &mut HashSet<String>,
    aliases: &mut Vec<String>,
) -> Result<(), String> {
    let canonical = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if !required && error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("无法打开 {}：{error}", path.display())),
    };
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let contents = fs::read_to_string(&canonical)
        .map_err(|error| format!("无法读取 {}：{error}", canonical.display()))?;
    for line in contents.lines() {
        let Some((keyword, values)) = parse_directive(line) else {
            continue;
        };
        match keyword.as_str() {
            "host" => {
                for alias in values.into_iter().filter(|value| is_concrete_alias(value)) {
                    if seen_aliases.insert(alias.clone()) {
                        aliases.push(alias);
                    }
                }
            }
            "include" => {
                for pattern in values {
                    for included in expand_include(&pattern, home_dir, include_root)? {
                        discover_file(
                            &included,
                            home_dir,
                            include_root,
                            false,
                            visited,
                            seen_aliases,
                            aliases,
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_directive(line: &str) -> Option<(String, Vec<String>)> {
    let line = strip_comment(line).trim();
    if line.is_empty() {
        return None;
    }
    let key_end = line
        .char_indices()
        .find_map(|(index, character)| {
            (character.is_whitespace() || character == '=').then_some(index)
        })
        .unwrap_or(line.len());
    let keyword = line[..key_end].to_ascii_lowercase();
    let values = split_words(line[key_end..].trim_start_matches([' ', '\t', '=']));
    Some((keyword, values))
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        match (quote, character) {
            (Some(active), current) if active == current => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, '#') => return &line[..index],
            _ => {}
        }
    }
    line
}

fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        match (quote, character) {
            (Some(active), current_quote) if active == current_quote => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, current_character) if current_character.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            (_, '\\') => {
                if characters.peek().is_some_and(|next| {
                    next.is_whitespace() || matches!(next, '\'' | '"' | '#' | '\\')
                }) {
                    current.push(characters.next().expect("已确认下一个字符存在"));
                } else {
                    current.push(character);
                }
            }
            _ => current.push(character),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn is_concrete_alias(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with(['!', '-'])
        && !value.chars().any(|character| {
            character.is_whitespace() || matches!(character, '*' | '?' | '[' | ']')
        })
}

fn expand_include(
    pattern: &str,
    home_dir: &Path,
    include_root: &Path,
) -> Result<Vec<PathBuf>, String> {
    let home = home_dir.to_string_lossy();
    let expanded = pattern.replace("%d", &home);
    let expanded = if expanded == "~" {
        home_dir.to_owned()
    } else if let Some(relative) = expanded
        .strip_prefix("~/")
        .or_else(|| expanded.strip_prefix("~\\"))
    {
        home_dir.join(relative)
    } else {
        let path = PathBuf::from(expanded);
        if path.is_absolute() {
            path
        } else {
            include_root.join(path)
        }
    };

    let pattern = expanded.to_string_lossy();
    let mut matches = glob(&pattern)
        .map_err(|error| format!("SSH Include 模式无效 {pattern}：{error}"))?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    matches.sort();
    Ok(matches)
}

fn resolve_connection(config_path: &Path, alias: &str) -> Result<SshConnection, String> {
    let mut command = Command::new("ssh");
    command.arg("-G").arg("-F").arg(config_path).arg(alias);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command
        .output()
        .map_err(|error| format!("无法运行 OpenSSH：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        return Err(if message.is_empty() {
            format!("ssh -G 已退出，状态为 {}", output.status)
        } else {
            message.to_string()
        });
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|_| "OpenSSH 返回的连接数据不是有效的 UTF-8".to_string())?;
    parse_effective_config(alias, &output)
}

fn parse_effective_config(alias: &str, output: &str) -> Result<SshConnection, String> {
    let mut host_name = None;
    let mut user = None;
    let mut port = None;
    let mut proxy_jump = None;
    for line in output.lines() {
        let Some((keyword, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        match keyword.to_ascii_lowercase().as_str() {
            "hostname" if host_name.is_none() => host_name = Some(value.to_string()),
            "user" if user.is_none() => user = Some(value.to_string()),
            "port" if port.is_none() => port = value.parse::<u16>().ok(),
            "proxyjump" if proxy_jump.is_none() && !value.eq_ignore_ascii_case("none") => {
                proxy_jump = Some(value.to_string());
            }
            _ => {}
        }
    }

    Ok(SshConnection {
        alias: alias.to_string(),
        host_name: host_name.unwrap_or_else(|| alias.to_string()),
        user: user.ok_or_else(|| "OpenSSH 未返回用户名".to_string())?,
        port: port.ok_or_else(|| "OpenSSH 未返回有效端口".to_string())?,
        proxy_jump,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_effective_connection_and_proxy_jump() {
        let connection = parse_effective_config(
            "private-app",
            "host private-app\nhostname 10.20.0.15\nuser deploy\nport 2202\nproxyjump bastion,edge\n",
        )
        .unwrap();
        assert_eq!(connection.alias, "private-app");
        assert_eq!(connection.host_name, "10.20.0.15");
        assert_eq!(connection.user, "deploy");
        assert_eq!(connection.port, 2202);
        assert_eq!(connection.proxy_jump.as_deref(), Some("bastion,edge"));
    }

    #[test]
    fn discovers_concrete_aliases_from_includes() {
        let root = std::env::temp_dir().join(format!("portweave-{}", uuid::Uuid::new_v4()));
        let ssh_dir = root.join(".ssh");
        let includes = ssh_dir.join("config.d");
        fs::create_dir_all(&includes).unwrap();
        fs::write(
            ssh_dir.join("config"),
            "Host *\n  ServerAliveInterval 30\nInclude config.d/*.conf\nHost direct !blocked *.wild\n",
        )
        .unwrap();
        fs::write(
            includes.join("servers.conf"),
            "Host jump app\n  User deploy\n",
        )
        .unwrap();

        let aliases = discover_host_aliases(&ssh_dir.join("config"), &root).unwrap();
        assert_eq!(aliases, ["jump", "app", "direct"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parses_equals_quotes_and_comments() {
        assert_eq!(
            parse_directive("Host = \"app server\" api # comment"),
            Some(("host".into(), vec!["app server".into(), "api".into()]))
        );
        assert_eq!(
            parse_directive("Include C:\\Users\\jack\\.ssh\\config.d\\*.conf"),
            Some((
                "include".into(),
                vec!["C:\\Users\\jack\\.ssh\\config.d\\*.conf".into()]
            ))
        );
    }
}
