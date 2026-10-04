//! Writes the app icon as a square PNG, for packaging: `cargo run --example icon -- 512 icon.png`.
#[allow(dead_code)]
#[path = "../src/art.rs"]
mod art;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(size), Some(path)) = (args.next(), args.next()) else {
        eprintln!("Usage: icon SIZE PATH");
        std::process::exit(2);
    };
    let size = size.parse().expect("SIZE is a whole number of pixels");
    art::write_png(&art::app_icon_svg(), size, path.as_ref())
}
