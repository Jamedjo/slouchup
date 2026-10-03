//! Where frames come from: a webcam, or the drawn demo person.

use std::sync::{Arc, Mutex};

use dioxus_cameras::cameras::{self, PixelFormat, Resolution, StreamConfig};
use posture::Pose;

use crate::camera_view::FrameSlot;
use crate::demo::Demo;
use crate::frames::Picture;

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A camera by device id, or the first that can capture.
    Camera(Option<String>),
    Demo,
}

/// A camera that can capture, for choosing between them.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraInfo {
    pub id: String,
    pub name: String,
}

/// Cameras that can capture, in device order (on Linux, as OpenCV numbers them), without the
/// metadata nodes that some webcams also expose.
pub fn list_cameras() -> Vec<CameraInfo> {
    let mut devices = cameras::devices().unwrap_or_default();
    // Shorter ids first keeps /dev/video2 ahead of /dev/video10.
    devices.sort_by(|a, b| (a.id.0.len(), &a.id.0).cmp(&(b.id.0.len(), &b.id.0)));
    devices
        .into_iter()
        .filter(|d| cameras::probe(d).is_ok_and(|caps| !caps.formats.is_empty()))
        .map(|d| CameraInfo {
            id: d.id.0,
            name: d.name,
        })
        .collect()
}

/// A running source, holding its newest frame for the engine to take. Dropping it releases
/// the camera, so the camera's light goes off.
pub struct Capture {
    latest: Arc<Mutex<Option<Picture>>>,
    control: Option<Control>,
}

enum Control {
    Camera(cameras::pump::Pump),
    Demo(Demo),
}

impl Drop for Capture {
    fn drop(&mut self) {
        match self.control.take() {
            // Joining, so the device is free before anything opens it again.
            Some(Control::Camera(pump)) => cameras::pump::stop_and_join(pump),
            Some(Control::Demo(demo)) => demo.stop(),
            None => {}
        }
    }
}

impl Capture {
    pub fn start(source: Source, preview: FrameSlot) -> Result<Self, String> {
        let latest = Arc::new(Mutex::new(None));
        let sink = latest.clone();
        let control = match source {
            Source::Camera(id) => {
                let mut frames = 0u64;
                Control::Camera(cameras::pump::spawn(
                    open_camera(id.as_deref())?,
                    move |frame| {
                        frames += 1;
                        let picture = Picture::Camera(frame);
                        // The preview converts every frame it's given to RGBA; half the camera's rate is plenty.
                        if frames.is_multiple_of(2) {
                            preview.publish(picture.clone());
                        }
                        *sink.lock().unwrap() = Some(picture);
                    },
                ))
            }
            Source::Demo => Control::Demo(Demo::start(move |picture| {
                preview.publish(picture.clone());
                *sink.lock().unwrap() = Some(picture);
            })),
        };
        Ok(Self {
            latest,
            control: Some(control),
        })
    }

    pub fn take(&self) -> Option<Picture> {
        self.latest.lock().unwrap().take()
    }

    /// Have the demo person strike the pose the calibration game asks for; a real person reads
    /// the prompts instead.
    pub fn act(&self, pose: Option<Pose>) {
        if let Some(Control::Demo(demo)) = &self.control {
            demo.act(pose);
        }
    }
}

/// Open the camera with `id`, or the first one that works if it's not given or not plugged in.
/// The saved choice stays as it is, so a docked webcam is used again once it's back.
fn open_camera(id: Option<&str>) -> Result<cameras::Camera, String> {
    let usable = list_cameras();
    let present = |id: &str| usable.iter().any(|c| c.id == id);
    let wanted = match id {
        Some(id) if present(id) => id,
        _ => {
            if let Some(id) = id {
                tracing::warn!("camera {id} isn't connected; using the first that works");
            }
            usable
                .first()
                .map(|c| c.id.as_str())
                .ok_or("no camera found")?
        }
    };
    let device = cameras::devices()
        .map_err(|e| format!("listing cameras: {e}"))?
        .into_iter()
        .find(|d| d.id.0 == wanted)
        .ok_or_else(|| format!("camera {wanted} not found"))?;
    let config = choose_format(&device)?;
    let camera =
        cameras::open(&device, config).map_err(|e| format!("opening {}: {e}", device.name))?;
    tracing::info!("camera: {} ({})", device.name, device.id.0);
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
