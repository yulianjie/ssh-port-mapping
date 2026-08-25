use std::path::Path;

const APP_VALUE_NAME: &str = "PortWeave";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub const fn is_supported() -> bool {
    cfg!(target_os = "windows")
}

#[cfg(target_os = "windows")]
pub fn is_enabled() -> Result<bool, String> {
    use std::io::ErrorKind;
    use winreg::HKCU;

    let executable =
        std::env::current_exe().map_err(|error| format!("无法确定 PortWeave 程序路径：{error}"))?;
    let key = match HKCU.open_subkey(RUN_KEY) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("无法读取 Windows 启动项：{error}")),
    };
    let registered: String = match key.get_value(APP_VALUE_NAME) {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("无法读取 PortWeave 启动项：{error}")),
    };

    Ok(command_matches(&registered, &executable))
}

#[cfg(not(target_os = "windows"))]
pub fn is_enabled() -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_os = "windows")]
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    use std::io::ErrorKind;
    use winreg::HKCU;
    use winreg::enums::KEY_SET_VALUE;

    if enabled {
        let executable = std::env::current_exe()
            .map_err(|error| format!("无法确定 PortWeave 程序路径：{error}"))?;
        let (key, _) = HKCU
            .create_subkey(RUN_KEY)
            .map_err(|error| format!("无法打开 Windows 启动项：{error}"))?;
        key.set_value(APP_VALUE_NAME, &startup_command(&executable))
            .map_err(|error| format!("无法注册 PortWeave 开机启动：{error}"))
    } else {
        let key = match HKCU.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
            Ok(key) => key,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("无法打开 Windows 启动项：{error}")),
        };
        match key.delete_value(APP_VALUE_NAME) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("无法移除 PortWeave 开机启动：{error}")),
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn set_enabled(_enabled: bool) -> Result<(), String> {
    Err("当前系统尚不支持开机启动设置".into())
}

fn startup_command(executable: &Path) -> String {
    format!(r#""{}""#, executable.display())
}

fn command_matches(registered: &str, executable: &Path) -> bool {
    registered
        .trim()
        .eq_ignore_ascii_case(&startup_command(executable))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_executable_paths_for_the_windows_run_key() {
        let command = startup_command(Path::new(r"C:\Program Files\PortWeave\portweave.exe"));
        assert_eq!(command, r#""C:\Program Files\PortWeave\portweave.exe""#);
    }

    #[test]
    fn compares_registered_commands_case_insensitively() {
        let executable = Path::new(r"C:\Apps\PortWeave\portweave.exe");
        assert!(command_matches(
            r#"  "c:\apps\portweave\PORTWEAVE.EXE"  "#,
            executable
        ));
        assert!(!command_matches(
            r#""C:\Apps\Other\portweave.exe""#,
            executable
        ));
    }
}
