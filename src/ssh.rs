use crate::config::TunnelConfig;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum ProcessEvent {
    Output { id: Uuid, line: String },
}

#[derive(Debug)]
pub struct RunningTunnel {
    pub child: Child,
    pub started_at: Instant,
}

#[derive(Debug)]
pub struct TunnelExit {
    pub id: Uuid,
    pub result: Result<i32, String>,
    pub runtime: Duration,
}

#[derive(Debug, Default)]
pub struct TunnelManager {
    running: HashMap<Uuid, RunningTunnel>,
}

impl TunnelManager {
    pub fn start(
        &mut self,
        config: &TunnelConfig,
        events: Sender<ProcessEvent>,
    ) -> Result<u32, String> {
        if let Some(running) = self.running.get(&config.id) {
            return Ok(running.child.id());
        }

        config.validate()?;
        let mut command = Command::new("ssh");
        command
            .args(command_args(config))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动 OpenSSH：{error}"))?;
        let pid = child.id();

        if let Some(stderr) = child.stderr.take() {
            let id = config.id;
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if events.send(ProcessEvent::Output { id, line }).is_err() {
                        break;
                    }
                }
            });
        }

        self.running.insert(
            config.id,
            RunningTunnel {
                child,
                started_at: Instant::now(),
            },
        );
        Ok(pid)
    }

    pub fn stop(&mut self, id: Uuid) -> Result<bool, String> {
        let Some(mut running) = self.running.remove(&id) else {
            return Ok(false);
        };
        running
            .child
            .kill()
            .map_err(|error| format!("无法停止隧道：{error}"))?;
        let _ = running.child.wait();
        Ok(true)
    }

    pub fn stop_all(&mut self) {
        for (_, mut running) in self.running.drain() {
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }

    pub fn poll_exited(&mut self) -> Vec<TunnelExit> {
        let ids: Vec<_> = self.running.keys().copied().collect();
        let mut exited = Vec::new();
        for id in ids {
            let result =
                self.running
                    .get_mut(&id)
                    .and_then(|running| match running.child.try_wait() {
                        Ok(Some(status)) => Some(Ok(status.code().unwrap_or(-1))),
                        Ok(None) => None,
                        Err(error) => Some(Err(error.to_string())),
                    });
            if let Some(result) = result
                && let Some(running) = self.running.remove(&id)
            {
                exited.push(TunnelExit {
                    id,
                    result,
                    runtime: running.started_at.elapsed(),
                });
            }
        }
        exited
    }

    pub fn pid(&self, id: Uuid) -> Option<u32> {
        self.running.get(&id).map(|running| running.child.id())
    }

    pub fn is_running(&self, id: Uuid) -> bool {
        self.running.contains_key(&id)
    }

    pub fn running_count(&self) -> usize {
        self.running.len()
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

pub fn command_args(config: &TunnelConfig) -> Vec<OsString> {
    let mapping = format!(
        "{}:{}:{}:{}",
        config.bind_address, config.bind_port, config.target_host, config.target_port
    );
    let mut args = vec![
        OsString::from("-N"),
        OsString::from("-T"),
        OsString::from("-o"),
        OsString::from("ExitOnForwardFailure=yes"),
        OsString::from("-o"),
        OsString::from("BatchMode=yes"),
        OsString::from("-o"),
        OsString::from("ServerAliveInterval=30"),
        OsString::from("-o"),
        OsString::from("ServerAliveCountMax=3"),
        OsString::from("-p"),
        OsString::from(config.ssh_port.to_string()),
    ];
    if let Some(identity_file) = config.identity_file.as_ref() {
        args.push(OsString::from("-i"));
        args.push(identity_file.as_os_str().to_owned());
    }
    if let Some(proxy_jump) = config.proxy_jump.as_ref() {
        args.push(OsString::from("-J"));
        args.push(OsString::from(proxy_jump));
    }
    args.push(OsString::from(config.kind.ssh_flag()));
    args.push(OsString::from(mapping));
    args.push(OsString::from(format!("{}@{}", config.user, config.host)));
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ForwardKind;
    use std::path::PathBuf;

    fn remote_config() -> TunnelConfig {
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
            auto_reconnect: true,
            reconnect_attempts: crate::config::DEFAULT_RECONNECT_ATTEMPTS,
            reconnect_interval_secs: crate::config::DEFAULT_RECONNECT_INTERVAL_SECS,
        }
    }

    #[test]
    fn builds_expected_remote_forward_arguments() {
        let args = command_args(&remote_config());
        let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        assert!(args.windows(2).any(|pair| pair == ["-p", "2222"]));
        assert!(args.windows(2).any(|pair| pair == ["-o", "BatchMode=yes"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-R", "127.0.0.1:7897:127.0.0.1:7897"])
        );
        assert_eq!(args.last().unwrap(), "developer@203.0.113.10");
    }

    #[test]
    fn identity_file_is_one_unsplit_argument() {
        let mut config = remote_config();
        config.identity_file = Some(PathBuf::from("C:/Keys/my server key"));
        let args = command_args(&config);
        assert!(args.contains(&OsString::from("C:/Keys/my server key")));
    }

    #[test]
    fn builds_local_forward_arguments() {
        let mut config = remote_config();
        config.kind = ForwardKind::Local;
        let args = command_args(&config);
        let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-L", "127.0.0.1:7897:127.0.0.1:7897"])
        );
    }

    #[test]
    fn builds_proxy_jump_chain_as_one_argument() {
        let mut config = remote_config();
        config.proxy_jump = Some("bastion,ops@edge.example:2202".into());
        let args = command_args(&config);
        let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-J", "bastion,ops@edge.example:2202"])
        );
    }
}
