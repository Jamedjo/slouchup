use crate::{Point, Rect, Size};

/// Space between the popover and the tray icon, and between the popover and the work area's
/// edges, in logical pixels.
pub const GAP: f64 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub bounds: Rect,
    /// The bounds less the taskbar, menu bar, dock and panels. The same as `bounds` where the
    /// platform doesn't say, or the taskbar hides itself.
    pub work_area: Rect,
    /// Physical pixels per logical pixel.
    pub scale: f64,
}

impl Monitor {
    fn logical_bounds(&self) -> (f64, f64, f64, f64) {
        let b = self.bounds;
        let s = self.scale;
        (
            b.x as f64 / s,
            b.y as f64 / s,
            b.right() as f64 / s,
            b.bottom() as f64 / s,
        )
    }

    fn logical_to_physical(&self, x: f64, y: f64) -> Point {
        let (left, top, _, _) = self.logical_bounds();
        Point::new(
            self.bounds.x + ((x - left) * self.scale).round() as i32,
            self.bounds.y + ((y - top) * self.scale).round() as i32,
        )
    }

    fn physical(&self, logical: f64) -> i32 {
        (logical * self.scale).round() as i32
    }
}

/// Which side of the screen the taskbar or panel holding the tray icon is on. The popover opens
/// away from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// What's known of where the tray icon is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// The icon's bounds, as Windows and macOS give them.
    Icon(Rect),
    /// A point on the icon, as Linux panels give one when they're clicked.
    Point(Point),
    /// A point in logical pixels, as some Linux panels give it. It's made physical with the
    /// scale of the monitor it falls on.
    Logical { x: f64, y: f64 },
    /// Nothing useful: the popover goes by the cursor, if that's known.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Where the popover's top-left corner goes.
    pub position: Point,
    /// The panel edge it opened away from.
    pub edge: Edge,
    /// Which of the monitors it's on.
    pub monitor: usize,
}

/// Where a popover of `size` goes for `anchor`: centred on the icon, just off the panel, and
/// moved fully inside the monitor's work area. `cursor` stands in for an unknown anchor.
///
/// With no monitors, `None`. With no anchor or cursor, the top-right corner of the first
/// monitor's work area, where most Linux panels keep their tray.
pub fn place(
    anchor: Anchor,
    size: Size,
    monitors: &[Monitor],
    cursor: Option<Point>,
) -> Option<Placement> {
    let first = monitors.first()?;
    let icon = match anchor {
        Anchor::Icon(rect) => Some(rect),
        Anchor::Point(point) => Some(Rect::at(point)),
        Anchor::Logical { x, y } => Some(Rect::at(logical_to_physical(x, y, monitors))),
        Anchor::Unknown => cursor.map(Rect::at),
    };
    let Some(icon) = icon else {
        let gap = first.physical(GAP);
        let area = first.work_area;
        let position = Point::new(area.right() - gap - size.width, area.y + gap);
        return Some(Placement {
            position: clamp_into(position, size, area, gap),
            edge: Edge::Top,
            monitor: 0,
        });
    };
    let index = monitor_at(icon.centre(), monitors);
    let monitor = &monitors[index];
    let edge = edge_of(monitor, icon.centre());
    let gap = monitor.physical(GAP);
    let centre = icon.centre();
    let position = match edge {
        Edge::Top => Point::new(centre.x - size.width / 2, icon.bottom() + gap),
        Edge::Bottom => Point::new(centre.x - size.width / 2, icon.y - gap - size.height),
        Edge::Left => Point::new(icon.right() + gap, centre.y - size.height / 2),
        Edge::Right => Point::new(icon.x - gap - size.width, centre.y - size.height / 2),
    };
    Some(Placement {
        position: clamp_into(position, size, monitor.work_area, gap),
        edge,
        monitor: index,
    })
}

/// The monitor holding `point`, or else the nearest.
fn monitor_at(point: Point, monitors: &[Monitor]) -> usize {
    monitors
        .iter()
        .position(|m| m.bounds.contains(point))
        .or_else(|| {
            monitors
                .iter()
                .enumerate()
                .min_by_key(|(_, m)| m.bounds.distance_squared(point))
                .map(|(index, _)| index)
        })
        .unwrap_or(0)
}

/// The panel's edge: the strip outside the work area that `point` is in, or where the work area
/// covers the whole monitor, as with a taskbar that hides itself, the nearest edge.
fn edge_of(monitor: &Monitor, point: Point) -> Edge {
    let work = monitor.work_area;
    if point.y < work.y {
        return Edge::Top;
    }
    if point.y >= work.bottom() {
        return Edge::Bottom;
    }
    if point.x < work.x {
        return Edge::Left;
    }
    if point.x >= work.right() {
        return Edge::Right;
    }
    let b = monitor.bounds;
    [
        (Edge::Top, point.y - b.y),
        (Edge::Bottom, b.bottom() - 1 - point.y),
        (Edge::Left, point.x - b.x),
        (Edge::Right, b.right() - 1 - point.x),
    ]
    .into_iter()
    .min_by_key(|(_, distance)| *distance)
    .map(|(edge, _)| edge)
    .expect("four edges")
}

/// `position` moved so `size` fits inside `area`, `margin` in from its edges. Too big to fit,
/// it's aligned with the area's top or left.
fn clamp_into(position: Point, size: Size, area: Rect, margin: i32) -> Point {
    let fit = |at: i32, length: i32, start: i32, span: i32| {
        let low = start + margin;
        let high = start + span - margin - length;
        if high < low {
            start
        } else {
            at.clamp(low, high)
        }
    };
    Point::new(
        fit(position.x, size.width, area.x, area.width),
        fit(position.y, size.height, area.y, area.height),
    )
}

/// A point in logical pixels, made physical with the scale of the monitor it falls on, or the
/// first monitor's when it falls on none.
fn logical_to_physical(x: f64, y: f64, monitors: &[Monitor]) -> Point {
    let on = monitors.iter().find(|m| {
        let (left, top, right, bottom) = m.logical_bounds();
        x >= left && x < right && y >= top && y < bottom
    });
    match on {
        Some(monitor) => monitor.logical_to_physical(x, y),
        None => {
            let scale = monitors.first().map_or(1.0, |m| m.scale);
            Point::new((x * scale).round() as i32, (y * scale).round() as i32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POPOVER: Size = Size::new(320, 300);

    fn monitor(bounds: Rect, work_area: Rect, scale: f64) -> Monitor {
        Monitor {
            bounds,
            work_area,
            scale,
        }
    }

    /// 1920x1080 with a 48px taskbar along the bottom, as Windows 11 has it.
    fn windows_laptop() -> Monitor {
        monitor(
            Rect::new(0, 0, 1920, 1080),
            Rect::new(0, 0, 1920, 1032),
            1.0,
        )
    }

    /// A 2x Retina screen with a 24pt menu bar along the top.
    fn retina() -> Monitor {
        monitor(
            Rect::new(0, 0, 2880, 1800),
            Rect::new(0, 48, 2880, 1752),
            2.0,
        )
    }

    fn placed(anchor: Anchor, monitors: &[Monitor]) -> Placement {
        place(anchor, POPOVER, monitors, None).unwrap()
    }

    #[test]
    fn opens_above_a_bottom_taskbar_centred_on_the_icon() {
        let icon = Rect::new(1500, 1040, 32, 32);
        let placement = placed(Anchor::Icon(icon), &[windows_laptop()]);
        assert_eq!(placement.edge, Edge::Bottom);
        assert_eq!(placement.position, Point::new(1516 - 160, 1032 - 8 - 300));
    }

    #[test]
    fn opens_below_the_macos_menu_bar_with_a_scaled_gap() {
        let icon = Rect::new(2000, 0, 44, 48);
        let placement = placed(Anchor::Icon(icon), &[retina()]);
        assert_eq!(placement.edge, Edge::Top);
        assert_eq!(placement.position, Point::new(2022 - 160, 48 + 16));
    }

    #[test]
    fn kept_inside_the_work_area_at_the_screen_corner() {
        let icon = Rect::new(1880, 1040, 32, 32);
        let placement = placed(Anchor::Icon(icon), &[windows_laptop()]);
        assert_eq!(
            placement.position,
            Point::new(1920 - 8 - 320, 1032 - 8 - 300)
        );

        let icon = Rect::new(2840, 0, 40, 48);
        let placement = placed(Anchor::Icon(icon), &[retina()]);
        assert_eq!(placement.position.x, 2880 - 16 - 320);
    }

    #[test]
    fn opens_beside_side_taskbars() {
        let left = monitor(
            Rect::new(0, 0, 1920, 1080),
            Rect::new(62, 0, 1858, 1080),
            1.0,
        );
        let placement = placed(Anchor::Icon(Rect::new(15, 900, 32, 32)), &[left]);
        assert_eq!(placement.edge, Edge::Left);
        assert_eq!(placement.position, Point::new(62 + 8, 916 - 150));

        let right = monitor(
            Rect::new(0, 0, 1920, 1080),
            Rect::new(0, 0, 1858, 1080),
            1.0,
        );
        let placement = placed(Anchor::Icon(Rect::new(1873, 1040, 32, 32)), &[right]);
        assert_eq!(placement.edge, Edge::Right);
        assert_eq!(
            placement.position,
            Point::new(1858 - 8 - 320, 1080 - 8 - 300)
        );
    }

    #[test]
    fn a_hidden_taskbar_goes_by_the_nearest_edge() {
        let whole = Rect::new(0, 0, 1920, 1080);
        let auto_hide = monitor(whole, whole, 1.0);
        let placement = placed(Anchor::Icon(Rect::new(1500, 1050, 30, 30)), &[auto_hide]);
        assert_eq!(placement.edge, Edge::Bottom);
        assert_eq!(placement.position.y, 1050 - 8 - 300);

        let placement = placed(Anchor::Point(Point::new(1700, 10)), &[auto_hide]);
        assert_eq!(placement.edge, Edge::Top);
        assert_eq!(placement.position, Point::new(1540, 18));
    }

    #[test]
    fn found_on_the_monitor_holding_the_icon() {
        let left = monitor(
            Rect::new(-2560, 0, 2560, 1440),
            Rect::new(-2560, 0, 2560, 1400),
            1.0,
        );
        let placement = placed(
            Anchor::Icon(Rect::new(-300, 1405, 32, 32)),
            &[windows_laptop(), left],
        );
        assert_eq!(placement.monitor, 1);
        assert_eq!(placement.edge, Edge::Bottom);
        assert_eq!(placement.position, Point::new(-284 - 160, 1400 - 8 - 300));
    }

    #[test]
    fn an_icon_off_every_screen_uses_the_nearest() {
        let placement = placed(
            Anchor::Point(Point::new(3000, 100)),
            &[windows_laptop(), retina()],
        );
        assert_eq!(placement.monitor, 1);
    }

    #[test]
    fn logical_points_scale_by_their_monitor() {
        let hidpi = monitor(
            Rect::new(0, 0, 3840, 2160),
            Rect::new(0, 64, 3840, 2096),
            2.0,
        );
        let placement = placed(Anchor::Logical { x: 1800.0, y: 16.0 }, &[hidpi]);
        assert_eq!(placement.edge, Edge::Top);
        assert_eq!(placement.position, Point::new(3600 - 160, 64 + 16));
    }

    #[test]
    fn logical_points_use_the_scale_of_the_monitor_they_fall_on() {
        let left = monitor(
            Rect::new(0, 0, 1920, 1080),
            Rect::new(0, 0, 1920, 1080),
            1.0,
        );
        let right = monitor(
            Rect::new(1920, 0, 1920, 1080),
            Rect::new(1920, 0, 1920, 1080),
            2.0,
        );
        assert_eq!(
            logical_to_physical(1000.0, 10.0, &[left, right]),
            Point::new(1000, 10)
        );
        assert_eq!(
            logical_to_physical(970.0, 10.0, &[right]),
            Point::new(1940, 20)
        );
        assert_eq!(
            logical_to_physical(5000.0, 10.0, &[right]),
            Point::new(10000, 20)
        );
    }

    #[test]
    fn an_unknown_anchor_goes_by_the_cursor() {
        let placement = place(
            Anchor::Unknown,
            POPOVER,
            &[windows_laptop()],
            Some(Point::new(1600, 1060)),
        )
        .unwrap();
        assert_eq!(placement.edge, Edge::Bottom);
        assert_eq!(placement.position, Point::new(1440, 1032 - 8 - 300));
    }

    #[test]
    fn with_nothing_to_go_by_it_opens_top_right() {
        let placement = place(Anchor::Unknown, POPOVER, &[windows_laptop()], None).unwrap();
        assert_eq!(placement.position, Point::new(1920 - 8 - 320, 8));
        assert_eq!(place(Anchor::Unknown, POPOVER, &[], None), None);
    }

    #[test]
    fn too_big_to_fit_aligns_with_the_top_left() {
        let small = monitor(Rect::new(0, 0, 300, 200), Rect::new(0, 0, 300, 200), 1.0);
        let placement = placed(Anchor::Point(Point::new(150, 199)), &[small]);
        assert_eq!(placement.position, Point::new(0, 0));
    }
}
