//! Naming the screens, so the calibration game can say where to look and find the webcam's screen.

use dioxus::desktop::tao::monitor::MonitorHandle;

/// Name each screen by where it sits, like "top-left screen", so the game can say where to look.
pub fn describe(monitors: &[MonitorHandle]) -> Vec<String> {
    let rects: Vec<(f64, f64, f64, f64)> = monitors
        .iter()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            (p.x as f64, p.y as f64, s.width as f64, s.height as f64)
        })
        .collect();
    let left = rects.iter().map(|r| r.0).fold(f64::MAX, f64::min);
    let right = rects.iter().map(|r| r.0 + r.2).fold(f64::MIN, f64::max);
    let top = rects.iter().map(|r| r.1).fold(f64::MAX, f64::min);
    let bottom = rects.iter().map(|r| r.1 + r.3).fold(f64::MIN, f64::max);
    let distinct = |values: Vec<f64>| values.iter().any(|v| *v != values[0]);
    let stacked = distinct(rects.iter().map(|r| r.1).collect());
    let side_by_side = distinct(rects.iter().map(|r| r.0).collect());
    rects
        .iter()
        .map(|&(x, y, w, h)| {
            let across = (x + w / 2.0 - left) / (right - left);
            let down = (y + h / 2.0 - top) / (bottom - top);
            let mut parts = Vec::new();
            if stacked {
                parts.push(if down < 0.5 { "top" } else { "bottom" });
            }
            if side_by_side {
                parts.push(if across < 1.0 / 3.0 {
                    "left"
                } else if across > 2.0 / 3.0 {
                    "right"
                } else {
                    "middle"
                });
            }
            format!("{} screen", parts.join("-"))
        })
        .collect()
}

/// A laptop's own panel, which is where its webcam sits, for steps that don't name a screen.
pub fn built_in(monitors: &[MonitorHandle]) -> Option<MonitorHandle> {
    let names = built_in_names();
    monitors
        .iter()
        .find(|m| {
            m.name().is_some_and(|name| {
                name.starts_with("Built-in") || names.iter().any(|n| n.eq_ignore_ascii_case(&name))
            })
        })
        .cloned()
}

/// macOS names its panel "Built-in …". GTK names monitors by model instead of connector, so on
/// Linux the model is read from the EDID of internal connectors: the panel's name if it has
/// one, otherwise its product code in hex, as Wayland compositors report it.
#[cfg(target_os = "linux")]
fn built_in_names() -> Vec<String> {
    let Ok(connectors) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    connectors
        .flatten()
        .filter(|c| {
            ["eDP", "LVDS", "DSI"]
                .iter()
                .any(|kind| c.file_name().to_string_lossy().contains(kind))
        })
        .filter_map(|c| std::fs::read(c.path().join("edid")).ok())
        .filter_map(|edid| edid_model(&edid))
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn built_in_names() -> Vec<String> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn edid_model(edid: &[u8]) -> Option<String> {
    if edid.len() < 128 {
        return None;
    }
    let product_name = edid[54..126].as_chunks::<18>().0.iter().find_map(|d| {
        (d[..3] == [0, 0, 0] && d[3] == 0xFC).then(|| {
            String::from_utf8_lossy(&d[5..])
                .split('\n')
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
    });
    product_name.or_else(|| {
        Some(format!(
            "0x{:04X}",
            u16::from_le_bytes([edid[10], edid[11]])
        ))
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn edid(descriptor: Option<(u8, &[u8])>) -> Vec<u8> {
        let mut edid = vec![0u8; 128];
        edid[10..12].copy_from_slice(&0x1515u16.to_le_bytes());
        if let Some((tag, text)) = descriptor {
            edid[57] = tag;
            edid[59..59 + text.len()].copy_from_slice(text);
        }
        edid
    }

    #[test]
    fn panel_named_by_product_code_without_a_name_descriptor() {
        assert_eq!(
            edid_model(&edid(Some((0xFE, b"LQ156N1\n")))).as_deref(),
            Some("0x1515")
        );
    }

    #[test]
    fn panel_named_by_its_name_descriptor() {
        assert_eq!(
            edid_model(&edid(Some((0xFC, b"Panel 15\n")))).as_deref(),
            Some("Panel 15")
        );
    }
}
