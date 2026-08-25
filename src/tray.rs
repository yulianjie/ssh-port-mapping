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
        let show = MenuItem::with_id(SHOW_ID, "Open PortWeave", true, None);
        let quit = MenuItem::with_id(QUIT_ID, "Quit", true, None);
        menu.append_items(&[&show, &quit])
            .map_err(|error| error.to_string())?;

        let icon = create_icon()?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("PortWeave · SSH tunnels")
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
        const SIZE: u32 = 32;
        let mut rgba = vec![0_u8; (SIZE * SIZE * 4) as usize];
        for y in 0..SIZE {
            for x in 0..SIZE {
                let index = ((y * SIZE + x) * 4) as usize;
                let dx = x as i32 - 15;
                let dy = y as i32 - 15;
                if dx * dx + dy * dy <= 14 * 14 {
                    rgba[index..index + 4].copy_from_slice(&[47, 129, 247, 255]);
                }
                let left_link = (7..=15).contains(&x) && (13..=18).contains(&y);
                let right_link = (16..=24).contains(&x) && (13..=18).contains(&y);
                let bridge = (13..=18).contains(&x) && (10..=21).contains(&y);
                if left_link || right_link || bridge {
                    rgba[index..index + 4].copy_from_slice(&[239, 246, 255, 255]);
                }
            }
        }
        Icon::from_rgba(rgba, SIZE, SIZE).map_err(|error| error.to_string())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use platform::{create, poll};

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub struct TrayHandle;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn create() -> Result<TrayHandle, String> {
    Err("tray integration is currently supported on Windows and macOS".into())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn poll() -> Option<TrayAction> {
    None
}
