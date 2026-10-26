use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

const MAX_BYTES: u64 = 256 * 1024;
static TEMP_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    name: String,
    url: String,
    sha256: String,
    bytes: u64,
}

pub(super) fn prepare(client: &Client, output: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(output)?;
    for asset in serde_json::from_str::<Vec<Asset>>(include_str!("../assets.json"))? {
        install(client, output, &asset)?;
    }
    Ok(())
}

fn install(client: &Client, output: &Path, asset: &Asset) -> Result<(), Box<dyn Error>> {
    if Path::new(&asset.name)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(&asset.name)
    {
        return Err(format!("invalid asset name: {}", asset.name).into());
    }
    let destination = output.join(&asset.name);
    if valid_file(&destination, asset) {
        return Ok(());
    }

    let response = client.get(&asset.url).send()?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BYTES)
    {
        return Err(format!("asset exceeds size limit: {}", asset.name).into());
    }
    let mut body = Vec::new();
    response.take(MAX_BYTES + 1).read_to_end(&mut body)?;
    if body.len() as u64 > MAX_BYTES {
        return Err(format!("asset exceeds size limit: {}", asset.name).into());
    }
    if body.len() as u64 != asset.bytes || format!("{:x}", Sha256::digest(&body)) != asset.sha256 {
        return Err(format!("asset verification failed: {}", asset.name).into());
    }

    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = output.join(format!(".metldr-asset-{}-{id}.tmp", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => break (path, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| -> Result<(), Box<dyn Error>> {
        file.write_all(&body)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn valid_file(path: &Path, asset: &Asset) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if metadata.len() != asset.bytes || metadata.len() > MAX_BYTES {
        return false;
    }
    fs::read(path)
        .map(|body| format!("{:x}", Sha256::digest(body)) == asset.sha256)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        thread,
        time::Duration,
    };

    #[test]
    fn bad_response_preserves_output_then_valid_fetch_is_cached() {
        let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/rust-panel-assets")
            .join(format!("check-{}", std::process::id()));
        fs::create_dir_all(test_dir.parent().unwrap()).unwrap();
        fs::create_dir(&test_dir).unwrap();
        let output = test_dir.join("output");
        fs::create_dir_all(&output).unwrap();
        let asset = Asset {
            name: "sample.bin".into(),
            url: String::new(),
            sha256: format!("{:x}", Sha256::digest(b"good")),
            bytes: 4,
        };
        let destination = output.join(&asset.name);
        fs::write(&destination, b"previous").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/asset", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for body in [b"evil".as_slice(), b"good".as_slice()] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 512];
                let _ = stream.read(&mut request);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let mut asset = asset;
        asset.url = url;
        assert!(install(&client, &output, &asset).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"previous");
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
        install(&client, &output, &asset).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"good");
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
        server.join().unwrap();
        install(&client, &output, &asset).unwrap();
        fs::remove_dir_all(test_dir).unwrap();
    }
}
