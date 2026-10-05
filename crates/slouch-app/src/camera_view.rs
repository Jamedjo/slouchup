//! In-process camera preview: frames reach this app's webview through its own `dioxus://`
//! protocol, never through a socket that another program or a web page could read.
//!
//! The drawing side is a WebGL2 script adapted from `dioxus-cameras`, which polls a URL for a
//! small binary frame format; this module serves that format from an asset handler instead of
//! the crate's loopback HTTP server.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use dioxus::desktop::use_asset_handler;
use dioxus::desktop::wry::http::Response;
use dioxus::prelude::*;
use dioxus_cameras::cameras::{self, Frame, PixelFormat};

use crate::frames::Picture;

const HANDLER: &str = "camera";
const PREVIEW_JS: &str = include_str!("preview.js");
const MAGIC: &[u8; 4] = b"CAMS";
const VERSION: u8 = 1;
const FORMAT_NONE: u8 = 0;
const FORMAT_BGRA: u8 = 2;
const FORMAT_RGBA: u8 = 3;

/// A response body and the frame counter it was encoded at.
type Encoded = (u32, Vec<u8>);

/// The newest camera frame, shared between the capture thread and the preview.
#[derive(Clone, Default)]
pub struct FrameSlot {
    frame: Arc<Mutex<Option<Picture>>>,
    counter: Arc<AtomicU32>,
    /// The last encoded response, so polls between camera frames don't convert again.
    encoded: Arc<Mutex<Option<Encoded>>>,
}

impl PartialEq for FrameSlot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.frame, &other.frame)
    }
}

impl FrameSlot {
    pub fn publish(&self, frame: Picture) {
        *self.frame.lock().unwrap() = Some(frame);
        self.counter.fetch_add(1, Ordering::Release);
    }

    /// The newest frame, or just its header when the preview already has it: a whole frame is
    /// a megabyte or more, and the preview polls faster than cameras send frames.
    fn encode_after(&self, shown: Option<u32>) -> Vec<u8> {
        let counter = self.counter.load(Ordering::Acquire);
        if shown == Some(counter) {
            return header(FORMAT_NONE, 0, 0, 0, counter);
        }
        self.encode()
    }

    fn encode(&self) -> Vec<u8> {
        let counter = self.counter.load(Ordering::Acquire);
        let mut encoded = self.encoded.lock().unwrap();
        if let Some((at, body)) = encoded.as_ref()
            && *at == counter
        {
            return body.clone();
        }
        let frame = self.frame.lock().unwrap().clone();
        let body = match frame {
            Some(Picture::Camera(frame)) => encode_frame(&frame, counter),
            Some(Picture::Rgba {
                width,
                height,
                pixels,
            }) => {
                let mut body = header(FORMAT_RGBA, width, height, width * 4, counter);
                body.extend_from_slice(&pixels);
                body
            }
            None => header(FORMAT_NONE, 0, 0, 0, counter),
        };
        *encoded = Some((counter, body.clone()));
        body
    }
}

fn encode_frame(frame: &Frame, counter: u32) -> Vec<u8> {
    if frame.pixel_format == PixelFormat::Bgra8 {
        let stride = if frame.stride == 0 {
            frame.width * 4
        } else {
            frame.stride
        };
        let mut body = header(FORMAT_BGRA, frame.width, frame.height, stride, counter);
        body.extend_from_slice(&frame.plane_primary);
        return body;
    }
    match cameras::to_rgba8(frame) {
        Ok(rgba) => {
            let mut body = header(
                FORMAT_RGBA,
                frame.width,
                frame.height,
                frame.width * 4,
                counter,
            );
            body.extend_from_slice(&rgba);
            body
        }
        Err(_) => header(FORMAT_NONE, 0, 0, 0, counter),
    }
}

fn header(format: u8, width: u32, height: u32, stride: u32, counter: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(24);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&[VERSION, format, 0, 0]);
    for value in [width, height, stride, counter] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    header
}

/// A live view of `slot`, sized to fill its parent.
#[component]
pub fn CameraView(slot: FrameSlot) -> Element {
    use_asset_handler(HANDLER, move |request, responder| {
        let response = Response::builder()
            .header("Content-Type", "application/octet-stream")
            .header("Cache-Control", "no-store")
            .body(slot.encode_after(shown(request.uri().query())))
            .expect("static headers are valid");
        responder.respond(response);
    });
    rsx! {
        canvas {
            class: "cameras-preview-canvas",
            "data-stream-id": "0",
            "data-preview-url": "/{HANDLER}/0.bin",
        }
        script { dangerous_inner_html: "{PREVIEW_JS}" }
    }
}

/// The frame counter in a poll's `after=` query.
fn shown(query: Option<&str>) -> Option<u32> {
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("after="))?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_pictures_encode_with_the_preview_script_header() {
        let slot = FrameSlot::default();
        slot.publish(Picture::Rgba {
            width: 2,
            height: 1,
            pixels: Arc::new(vec![1, 2, 3, 4, 5, 6, 7, 8]),
        });
        let body = slot.encode();
        assert_eq!(&body[..4], b"CAMS");
        assert_eq!(body[5], FORMAT_RGBA);
        assert_eq!(u32::from_le_bytes(body[8..12].try_into().unwrap()), 2);
        assert_eq!(
            u32::from_le_bytes(body[20..24].try_into().unwrap()),
            1,
            "counter"
        );
        assert_eq!(&body[24..], &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn polls_for_a_frame_already_shown_get_only_a_header() {
        let slot = FrameSlot::default();
        slot.publish(Picture::Rgba {
            width: 1,
            height: 1,
            pixels: Arc::new(vec![1, 2, 3, 4]),
        });
        assert_eq!(slot.encode_after(shown(Some("after=1"))).len(), 24);
        assert_eq!(slot.encode_after(shown(Some("after=0"))).len(), 28);
        assert_eq!(slot.encode_after(shown(None)).len(), 28);
    }

    #[test]
    fn an_empty_slot_encodes_as_no_frame() {
        assert_eq!(FrameSlot::default().encode()[5], FORMAT_NONE);
    }
}
