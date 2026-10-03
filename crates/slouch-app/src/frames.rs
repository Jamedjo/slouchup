//! Camera frames shrunk to the half size detection runs at.

use std::sync::Arc;

use dioxus_cameras::cameras::{self, Frame, PixelFormat};

/// A frame from the webcam, or one the demo drew.
#[derive(Clone)]
pub enum Picture {
    Camera(Frame),
    /// Straight RGBA. The `cameras` crate's own frame type can't be built outside it.
    Rgba {
        width: u32,
        height: u32,
        pixels: Arc<Vec<u8>>,
    },
}

impl Picture {
    pub fn size(&self) -> (u32, u32) {
        match self {
            Picture::Camera(frame) => (frame.width, frame.height),
            Picture::Rgba { width, height, .. } => (*width, *height),
        }
    }
}

/// A frame at half width and height: packed RGB for the face detector and grey for tilt tracking.
pub struct Half {
    pub rgb: Vec<u8>,
    pub grey: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub fn half_size(picture: &Picture) -> Option<Half> {
    let (width, height) = picture.size();
    let (width, height) = (width as usize, height as usize);
    let rgb = match picture {
        Picture::Camera(frame) if frame.pixel_format == PixelFormat::Yuyv => {
            return yuyv_half(&frame.plane_primary, width, height, frame.stride as usize);
        }
        Picture::Camera(frame) => cameras::to_rgb8(frame).ok()?,
        Picture::Rgba { pixels, .. } => pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [r, g, b])
            .collect(),
    };
    // Decoders can disagree with the frame's stated size; better to skip a frame than panic.
    if rgb.len() != width * height * 3 {
        return None;
    }
    let rgb = halve(&rgb, width, height);
    let grey = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[r, g, b]| luma(r, g, b))
        .collect();
    Some(Half {
        rgb,
        grey,
        width: width / 2,
        height: height / 2,
    })
}

/// A `size` x `size` window of packed RGB starting at `left`, `top`. Parts beyond the image
/// are black, as the face detector pads a whole frame.
pub fn crop(
    rgb: &[u8],
    width: usize,
    height: usize,
    left: usize,
    top: usize,
    size: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; size * size * 3];
    let columns = size.min(width.saturating_sub(left));
    for row in 0..size.min(height.saturating_sub(top)) {
        let from = ((top + row) * width + left) * 3;
        out[row * size * 3..(row * size + columns) * 3]
            .copy_from_slice(&rgb[from..from + columns * 3]);
    }
    out
}

/// Each YUYV group of four bytes is two pixels sharing their colour, so taking one group per
/// output pixel from every other row halves the frame without converting the rest.
fn yuyv_half(data: &[u8], width: usize, height: usize, stride: usize) -> Option<Half> {
    let stride = if stride == 0 { width * 2 } else { stride };
    if stride < width * 2 || height == 0 || data.len() < (height - 1) * stride + width * 2 {
        return None;
    }
    let (w, h) = (width / 2, height / 2);
    let mut rgb = Vec::with_capacity(w * h * 3);
    let mut grey = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = &data[2 * y * stride..2 * y * stride + width * 2];
        for &[y0, u, y1, v] in row.as_chunks::<4>().0 {
            let luma = ((y0 as u16 + y1 as u16) / 2) as u8;
            rgb.extend_from_slice(&yuv_to_rgb(luma, u, v));
            grey.push(luma);
        }
    }
    Some(Half {
        rgb,
        grey,
        width: w,
        height: h,
    })
}

/// BT.601 limited range, matching the `cameras` crate's own conversion.
fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let (c, d, e) = (y as i32 - 16, u as i32 - 128, v as i32 - 128);
    let clamp = |x: i32| ((x + 128) >> 8).clamp(0, 255) as u8;
    [
        clamp(298 * c + 409 * e),
        clamp(298 * c - 100 * d - 208 * e),
        clamp(298 * c + 516 * d),
    ]
}

fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8
}

/// Average 2x2 blocks of packed RGB.
fn halve(rgb: &[u8], width: usize, height: usize) -> Vec<u8> {
    let (w, h) = (width / 2, height / 2);
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        let (top, bottom) = (2 * y * width * 3, (2 * y + 1) * width * 3);
        for x in 0..w {
            for c in 0..3 {
                let i = 2 * x * 3 + c;
                let sum = rgb[top + i] as u16
                    + rgb[top + i + 3] as u16
                    + rgb[bottom + i] as u16
                    + rgb[bottom + i + 3] as u16;
                out.push((sum / 4) as u8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smooth YUYV gradient, so skipping rows and averaging pixels agree closely.
    fn gradient(width: usize, height: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(width * height * 2);
        for y in 0..height {
            for x in (0..width).step_by(2) {
                let luma = (16 + (x + y) * 200 / (width + height)) as u8;
                data.extend_from_slice(&[luma, (100 + y / 8) as u8, luma, (150 - x / 8) as u8]);
            }
        }
        data
    }

    #[test]
    fn short_yuyv_buffers_are_skipped() {
        assert!(yuyv_half(&[0; 100], 64, 48, 0).is_none());
    }

    #[test]
    fn crop_copies_the_window_and_pads_past_the_edge() {
        let rgb: Vec<u8> = (1..=4 * 4 * 3).map(|i| i as u8).collect();
        let window = crop(&rgb, 4, 4, 3, 2, 2);
        let pixel = |x: usize, y: usize| &rgb[(y * 4 + x) * 3..(y * 4 + x) * 3 + 3];
        assert_eq!(&window[..3], pixel(3, 2));
        assert_eq!(&window[3..6], &[0, 0, 0], "past the right edge");
        assert_eq!(&window[6..9], pixel(3, 3));
    }

    #[test]
    fn yuyv_shortcut_matches_full_conversion() {
        let (width, height) = (64, 48);
        let data = gradient(width, height);
        let fast = yuyv_half(&data, width, height, 0).unwrap();

        let mut full = Vec::with_capacity(width * height * 3);
        for &[y0, u, y1, v] in data.as_chunks::<4>().0 {
            full.extend_from_slice(&yuv_to_rgb(y0, u, v));
            full.extend_from_slice(&yuv_to_rgb(y1, u, v));
        }
        let slow = halve(&full, width, height);

        assert_eq!((fast.width, fast.height), (width / 2, height / 2));
        let worst = fast
            .rgb
            .iter()
            .zip(&slow)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 6, "fast and full conversions differ by {worst}");
    }
}
