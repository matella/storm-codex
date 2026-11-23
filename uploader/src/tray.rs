use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

use crate::win_utils;

/// Build the tray icon and its context menu.
/// Returns the tray icon handle (must be kept alive).
pub fn create_tray() -> TrayIcon {
    // Build context menu
    let menu = Menu::new();
    let show_item = MenuItem::new("Open Dashboard", true, None);
    let show_id = show_item.id().clone();
    menu.append(&show_item).ok();

    // Load icon from embedded bytes
    let icon_bytes = include_bytes!("../assets/icon.png");
    let icon_image = image::load_from_memory(icon_bytes)
        .expect("Failed to load tray icon")
        .into_rgba8();
    let (w, h) = icon_image.dimensions();
    let icon =
        Icon::from_rgba(icon_image.into_raw(), w, h).expect("Failed to create tray icon from RGBA");

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("HotS Replay Client")
        .with_icon(icon)
        .build()
        .expect("Failed to create tray icon");

    // Spawn thread to listen for menu events.
    // Calls the Windows API directly to show the window — this works
    // even when eframe's update() loop isn't running (window hidden).
    std::thread::spawn(move || {
        let receiver = MenuEvent::receiver();
        loop {
            match receiver.recv() {
                Ok(event) if event.id == show_id => {
                    win_utils::show_window(win_utils::WINDOW_TITLE);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    tray
}
