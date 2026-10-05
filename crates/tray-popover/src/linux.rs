//! Linux panels show tray icons as StatusNotifierItems, and tell the app about a left click with
//! `Activate(x, y)`. What `x` and `y` mean is up to the panel: device pixels, logical pixels, a
//! constant 0,0, or no call at all. These are the panels as found on X11; on Wayland a client
//! can't place its own window, so the numbers don't matter there.

use crate::{Anchor, Point};

/// The panel showing the tray icon, as far as the session says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// Plasma, with its version when it's known.
    Kde {
        plasma: Option<(u32, u32)>,
    },
    /// GNOME's AppIndicator extension.
    Gnome,
    Xfce,
    Waybar,
    Swaybar,
    Budgie,
    Lxqt,
    Cinnamon,
    /// snixembed, which shows StatusNotifierItems in an older XEmbed tray such as i3bar's.
    Snixembed,
    /// Ayatana's indicator service, whose panels open the menu and never activate.
    Ayatana,
    Unknown,
}

/// What a host's `Activate(x, y)` is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    /// Device pixels.
    Physical,
    /// Logical pixels, which need the monitor's scale.
    Logical,
    /// Always 0,0.
    Origin,
    /// The host never activates the icon, so a click never arrives.
    NoClicks,
    /// Not known: the position isn't trusted.
    Unknown,
}

impl Host {
    pub fn units(self) -> Units {
        match self {
            // Plasma 5.27 switched Activate to device pixels.
            Host::Kde {
                plasma: Some(version),
            } if version <= (5, 26) => Units::Logical,
            Host::Kde { .. } | Host::Gnome => Units::Physical,
            Host::Xfce
            | Host::Waybar
            | Host::Swaybar
            | Host::Budgie
            | Host::Lxqt
            | Host::Cinnamon => Units::Logical,
            Host::Snixembed => Units::Origin,
            Host::Ayatana => Units::NoClicks,
            Host::Unknown => Units::Unknown,
        }
    }

    /// Whether a click on the icon ever reaches the app.
    pub fn clicks(self) -> bool {
        self.units() != Units::NoClicks
    }

    /// The anchor for `Activate(x, y)` from this host.
    pub fn anchor(self, x: i32, y: i32) -> Anchor {
        match self.units() {
            _ if (x, y) == (0, 0) => Anchor::Unknown,
            Units::Physical => Anchor::Point(Point::new(x, y)),
            Units::Logical => Anchor::Logical {
                x: x as f64,
                y: y as f64,
            },
            Units::Origin | Units::NoClicks | Units::Unknown => Anchor::Unknown,
        }
    }

    /// The host for a session, from its environment through `var`, and Plasma's version when
    /// the caller has asked `plasmashell --version` for it.
    pub fn detect(var: impl Fn(&str) -> Option<String>, plasma: Option<(u32, u32)>) -> Host {
        let desktop = var("XDG_CURRENT_DESKTOP")
            .or_else(|| var("XDG_SESSION_DESKTOP"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let names: Vec<&str> = desktop.split(':').collect();
        let any = |wanted: &[&str]| names.iter().any(|name| wanted.contains(name));
        // Some desktops name GNOME too, as in "Budgie:GNOME", so it's checked for last.
        if any(&["kde"]) {
            Host::Kde { plasma }
        } else if any(&["xfce"]) {
            Host::Xfce
        } else if any(&["budgie", "budgie-desktop"]) {
            Host::Budgie
        } else if any(&["lxqt"]) {
            Host::Lxqt
        } else if any(&["x-cinnamon", "cinnamon"]) {
            Host::Cinnamon
        } else if any(&["gnome", "ubuntu", "pop"]) {
            Host::Gnome
        } else if any(&["sway"]) {
            Host::Swaybar
        } else if any(&["hyprland", "niri", "river", "wayfire", "labwc"]) {
            Host::Waybar
        } else if any(&["i3"]) {
            Host::Snixembed
        } else if any(&["unity"]) {
            Host::Ayatana
        } else {
            Host::Unknown
        }
    }
}

/// Plasma's version from `plasmashell --version`, which prints `plasmashell 5.27.11`.
pub fn parse_plasma_version(output: &str) -> Option<(u32, u32)> {
    let version = output.split_whitespace().last()?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(desktop: &str) -> Host {
        Host::detect(
            |name| (name == "XDG_CURRENT_DESKTOP").then(|| desktop.to_string()),
            None,
        )
    }

    #[test]
    fn hosts_from_the_desktop_name() {
        assert_eq!(session("KDE"), Host::Kde { plasma: None });
        assert_eq!(session("ubuntu:GNOME"), Host::Gnome);
        assert_eq!(session("XFCE"), Host::Xfce);
        assert_eq!(session("Budgie:GNOME"), Host::Budgie);
        assert_eq!(session("LXQt"), Host::Lxqt);
        assert_eq!(session("X-Cinnamon"), Host::Cinnamon);
        assert_eq!(session("sway"), Host::Swaybar);
        assert_eq!(session("Hyprland"), Host::Waybar);
        assert_eq!(session("i3"), Host::Snixembed);
        assert_eq!(session("Unity"), Host::Ayatana);
        assert_eq!(session(""), Host::Unknown);
    }

    #[test]
    fn the_session_desktop_stands_in_for_a_missing_current_desktop() {
        let host = Host::detect(
            |name| (name == "XDG_SESSION_DESKTOP").then(|| "xfce".to_string()),
            None,
        );
        assert_eq!(host, Host::Xfce);
    }

    #[test]
    fn units_per_host() {
        assert_eq!(Host::Kde { plasma: None }.units(), Units::Physical);
        assert_eq!(
            Host::Kde {
                plasma: Some((5, 27))
            }
            .units(),
            Units::Physical
        );
        assert_eq!(
            Host::Kde {
                plasma: Some((5, 26))
            }
            .units(),
            Units::Logical
        );
        assert_eq!(
            Host::Kde {
                plasma: Some((6, 0))
            }
            .units(),
            Units::Physical
        );
        assert_eq!(Host::Gnome.units(), Units::Physical);
        for logical in [
            Host::Xfce,
            Host::Waybar,
            Host::Swaybar,
            Host::Budgie,
            Host::Lxqt,
            Host::Cinnamon,
        ] {
            assert_eq!(logical.units(), Units::Logical, "{logical:?}");
        }
        assert_eq!(Host::Snixembed.units(), Units::Origin);
        assert!(!Host::Ayatana.clicks());
        assert!(Host::Snixembed.clicks());
    }

    #[test]
    fn anchors_per_host() {
        assert_eq!(
            Host::Gnome.anchor(3000, 20),
            Anchor::Point(Point::new(3000, 20))
        );
        assert_eq!(
            Host::Xfce.anchor(1500, 10),
            Anchor::Logical { x: 1500.0, y: 10.0 }
        );
        assert_eq!(Host::Snixembed.anchor(0, 0), Anchor::Unknown);
        assert_eq!(Host::Unknown.anchor(400, 10), Anchor::Unknown);
        assert_eq!(Host::Gnome.anchor(0, 0), Anchor::Unknown);
    }

    #[test]
    fn plasma_versions() {
        assert_eq!(parse_plasma_version("plasmashell 5.27.11\n"), Some((5, 27)));
        assert_eq!(parse_plasma_version("plasmashell 6.1.4"), Some((6, 1)));
        assert_eq!(parse_plasma_version("nonsense"), None);
    }
}
