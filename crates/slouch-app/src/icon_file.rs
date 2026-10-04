//! The app icon as a Windows .ico, for the exe and its installer. Not part of the app: build.rs
//! and the icon example include it.

use std::fs::File;
use std::io;
use std::path::Path;

use super::art;

/// The icon at every size Windows picks from, from title bars to large Explorer views, up to
/// `largest` pixels square.
pub fn write_ico(largest: u32, path: &Path) -> io::Result<()> {
    let mut icon = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16, 24, 32, 48, 64, 128, 256] {
        if size > largest {
            break;
        }
        let rgba = art::rasterise(&art::app_icon_svg(), size, size);
        icon.add_entry(ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
            size, size, rgba,
        ))?);
    }
    icon.write(File::create(path)?)
}
