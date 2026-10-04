//! Every window's stylesheet: the design system's tokens, its two typefaces, and the app's rules.
//!
//! The typefaces are built in rather than fetched, so the app never calls out to a font service.

use std::sync::LazyLock;

use base64::Engine;

const TOKENS: &str = include_str!("tokens.css");
const RULES: &str = include_str!("style.css");

/// Family, weight and file of each typeface the design system uses.
const FONTS: [(&str, u16, &[u8]); 3] = [
    (
        "Fredoka",
        600,
        include_bytes!("../fonts/Fredoka-SemiBold.ttf"),
    ),
    (
        "Figtree",
        400,
        include_bytes!("../fonts/Figtree-Regular.ttf"),
    ),
    (
        "Figtree",
        600,
        include_bytes!("../fonts/Figtree-SemiBold.ttf"),
    ),
];

fn font_face((family, weight, file): (&str, u16, &[u8])) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(file);
    format!(
        "@font-face {{ font-family: \"{family}\"; font-weight: {weight}; \
         src: url(data:font/ttf;base64,{data}) format(\"truetype\"); }}\n"
    )
}

/// A `<style>` element for a window's head.
pub fn head() -> String {
    static HEAD: LazyLock<String> = LazyLock::new(|| {
        let fonts: String = FONTS.into_iter().map(font_face).collect();
        format!("<style>\n{fonts}{TOKENS}\n{RULES}</style>")
    });
    HEAD.clone()
}
