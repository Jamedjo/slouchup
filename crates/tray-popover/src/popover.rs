use std::time::Instant;

use crate::{Anchor, Monitor, Point, Size, Step, Toggle, place};

/// The window a popover shows in, as a windowing library offers it. An adapter implements this
/// for its own window type; [`Popover`] decides when and where to show it.
pub trait Surface {
    /// Every monitor, with its work area where the platform gives one.
    fn monitors(&self) -> Vec<Monitor>;
    /// The window's size, frame included, in physical pixels.
    fn size(&self) -> Size;
    /// Where the mouse cursor is, where the platform says.
    fn cursor(&self) -> Option<Point>;
    /// Move the window to `position`, show it above everything else and give it the focus.
    fn show_at(&mut self, position: Point);
    /// Give the window the focus again, where it is.
    fn focus(&mut self);
    fn hide(&mut self);
}

/// A popover by a tray icon: shown and hidden by clicks on the icon, hidden when it loses the
/// focus, and placed by the icon each time it opens.
pub struct Popover<S> {
    surface: S,
    toggle: Toggle,
}

impl<S: Surface> Popover<S> {
    pub fn new(surface: S) -> Self {
        Self {
            surface,
            toggle: Toggle::default(),
        }
    }

    pub fn surface(&self) -> &S {
        &self.surface
    }

    pub fn shown(&self) -> bool {
        self.toggle.shown()
    }

    /// A left click on the tray icon at `anchor`.
    pub fn click(&mut self, anchor: Anchor, now: Instant) {
        let step = self.toggle.click(now);
        self.apply(step, anchor);
    }

    /// Show it by `anchor`, whether or not it's showing.
    pub fn show(&mut self, anchor: Anchor, now: Instant) {
        let step = self.toggle.show(now);
        self.apply(step, anchor);
    }

    /// Hide it, as for Esc or after choosing something in it.
    pub fn hide(&mut self) {
        let step = self.toggle.hide();
        self.apply(step, Anchor::Unknown);
    }

    /// Hide the window unless the popover is showing, for a toolkit that shows a window on its
    /// own, as dioxus-desktop does once a window's page has loaded.
    pub fn settle(&mut self) {
        if !self.toggle.shown() {
            self.surface.hide();
        }
    }

    /// The window gained or lost the focus.
    pub fn focus(&mut self, focused: bool, now: Instant) {
        let step = self.toggle.focus(focused, now);
        self.apply(step, Anchor::Unknown);
    }

    fn apply(&mut self, step: Step, anchor: Anchor) {
        match step {
            Step::Show => {
                let surface = &self.surface;
                let placement = place(
                    anchor,
                    surface.size(),
                    &surface.monitors(),
                    surface.cursor(),
                );
                let position = placement.map_or(Point::default(), |p| p.position);
                self.surface.show_at(position);
            }
            Step::Hide => self.surface.hide(),
            Step::Refocus => self.surface.focus(),
            Step::Nothing => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::Rect;

    #[derive(Default)]
    struct Fake {
        shown_at: Option<Point>,
        cursor: Option<Point>,
        refocused: u32,
    }

    impl Surface for Fake {
        fn monitors(&self) -> Vec<Monitor> {
            vec![Monitor {
                bounds: Rect::new(0, 0, 1920, 1080),
                work_area: Rect::new(0, 0, 1920, 1032),
                scale: 1.0,
            }]
        }

        fn size(&self) -> Size {
            Size::new(300, 280)
        }

        fn cursor(&self) -> Option<Point> {
            self.cursor
        }

        fn show_at(&mut self, position: Point) {
            self.shown_at = Some(position);
        }

        fn hide(&mut self) {
            self.shown_at = None;
        }

        fn focus(&mut self) {
            self.refocused += 1;
        }
    }

    #[test]
    fn a_click_shows_it_by_the_icon_and_another_hides_it() {
        let now = Instant::now();
        let mut popover = Popover::new(Fake::default());
        let icon = Anchor::Icon(Rect::new(1700, 1040, 32, 32));
        popover.click(icon, now);
        assert_eq!(popover.surface().shown_at, Some(Point::new(1566, 744)));
        popover.click(icon, now + Duration::from_secs(1));
        assert_eq!(popover.surface().shown_at, None);
        assert!(!popover.shown());
    }

    #[test]
    fn losing_focus_hides_it() {
        let now = Instant::now();
        let mut popover = Popover::new(Fake::default());
        popover.show(Anchor::Point(Point::new(1700, 1050)), now);
        popover.focus(true, now);
        popover.focus(false, now + Duration::from_secs(1));
        assert_eq!(popover.surface().shown_at, None);
    }

    #[test]
    fn a_focus_loss_straight_after_opening_refocuses_rather_than_hides() {
        let now = Instant::now();
        let mut popover = Popover::new(Fake::default());
        popover.click(Anchor::Point(Point::new(1700, 1050)), now);
        popover.focus(true, now);
        popover.focus(false, now + Duration::from_millis(40));
        assert!(popover.shown());
        assert!(popover.surface().shown_at.is_some());
        assert_eq!(popover.surface().refocused, 1);
    }

    #[test]
    fn settling_hides_only_a_popover_not_meant_to_show() {
        let mut popover = Popover::new(Fake {
            shown_at: Some(Point::new(5, 5)),
            ..Fake::default()
        });
        popover.settle();
        assert_eq!(popover.surface().shown_at, None);

        popover.click(Anchor::Point(Point::new(1700, 1050)), Instant::now());
        let shown = popover.surface().shown_at;
        popover.settle();
        assert_eq!(popover.surface().shown_at, shown);
        assert!(shown.is_some());
    }

    #[test]
    fn an_unknown_anchor_opens_by_the_cursor() {
        let mut popover = Popover::new(Fake {
            cursor: Some(Point::new(1000, 1050)),
            ..Fake::default()
        });
        popover.click(Anchor::Unknown, Instant::now());
        assert_eq!(popover.surface().shown_at, Some(Point::new(850, 744)));
    }
}
