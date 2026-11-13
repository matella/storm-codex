// The shell's own commands, shared with `src/command_names.rs` so the capability test and
// the build agree on one list.
include!("src/command_names.rs");

fn main() {
    // `AppManifest::commands`: each app command is denied unless a capability grants its
    // generated `allow-<command>` permission (by default every registered command would be
    // allowed for every window).
    let attrs = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS));
    if let Err(e) = tauri_build::try_build(attrs) {
        eprintln!("tauri build failed: {e:#}");
        std::process::exit(1);
    }
}
