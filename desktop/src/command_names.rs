/// The shell's IPC commands (`#[tauri::command]` in `commands.rs`), in one list read by
/// `build.rs` (the app manifest) and by the capability test.
#[allow(dead_code)] // read by build.rs and the tests
pub const APP_COMMANDS: &[&str] = &[
    "update_status",
    "app_version",
    "check_for_update_now",
    "install_update",
    "export_data",
    "import_data",
    "set_autostart",
    "get_autostart",
];
