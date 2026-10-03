//! Lists cameras and the formats they offer, for diagnosing capture problems.
use dioxus_cameras::cameras;

fn main() {
    for device in cameras::devices().expect("listing cameras") {
        println!("{:?} {:?}", device.id, device.name);
        match cameras::probe(&device) {
            Ok(caps) => {
                for f in caps.formats.iter().filter(|f| f.resolution.width <= 640) {
                    println!(
                        "  {:?} {}x{} {:?}",
                        f.pixel_format, f.resolution.width, f.resolution.height, f.framerate_range
                    );
                }
            }
            Err(e) => println!("  probe failed: {e}"),
        }
    }
}
