// Manual smoke-test example: posts one `.StormReplay` file to a server's `/api/upload-raw`.
// Usage: cargo run --example test_upload -- <path-to-replay> [server-url]
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: test_upload <path-to-replay> [server-url]");
    let server = args
        .next()
        .or_else(|| std::env::var("SERVER_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:5102".to_string());

    let filename = std::path::Path::new(&path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let file_bytes = std::fs::read(&path).unwrap();
    println!("File: {}, size: {}", filename, file_bytes.len());

    let client = ureq::Agent::new_with_defaults();
    let mut resp = client
        .post(format!("{server}/api/upload-raw"))
        .header("Content-Type", "application/octet-stream")
        .header("X-Filename", &filename)
        .send(file_bytes.as_slice())
        .unwrap();
    println!("Response: {}", resp.body_mut().read_to_string().unwrap());
}
