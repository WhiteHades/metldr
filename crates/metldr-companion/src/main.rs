fn main() {
    if let Err(error) = metldr_companion::run() {
        eprintln!("metldr companion: {error}");
        std::process::exit(1);
    }
}
