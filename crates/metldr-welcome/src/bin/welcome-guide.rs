use std::{
    env, fs,
    io::{self, Write},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let template = args.next().ok_or("expected template path")?;
    let output = args.next().ok_or("expected output path")?;
    if args.next().is_some() {
        return Err("expected only template and output paths".into());
    }
    let mut html = fs::read_to_string(template)?;
    for (marker, content) in [
        ("<!--WELCOME_FAQS-->", welcome_runtime::guide::render_html()),
        (
            "<!--WELCOME_SETUP-->",
            welcome_runtime::setup_guide::render_html(),
        ),
    ] {
        if html.matches(marker).count() != 1 {
            return Err(format!("welcome template must contain exactly one {marker}").into());
        }
        html = html.replacen(marker, &content, 1);
    }
    if output == "-" {
        io::stdout().lock().write_all(html.as_bytes())?;
    } else {
        fs::write(output, html)?;
    }
    Ok(())
}
