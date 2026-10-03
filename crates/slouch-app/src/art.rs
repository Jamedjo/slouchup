//! The app's artwork, drawn as SVG so the tray, notifications and launcher share one source.

use std::path::{Path, PathBuf};

pub const RAINBOW: [&str; 6] = [
    "#ff5050", "#ffb43c", "#ffe650", "#5adc78", "#3ca0ff", "#c85adc",
];
const GOOD: [&str; 2] = ["#5adc78", "#3cc8c8"];
const BAD: [&str; 2] = ["#ff3c3c", "#ff8c3c"];
const IDLE: [&str; 2] = ["#8c8c8c", "#5a5a5a"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Rainbow,
    Good,
    Bad,
    Idle,
}

impl Mood {
    fn colours(self) -> &'static [&'static str] {
        match self {
            Mood::Rainbow => &RAINBOW,
            Mood::Good => &GOOD,
            Mood::Bad => &BAD,
            Mood::Idle => &IDLE,
        }
    }
}

fn gradient(id: &str, colours: &[&str]) -> String {
    let stops: String = colours
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                r#"<stop offset="{}" stop-color="{c}"/>"#,
                i as f32 / (colours.len() - 1) as f32
            )
        })
        .collect();
    format!(r#"<linearGradient id="{id}" x1="0" y1="0" x2="1" y2="0">{stops}</linearGradient>"#)
}

/// A round badge with a person standing tall.
pub fn icon_svg(mood: Mood) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128"><defs>{}</defs>
<circle cx="64" cy="64" r="62" fill="url(#g)"/>
<circle cx="64" cy="40" r="16" fill="#fff"/>
<rect x="59" y="56" width="10" height="48" rx="5" fill="#fff"/></svg>"##,
        gradient("g", mood.colours())
    )
}

fn banner_svg() -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 60"><defs>{}</defs>
<rect width="400" height="60" fill="url(#g)"/>
<text x="200" y="41" text-anchor="middle" font-family="sans-serif" font-weight="bold" font-size="30" fill="#fff">SIT UP STRAIGHT</text></svg>"##,
        gradient("g", &RAINBOW)
    )
}

fn render(svg: &str, width: u32, height: u32) -> resvg::tiny_skia::Pixmap {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options).expect("built-in SVG parses");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).expect("non-zero size");
    let size = tree.size();
    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap
}

/// Rasterise an SVG to straight (not premultiplied) RGBA.
pub fn rasterise(svg: &str, width: u32, height: u32) -> Vec<u8> {
    render(svg, width, height)
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

pub fn icon_png(mood: Mood, size: u32) -> Vec<u8> {
    render(&icon_svg(mood), size, size)
        .encode_png()
        .expect("PNG encodes")
}

fn write_png(svg: &str, width: u32, height: u32, path: &Path) -> std::io::Result<()> {
    render(svg, width, height)
        .save_png(path)
        .map_err(std::io::Error::other)
}

/// Files notifications point at, since notification servers load images by path.
#[derive(Clone)]
pub struct Files {
    pub icon: PathBuf,
    pub banner: PathBuf,
}

pub fn write_files(dir: &Path) -> std::io::Result<Files> {
    std::fs::create_dir_all(dir)?;
    let files = Files {
        icon: dir.join("slouch.png"),
        banner: dir.join("banner.png"),
    };
    write_png(&icon_svg(Mood::Rainbow), 128, 128, &files.icon)?;
    write_png(&banner_svg(), 400, 60, &files.banner)?;
    Ok(files)
}
