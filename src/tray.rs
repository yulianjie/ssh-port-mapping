#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Quit,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod platform {
    use super::TrayAction;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    const SHOW_ID: &str = "portweave-show";
    const QUIT_ID: &str = "portweave-quit";

    pub struct TrayHandle {
        _icon: TrayIcon,
    }

    pub fn create() -> Result<TrayHandle, String> {
        let menu = Menu::new();
        let show = MenuItem::with_id(SHOW_ID, "打开 PortWeave", true, None);
        let quit = MenuItem::with_id(QUIT_ID, "退出", true, None);
        menu.append_items(&[&show, &quit])
            .map_err(|error| error.to_string())?;

        let icon = create_icon()?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("PortWeave · SSH 隧道")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(TrayHandle { _icon: tray })
    }

    pub fn poll() -> Option<TrayAction> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id.0 == SHOW_ID {
                return Some(TrayAction::Show);
            }
            if event.id.0 == QUIT_ID {
                return Some(TrayAction::Quit);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                return Some(TrayAction::Show);
            }
        }
        None
    }

    fn create_icon() -> Result<Icon, String> {
        Icon::from_rgba(
            crate::icon::RGBA.to_vec(),
            crate::icon::SIZE,
            crate::icon::SIZE,
        )
        .map_err(|error| error.to_string())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use platform::{create, poll};

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub struct TrayHandle;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn create() -> Result<TrayHandle, String> {
    Err("系统托盘目前仅支持 Windows 和 macOS".into())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn poll() -> Option<TrayAction> {
    None
}
