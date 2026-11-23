use std::path::Path;

fn main() {
    // Load uploader/.env if it exists. NOT the repo root .env: in storm-codex that file is the
    // server's own config (DATABASE_URL, ADMIN_TOKEN, ...) — a build script here must never read
    // or (via build-uploader.ps1) overwrite it.
    let env_path = Path::new(".env");
    if env_path.exists() {
        if let Ok(contents) = std::fs::read_to_string(env_path) {
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();
                    // Only set if not already in environment (env vars take priority)
                    if std::env::var(key).is_err() {
                        std::env::set_var(key, value);
                    }
                }
            }
        }
        // Only when the file exists: Cargo's change-tracking treats a rerun-if-changed path that
        // doesn't exist as permanently dirty, forcing a rebuild on every single invocation (even
        // back-to-back, zero-change ones) for everyone who never created a uploader/.env — i.e.
        // almost every contributor and every CI run. The cost of this is just not noticing a
        // same-session env file created after an already-cached build (run `cargo clean -p
        // hots-replay-client` if that specific case ever matters), far cheaper than rebuilding the
        // crate on every build for everyone else.
        println!("cargo:rerun-if-changed=.env");
    }

    // Defaults baked into the binary. NEUTRAL by default (empty) — le serveur et le token se
    // configurent à l'exécution (env pour Docker, GUI 1er lancement pour l'.exe). On ne grave plus
    // d'URL/token : un binaire distribué ne doit contenir aucun secret ni pointer un serveur précis.
    // (Un build interne peut toujours fournir SERVER_URL/AUTH_TOKEN via uploader/.env s'il le souhaite.)
    let server_url = std::env::var("SERVER_URL").unwrap_or_default();
    let auth_token = std::env::var("AUTH_TOKEN").unwrap_or_default();

    println!("cargo:rustc-env=DEFAULT_SERVER_URL={server_url}");
    println!("cargo:rustc-env=DEFAULT_AUTH_TOKEN={auth_token}");

    // Re-run if env vars change
    println!("cargo:rerun-if-env-changed=SERVER_URL");
    println!("cargo:rerun-if-env-changed=AUTH_TOKEN");

    // Windows: embed icon in exe
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
}
