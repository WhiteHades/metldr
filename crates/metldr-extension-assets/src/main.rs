mod assets;

use std::{error::Error, time::Duration};

fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::current_dir()?;
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()?;
    assets::prepare(
        &client,
        &root.join("public/rust-panel/assets"),
        &root.join(".tmp/rust-panel-assets"),
    )
}
