//! Embeds the app icon in the Windows executable, so Explorer, the taskbar and Task Manager show
//! it. The icon is drawn from the same artwork as the tray and windows.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[path = "src/art.rs"]
mod art;

fn main() {
    println!("cargo:rerun-if-changed=src/art.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let icon = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("slouchup.ico");
    write_icon(&icon).expect("writing the app icon");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon.to_str().expect("OUT_DIR is UTF-8"))
        .set("FileDescription", "slouchup")
        .set("ProductName", "slouchup");
    resource.compile().expect("embedding the app icon");
}

/// The icon at every size Windows picks from, from title bars to large Explorer views.
fn write_icon(path: &Path) -> io::Result<()> {
    let mut icon = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let rgba = art::rasterise(&art::app_icon_svg(), size, size);
        icon.add_entry(ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
            size, size, rgba,
        ))?);
    }
    icon.write(File::create(path)?)
}
