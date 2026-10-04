//! Writes the installer's splash image (`cargo run --example splash -- splash.png`).
#[allow(dead_code)]
#[path = "../src/art.rs"]
mod art;
#[allow(dead_code)]
#[path = "../src/style.rs"]
mod style;

fn main() -> std::io::Result<()> {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("Usage: splash PATH");
        std::process::exit(2);
    };
    let mut options = resvg::usvg::Options::default();
    for (_, _, file) in style::FONTS {
        options.fontdb_mut().load_font_data(file.to_vec());
    }
    art::write_splash(std::path::Path::new(&path), &options)
}
