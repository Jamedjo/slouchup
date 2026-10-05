//! A popover window by a tray icon: where it goes, and when it shows or hides.
//!
//! Nothing here opens a window. A windowing adapter implements [`Surface`] for its own window
//! type, and [`Popover`] drives it: [`place`] works out the position against the monitors' work
//! areas and the panel's edge, and [`Toggle`] decides whether a click on the tray icon shows or
//! hides it. Linux panels report clicks in different units; [`linux::Host`] says which.
//!
//! Every position is in physical pixels, with the origin at the top left of the primary monitor,
//! unless a type says otherwise.

mod geometry;
pub mod linux;
mod place;
mod popover;
mod toggle;

pub use geometry::{Point, Rect, Size};
pub use place::{Anchor, Edge, GAP, Monitor, Placement, place};
pub use popover::{Popover, Surface};
pub use toggle::{REOPEN_GUARD, SHOW_GRACE, Step, Toggle};
