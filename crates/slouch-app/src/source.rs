//! Where frames come from: a webcam, or the drawn demo person.

use std::sync::{Arc, Mutex};

use dioxus_cameras::cameras::{self, PixelFormat, Resolution, StreamConfig};
use posture::Pose;

use crate::camera_view::FrameSlot;
use crate::demo::Demo;
use crate::frames::Picture;

pub enum Source {
    /// The nth camera that can capture, numbered as OpenCV numbers them.
    Camera(usize),
    Demo,
}

/// A running source, holding its newest frame for the engine to take.
pub struct Capture {
    latest: Arc<Mutex<Option<Picture>>>,
    control: Control,
}

enum Control {
    Camera(cameras::pump::Pump),
    Demo(Demo),
}

impl Capture {
    pub fn start(source: Source, preview: FrameSlot) -> Result<Self, String> {
        let latest = Arc::new(Mutex::new(None));
        let sink = latest.clone();
        let control = match source {
            Source::Camera(index) => {
                let mut frames = 0u64;
                Control::Camera(cameras::pump::spawn(open_camera(index)?, move |frame| {
                    frames += 1;
                    let picture = Picture::Camera(frame);
                    // The preview converts every frame it's given to RGBA; half the camera's rate is plenty.
                    if frames.is_multiple_of(2) {
                        preview.publish(picture.clone());
                    }
                    *sink.lock().unwrap() = Some(picture);
                }))
            }
            Source::Demo => Control::Demo(Demo::start(move |picture| {
                preview.publish(picture.clone());
                *sink.lock().unwrap() = Some(picture);
            })),
        };
        Ok(Self { latest, control })
    }

    pub fn take(&self) -> Option<Picture> {
        self.latest.lock().unwrap().take()
    }

    pub fn set_active(&self, active: bool) {
        match &self.control {
            Control::Camera(pump) => cameras::pump::set_active(pump, active),
            Control::Demo(demo) => demo.set_active(active),
        }
    }

    /// Have the demo person strike the pose the calibration game asks for; a real person reads
    /// the prompts instead.
    pub fn act(&self, pose: Option<Pose>) {
        if let Control::Demo(demo) = &self.control {
            demo.act(pose);
        }
    }
}

fn open_camera(index: usize) -> Result<cameras::Camera, String> {
    let mut devices = cameras::devices().map_err(|e| format!("listing cameras: {e}"))?;
    // Number cameras like OpenCV does: by device path, skipping metadata nodes that can't capture.
    devices.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    let device = devices
        .into_iter()
        .filter(|d| cameras::probe(d).is_ok_and(|caps| !caps.formats.is_empty()))
        .nth(index)
        .ok_or_else(|| format!("no camera {index}"))?;
    let config = choose_format(&device)?;
    let camera =
        cameras::open(&device, config).map_err(|e| format!("opening {}: {e}", device.name))?;
    tracing::info!("camera: {}", device.name);
    Ok(camera)
}

/// 640x480 in a format that is cheap to read, or failing that the nearest size on offer.
fn choose_format(device: &cameras::Device) -> Result<StreamConfig, String> {
    let wanted = Resolution {
        width: 640,
        height: 480,
    };
    let capabilities =
        cameras::probe(device).map_err(|e| format!("probing {}: {e}", device.name))?;
    let cheapest_first = [
        PixelFormat::Yuyv,
        PixelFormat::Nv12,
        PixelFormat::Bgra8,
        PixelFormat::Rgb8,
        PixelFormat::Mjpeg,
    ];
    let format = cheapest_first
        .into_iter()
        .find_map(|pixel_format| {
            capabilities
                .formats
                .iter()
                .find(|f| f.pixel_format == pixel_format && f.resolution == wanted)
                .cloned()
        })
        .or_else(|| {
            let ideal = StreamConfig {
                resolution: wanted,
                framerate: 30,
                pixel_format: PixelFormat::Yuyv,
            };
            cameras::best_format(&capabilities, &ideal)
        })
        .ok_or_else(|| format!("{} offers no video formats", device.name))?;
    let range = format.framerate_range;
    Ok(StreamConfig {
        resolution: format.resolution,
        framerate: 30f64.clamp(range.min, range.max).round() as u32,
        pixel_format: format.pixel_format,
    })
}
