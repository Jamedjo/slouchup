//! Writes the app icon for packaging, as a square PNG (`cargo run --example icon -- 512 icon.png`)
//! or as a Windows .ico holding every size up to the one given.
#[allow(dead_code)]
#[path = "../src/art.rs"]
mod art;
#[path = "../src/icon_file.rs"]
mod icon_file;

use std::path::Path;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(size), Some(path)) = (args.next(), args.next()) else {
        eprintln!("Usage: icon SIZE PATH");
        std::process::exit(2);
    };
    let size = size.parse().expect("SIZE is a whole number of pixels");
    let path = Path::new(&path);
    if path.extension().is_some_and(|extension| extension == "ico") {
        icon_file::write_ico(size, path)
    } else {
        art::write_png(&art::app_icon_svg(), size, path)
    }
}
