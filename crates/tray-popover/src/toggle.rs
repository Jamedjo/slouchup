use std::time::{Duration, Instant};

/// How soon after the popover hid for losing focus a click on the tray icon counts as the click
/// that took the focus. Pressing the icon takes focus before the click arrives, so without this,
/// clicking the icon to close the popover would hide it and then show it again.
pub const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// How long after showing the popover a loss of focus takes the focus back rather than hiding
/// it. On Windows the taskbar finishes handling the click on the icon after the popover has
/// opened, and takes the focus back from it.
pub const SHOW_GRACE: Duration = Duration::from_millis(300);

/// What to do with the popover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Show,
    Hide,
    /// Give the popover the focus again, leaving it where it is.
    Refocus,
    Nothing,
}

/// Whether the popover is showing, and whether a click or a change of focus should change that.
#[derive(Clone, Debug, Default)]
pub struct Toggle {
    shown: bool,
    /// Whether it has had the focus since it was shown, so a window manager that never gives it
    /// the focus doesn't hide it straight away.
    focused: bool,
    shown_at: Option<Instant>,
    hid_for_focus: Option<Instant>,
}

impl Toggle {
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// A left click on the tray icon.
    pub fn click(&mut self, now: Instant) -> Step {
        if self.shown {
            return self.hide();
        }
        let just_hid = self
            .hid_for_focus
            .take()
            .is_some_and(|at| now.saturating_duration_since(at) < REOPEN_GUARD);
        if just_hid {
            Step::Nothing
        } else {
            self.show(now)
        }
    }

    /// Showing whatever its state, as for opening it from elsewhere than the tray.
    pub fn show(&mut self, now: Instant) -> Step {
        self.shown = true;
        self.focused = false;
        self.shown_at = Some(now);
        self.hid_for_focus = None;
        Step::Show
    }

    /// Hiding it, as for Esc or after choosing something in it.
    pub fn hide(&mut self) -> Step {
        if !self.shown {
            return Step::Nothing;
        }
        self.shown = false;
        Step::Hide
    }

    pub fn focus(&mut self, focused: bool, now: Instant) -> Step {
        if !self.shown {
            return Step::Nothing;
        }
        if focused {
            self.focused = true;
            return Step::Nothing;
        }
        let just_shown = self
            .shown_at
            .is_some_and(|at| now.saturating_duration_since(at) < SHOW_GRACE);
        if just_shown {
            return Step::Refocus;
        }
        if !self.focused {
            return Step::Nothing;
        }
        self.hid_for_focus = Some(now);
        self.hide()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOON: Duration = Duration::from_millis(50);
    const LATER: Duration = Duration::from_secs(2);

    #[test]
    fn clicks_toggle() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        assert_eq!(toggle.click(now), Step::Show);
        assert!(toggle.shown());
        assert_eq!(toggle.click(now + LATER), Step::Hide);
        assert!(!toggle.shown());
        assert_eq!(toggle.click(now + LATER * 2), Step::Show);
    }

    #[test]
    fn hides_when_it_loses_the_focus() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        toggle.click(now);
        assert_eq!(toggle.focus(true, now), Step::Nothing);
        assert_eq!(toggle.focus(false, now + LATER), Step::Hide);
        assert!(!toggle.shown());
    }

    #[test]
    fn a_click_on_the_icon_that_took_the_focus_closes_it() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        toggle.click(now);
        toggle.focus(true, now);
        let pressed = now + LATER;
        assert_eq!(toggle.focus(false, pressed), Step::Hide);
        assert_eq!(toggle.click(pressed + SOON), Step::Nothing);
        assert!(!toggle.shown());
        assert_eq!(toggle.click(pressed + SOON * 2), Step::Show);
    }

    #[test]
    fn a_click_long_after_losing_focus_opens_it() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        toggle.click(now);
        toggle.focus(true, now);
        toggle.focus(false, now + LATER);
        assert_eq!(toggle.click(now + LATER * 2), Step::Show);
    }

    #[test]
    fn losing_focus_it_never_had_leaves_it_showing() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        toggle.click(now);
        assert_eq!(toggle.focus(false, now + LATER), Step::Nothing);
        assert!(toggle.shown());
    }

    #[test]
    fn hiding_twice_does_nothing_the_second_time() {
        let mut toggle = Toggle::default();
        toggle.show(Instant::now());
        assert_eq!(toggle.hide(), Step::Hide);
        assert_eq!(toggle.hide(), Step::Nothing);
        assert_eq!(toggle.focus(false, Instant::now()), Step::Nothing);
    }

    #[test]
    fn showing_forgets_an_earlier_focus() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        toggle.click(now);
        toggle.focus(true, now);
        toggle.hide();
        toggle.show(Instant::now());
        assert_eq!(toggle.focus(false, now + LATER), Step::Nothing);
    }

    #[test]
    fn the_taskbar_taking_the_focus_back_just_after_opening_refocuses_it() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        assert_eq!(toggle.click(now), Step::Show);
        assert_eq!(toggle.focus(true, now + SOON / 5), Step::Nothing);
        assert_eq!(toggle.focus(false, now + SOON), Step::Refocus);
        assert!(toggle.shown());
        assert_eq!(toggle.focus(true, now + SOON * 2), Step::Nothing);
        assert_eq!(toggle.focus(false, now + LATER), Step::Hide);
    }

    #[test]
    fn losing_a_focus_never_given_just_after_opening_asks_for_it_again() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        toggle.click(now);
        assert_eq!(toggle.focus(false, now + SOON), Step::Refocus);
        assert!(toggle.shown());
    }

    #[test]
    fn open_close_open_by_the_icon_with_focus_events_between() {
        let now = Instant::now();
        let mut toggle = Toggle::default();
        // Open: the click, then the popover takes the focus.
        assert_eq!(toggle.click(now), Step::Show);
        toggle.focus(true, now + SOON);
        // Close by the icon: pressing it takes the focus, then the click arrives.
        let second = now + LATER;
        assert_eq!(toggle.focus(false, second), Step::Hide);
        assert_eq!(toggle.click(second + SOON), Step::Nothing);
        // Open again later, as many times as wanted.
        for round in 2..5 {
            let at = now + LATER * round;
            assert_eq!(toggle.click(at), Step::Show, "round {round}");
            toggle.focus(true, at + SOON);
            assert_eq!(
                toggle.focus(false, at + LATER / 2),
                Step::Hide,
                "round {round}"
            );
        }
    }
}
