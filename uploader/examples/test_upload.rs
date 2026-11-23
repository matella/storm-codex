fn main() {
    let path = r"C:\Users\matth\Documents\Heroes of the Storm\Accounts\142414274\2-Hero-1-2844614\Replays\Multiplayer\2024-08-18 16.35.38 Cursed Hollow.StormReplay";
    let filename = std::path::Path::new(path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let file_bytes = std::fs::read(path).unwrap();
    println!("File: {}, size: {}", filename, file_bytes.len());

    let client = ureq::Agent::new_with_defaults();
    let mut resp = client
        .post("https://hots-overlay.azurewebsites.net/api/upload-raw")
        .header("Content-Type", "application/octet-stream")
        .header("X-Filename", &filename)
        .send(file_bytes.as_slice())
        .unwrap();
    println!("Response: {}", resp.body_mut().read_to_string().unwrap());
}
