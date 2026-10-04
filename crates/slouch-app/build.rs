//! Embeds the app icon in the Windows executable, so Explorer, the taskbar and Task Manager show
//! it. The icon is drawn from the same artwork as the tray and windows.

use std::path::PathBuf;

#[allow(dead_code)]
#[path = "src/art.rs"]
mod art;
#[path = "src/icon_file.rs"]
mod icon_file;

fn main() {
    println!("cargo:rerun-if-changed=src/art.rs");
    println!("cargo:rerun-if-changed=src/icon_file.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let icon = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("slouchup.ico");
    icon_file::write_ico(256, &icon).expect("writing the app icon");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon.to_str().expect("OUT_DIR is UTF-8"))
        .set("FileDescription", "SlouchUp")
        .set("ProductName", "SlouchUp");
    resource.compile().expect("embedding the app icon");
}
