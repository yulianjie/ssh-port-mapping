//! Embedded pixels generated from assets/portweave.svg; no runtime file access.

pub const SIZE: u32 = 64;
pub const RGBA: &[u8; (SIZE * SIZE * 4) as usize] = include_bytes!("../assets/portweave-64.rgba");

pub fn window_icon() -> iced::window::Icon {
    iced::window::icon::from_rgba(RGBA.to_vec(), SIZE, SIZE)
        .expect("embedded icon has valid RGBA dimensions")
}
