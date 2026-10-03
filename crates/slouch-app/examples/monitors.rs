//! Lists monitors as the app sees them, for checking how screens get named.
use dioxus::desktop::tao::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new();
    for monitor in event_loop.available_monitors() {
        println!(
            "{:?} at {:?} size {:?}",
            monitor.name(),
            monitor.position(),
            monitor.size()
        );
    }
}
