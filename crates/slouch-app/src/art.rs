//! The app's artwork, drawn as SVG so the tray, windows, notifications and launcher share one source.
//!
//! The app icon and the nudge carry the wordmark's "up". The tray carries a pair of eyes on a 24px
//! grid whose shape says how you're sitting, so it still reads where colour doesn't. Colours are
//! the design system's, from `tokens.css`.

use std::path::{Path, PathBuf};

const BUTTER: &str = "#FFF1C9";
const INK: &str = "#22201C";
const TOMATO: &str = "#E8553A";
const TOMATO_FILL: &str = "#EC6247";
// Only the installer splash, drawn by examples/splash.rs, uses these.
#[allow(dead_code)]
const TOMATO_SOFT: &str = "#FF8A6B";
#[allow(dead_code)]
const PAPER: &str = "#FFF8E3";
#[allow(dead_code)]
const STONE: &str = "#5C574C";
#[allow(dead_code)]
const SAND: &str = "#E6DCC0";

/// The wordmark's "up", outlined from Fredoka SemiBold and centred on a 128px tile.
const UP: &str = "M39.5 77.6Q35.9 77.6 32.6 76.2Q29.4 74.8 27 72.2Q24.6 69.6 23.3 66Q22 62.5 22 58.3V43.9Q22 42.3 22.3 40.9Q22.6 39.6 23.9 38.7Q25.1 37.8 28.1 37.8Q31 37.8 32.2 38.7Q33.5 39.6 33.7 41Q34 42.4 34 43.9V58.3Q34 60.6 34.9 62.3Q35.7 64 37.3 64.9Q39 65.8 41.2 65.8Q43.5 65.8 45.1 64.8Q46.8 63.9 47.7 62.3Q48.6 60.6 48.6 58.3V43.8Q48.6 42.2 48.9 40.8Q49.2 39.5 50.5 38.7Q51.7 37.8 54.6 37.8Q57.6 37.8 58.8 38.7Q60.1 39.6 60.3 41Q60.6 42.4 60.6 43.9V71.9Q60.6 73.4 60.3 74.7Q60.1 76 58.8 76.8Q57.5 77.6 54.6 77.6Q52.5 77.6 51.3 77.1Q50.1 76.7 49.5 76Q49 75.2 48.9 74.4Q48.8 73.6 48.8 73L49.8 72.1Q49.5 72.4 48.8 73.3Q48 74.2 46.6 75.2Q45.3 76.2 43.6 76.9Q41.8 77.6 39.5 77.6ZM87.8 77Q83.9 77 81 75.4Q78 73.9 76 71.2Q74.1 68.5 73.1 65Q72.1 61.5 72.2 57.5Q72.2 53.6 73.2 50Q74.2 46.5 76.2 43.7Q78.1 41 81 39.4Q83.9 37.9 87.8 37.9Q91.3 37.9 94.7 39.5Q98 41.1 100.5 43.8Q103.1 46.6 104.5 50.2Q106 53.7 106 57.7Q106 61.7 104.5 65.2Q103.1 68.8 100.5 71.4Q98 74 94.7 75.5Q91.4 77 87.8 77ZM72.5 94.2Q69.6 94.2 68.4 93.3Q67.1 92.5 66.9 91.2Q66.6 89.9 66.6 88.4V43.9Q66.6 42.3 66.8 41Q67.1 39.6 68.3 38.8Q69.5 37.9 72.4 37.9Q74.9 37.9 76.3 38.6Q77.8 39.3 78.5 40.8V88.3Q78.5 89.8 78.2 91.2Q77.9 92.5 76.7 93.3Q75.4 94.2 72.5 94.2ZM86.2 65.8Q88.4 65.8 90.2 64.7Q91.9 63.6 93 61.8Q94.1 59.9 94.1 57.7Q94.1 55.4 93 53.6Q92 51.8 90.2 50.8Q88.5 49.8 86.3 49.8Q84.1 49.8 82.3 50.8Q80.5 51.9 79.5 53.7Q78.5 55.5 78.5 57.7Q78.5 59.9 79.6 61.8Q80.6 63.6 82.4 64.7Q84.1 65.8 86.2 65.8Z";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Good,
    Bad,
    Idle,
    Paused,
}

impl Mood {
    /// Good looks up, bad drops its lids and sinks, idle closes its eyes and wonders, and paused
    /// closes them and sleeps.
    fn eyes(self, colour: &str) -> String {
        let shapes = match self {
            Mood::Good => format!(
                r#"<path d="M3 13a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0 -9 0M12 13a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0 -9 0"/>
<circle cx="7.5" cy="10.6" r="1.7" fill="{colour}" stroke="none"/>
<circle cx="16.5" cy="10.6" r="1.7" fill="{colour}" stroke="none"/>"#
            ),
            Mood::Bad => format!(
                r#"<circle cx="7.5" cy="12" r="4.5"/><circle cx="16.5" cy="12" r="4.5"/>
<path d="M3 10.4h9M12 10.4h9"/>
<circle cx="7.5" cy="14.3" r="1.7" fill="{colour}" stroke="none"/>
<circle cx="16.5" cy="14.3" r="1.7" fill="{colour}" stroke="none"/>"#
            ),
            Mood::Idle => format!(
                r#"<path d="M3 13c1.6 2.2 7.4 2.2 9 0M12 13c1.6 2.2 7.4 2.2 9 0"/>
<path d="M17.4 3.6a1.7 1.7 0 1 1 2.4 1.6c-.5.3-.8.6-.8 1.2"/>
<circle cx="19" cy="8.6" r="0.6" fill="{colour}" stroke="none"/>"#
            ),
            Mood::Paused => r#"<path d="M3 13c1.6 2.2 7.4 2.2 9 0M12 13c1.6 2.2 7.4 2.2 9 0"/>
<path d="M16.6 3.6h3.4l-3.4 4.4h3.4"/>"#
                .to_string(),
        };
        format!(
            r#"<g fill="none" stroke="{colour}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{shapes}</g>"#
        )
    }
}

/// The design system's two themes: day for light grounds, night for dark ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Day,
    Night,
}

impl Theme {
    /// The value for a window's `data-theme`, which picks the theme's tokens in `tokens.css`.
    pub fn name(self) -> &'static str {
        match self {
            Theme::Day => "day",
            Theme::Night => "night",
        }
    }

    /// The `surface` token, for painting a window before its page loads.
    pub fn ground(self) -> (u8, u8, u8, u8) {
        match self {
            Theme::Day => (0xFF, 0xF8, 0xE3, 0xFF),
            Theme::Night => (0x22, 0x20, 0x1C, 0xFF),
        }
    }

    /// The `state-upright`, `state-slouch` and `state-lost` tokens.
    fn state(self, mood: Mood) -> &'static str {
        match (self, mood) {
            (Theme::Day, Mood::Good) => INK,
            (Theme::Day, Mood::Bad) => "#C8432A",
            (Theme::Day, Mood::Idle | Mood::Paused) => "#77726A",
            (Theme::Night, Mood::Good) => BUTTER,
            (Theme::Night, Mood::Bad) => "#FF8A6B",
            (Theme::Night, Mood::Idle | Mood::Paused) => "#A39E93",
        }
    }
}

/// The eyes alone, for the tray on a panel in `theme`.
pub fn tray_svg(mood: Mood, theme: Theme) -> String {
    eyes_svg(mood, theme.state(mood))
}

/// The eyes for inline use in a page, coloured by the surrounding text colour.
pub fn eyes_markup(mood: Mood) -> String {
    eyes_svg(mood, "currentColor")
}

fn eyes_svg(mood: Mood, colour: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" aria-hidden="true">{}</svg>"#,
        mood.eyes(colour)
    )
}

/// Big friendly eyes looking up, asking you to look at the screen being calibrated.
pub fn looking_up_svg() -> String {
    let eyes = [52, 148]
        .map(|x| {
            format!(
                r#"<circle cx="{x}" cy="48" r="44" fill="{BUTTER}"/><circle cx="{x}" cy="30" r="17" fill="{INK}"/>"#
            )
        })
        .concat();
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 96" aria-hidden="true">{eyes}</svg>"#
    )
}

/// A rounded 128px tile holding `content` drawn in tile units.
fn tile_svg(ground: &str, content: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128"><rect width="128" height="128" rx="30" fill="{ground}"/>{content}</svg>"#
    )
}

/// The app icon: the wordmark's "up" in butter on tomato.
pub fn app_icon_svg() -> String {
    tile_svg(TOMATO_FILL, &format!(r#"<path d="{UP}" fill="{BUTTER}"/>"#))
}

#[allow(dead_code)] // Drawn by examples/splash.rs for packaging, not by the app.
/// The installer's splash: the homepage's OpenGraph card at half its size, with the wordmark, what
/// SlouchUp does for you, and the nudge that does it. Its text is set in Figtree and Fredoka.
pub fn splash_svg() -> String {
    let display = r#"font-family="Figtree" font-weight="800" letter-spacing="-0.8""#;
    let wordmark = r#"font-family="Fredoka" font-weight="600""#;
    let body = r#"font-family="Figtree""#;
    let headline = ["Better posture", "without thinking", "about it."]
        .iter()
        .zip([150, 186, 222])
        .map(|(line, y)| {
            format!(r#"<text x="36" y="{y}" {display} font-size="32" fill="{INK}">{line}</text>"#)
        })
        .collect::<String>();
    let subhead = [
        "One gentle nudge when you start",
        "to slouch, then it leaves you alone.",
    ]
    .iter()
    .zip([272, 294])
    .map(|(line, y)| {
        format!(r#"<text x="36" y="{y}" {body} font-size="16" fill="{STONE}">{line}</text>"#)
    })
    .collect::<String>();
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 400"><rect width="600" height="400" fill="{PAPER}"/><rect x="350" y="20" width="280" height="360" rx="16" fill="{SAND}"/><text x="36" y="56" {wordmark} font-size="26" fill="{INK}">slouch<tspan dy="-6.5" fill="{TOMATO}">up</tspan></text>{headline}{subhead}<g transform="translate(306 148) scale(0.5)">{nudge}</g></svg>"#,
        nudge = nudge_card_svg(),
    )
}

/// The nudge as the homepage's card draws it, 560 wide, at one and a half times its size on screen.
#[allow(dead_code)]
fn nudge_card_svg() -> String {
    format!(
        r#"<rect x="9" y="9" width="560" height="226" rx="26" fill="{TOMATO}"/><rect width="560" height="226" rx="26" fill="{INK}"/><svg x="26" y="26" width="66" height="66">{tile}</svg><text x="114" y="68" font-family="Figtree" font-weight="800" font-size="34" letter-spacing="-0.85" fill="{BUTTER}">Psst, sit <tspan dy="-8" font-family="Fredoka" font-weight="600" letter-spacing="0" fill="{TOMATO_SOFT}">up</tspan></text><text x="114" y="110" font-family="Figtree" font-size="22" fill="{SAND}">Your head has sunk lower than usual.</text><rect x="114" y="136" width="128" height="64" rx="32" fill="{TOMATO_FILL}"/><text x="178" y="176" text-anchor="middle" font-family="Figtree" font-weight="600" font-size="24" fill="{INK}">I'm up</text><rect x="255.5" y="137.5" width="141" height="61" rx="30.5" fill="none" stroke="{BUTTER}" stroke-width="3"/><text x="326" y="176" text-anchor="middle" font-family="Figtree" font-weight="600" font-size="24" fill="{BUTTER}">Snooze</text>"#,
        tile = nudge_icon_svg()
    )
}

/// Write the splash as a PNG at its own 600x400 size, with `options` holding its fonts.
#[allow(dead_code)] // Used by examples/splash.rs.
pub fn write_splash(path: &Path, options: &resvg::usvg::Options) -> std::io::Result<()> {
    render_with(&splash_svg(), 600, 400, options)
        .save_png(path)
        .map_err(std::io::Error::other)
}

/// The nudge's icon: the app icon's colours swapped, so it stands apart from other notices.
fn nudge_icon_svg() -> String {
    tile_svg(BUTTER, &format!(r#"<path d="{UP}" fill="{TOMATO}"/>"#))
}

fn render(svg: &str, width: u32, height: u32) -> resvg::tiny_skia::Pixmap {
    render_with(svg, width, height, &resvg::usvg::Options::default())
}

fn render_with(
    svg: &str,
    width: u32,
    height: u32,
    options: &resvg::usvg::Options,
) -> resvg::tiny_skia::Pixmap {
    let tree = resvg::usvg::Tree::from_str(svg, options).expect("built-in SVG parses");
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

pub fn write_png(svg: &str, size: u32, path: &Path) -> std::io::Result<()> {
    render(svg, size, size)
        .save_png(path)
        .map_err(std::io::Error::other)
}

/// Files notifications point at, since notification servers load images by path.
#[derive(Clone)]
pub struct Files {
    pub icon: PathBuf,
    pub nudge_icon: PathBuf,
}

pub fn write_files(dir: &Path) -> std::io::Result<Files> {
    std::fs::create_dir_all(dir)?;
    let files = Files {
        icon: dir.join("slouchup.png"),
        nudge_icon: dir.join("nudge.png"),
    };
    write_png(&app_icon_svg(), 128, &files.icon)?;
    write_png(&nudge_icon_svg(), 128, &files.nudge_icon)?;
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses_and_draws_something() {
        let mut svgs = vec![app_icon_svg(), nudge_icon_svg(), looking_up_svg()];
        for mood in [Mood::Good, Mood::Bad, Mood::Idle] {
            svgs.extend([tray_svg(mood, Theme::Day), tray_svg(mood, Theme::Night)]);
        }
        for svg in svgs {
            let pixels = rasterise(&svg, 32, 32);
            assert!(pixels.chunks(4).any(|p| p[3] > 0), "blank icon: {svg}");
        }
    }
}
