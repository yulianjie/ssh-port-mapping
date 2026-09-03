use std::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub enum NotificationEvent {
    Shown,
    Failed(String),
}

pub fn show_failure(
    title: String,
    message: String,
    events: Sender<NotificationEvent>,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::thread::Builder::new()
            .name("portweave-notification".into())
            .spawn(move || platform::show_failure(&title, &message, events))
            .map(|_| ())
            .map_err(|error| format!("无法创建通知线程：{error}"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, message);
        events
            .send(NotificationEvent::Failed(
                "系统通知目前仅支持 Windows".into(),
            ))
            .map_err(|error| format!("无法报告通知状态：{error}"))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::NotificationEvent;
    use std::mem::size_of;
    use std::ptr;
    use std::sync::mpsc::Sender;
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_INFO, NIF_TIP, NIIF_ERROR, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
        Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE, IDI_ERROR, LoadIconW, WS_EX_TOOLWINDOW,
        WS_OVERLAPPED,
    };

    const NOTIFICATION_LIFETIME: Duration = Duration::from_secs(10);
    const NOTIFICATION_ID: u32 = 1;

    pub fn show_failure(title: &str, message: &str, events: Sender<NotificationEvent>) {
        match create_notification(title, message) {
            Ok(notification) => {
                let _ = events.send(NotificationEvent::Shown);
                thread::sleep(NOTIFICATION_LIFETIME);
                drop(notification);
            }
            Err(error) => {
                let _ = events.send(NotificationEvent::Failed(error));
            }
        }
    }

    struct Notification {
        data: NOTIFYICONDATAW,
    }

    impl Drop for Notification {
        fn drop(&mut self) {
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &self.data);
                DestroyWindow(self.data.hWnd);
            }
        }
    }

    fn create_notification(title: &str, message: &str) -> Result<Notification, String> {
        let class_name = wide_null("STATIC");
        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                ptr::null(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if window.is_null() {
            return Err(format!(
                "无法创建 Windows 通知窗口：{}",
                std::io::Error::last_os_error()
            ));
        }

        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: NOTIFICATION_ID,
            uFlags: NIF_ICON | NIF_TIP | NIF_INFO,
            hIcon: unsafe { LoadIconW(ptr::null_mut(), IDI_ERROR) },
            dwInfoFlags: NIIF_ERROR,
            ..Default::default()
        };
        copy_utf16(&mut data.szTip, "PortWeave");
        copy_utf16(&mut data.szInfoTitle, title);
        copy_utf16(&mut data.szInfo, message);

        if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
            unsafe {
                DestroyWindow(window);
            }
            return Err(format!(
                "Windows 未能显示系统通知：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Notification { data })
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain([0]).collect()
    }

    fn copy_utf16<const N: usize>(target: &mut [u16; N], value: &str) {
        let mut offset = 0;
        for character in value.chars() {
            let mut buffer = [0_u16; 2];
            let encoded = character.encode_utf16(&mut buffer);
            if offset + encoded.len() >= N {
                break;
            }
            target[offset..offset + encoded.len()].copy_from_slice(encoded);
            offset += encoded.len();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn notification_text_is_null_terminated_without_splitting_surrogates() {
            let mut target = [0_u16; 5];
            copy_utf16(&mut target, "ab😀c");
            assert_eq!(String::from_utf16_lossy(&target[..4]), "ab😀");
            assert_eq!(target[4], 0);

            let mut short = [0_u16; 4];
            copy_utf16(&mut short, "ab😀");
            assert_eq!(String::from_utf16_lossy(&short[..2]), "ab");
            assert_eq!(short[2], 0);
        }
    }
}
