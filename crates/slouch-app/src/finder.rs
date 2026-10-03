//! Finding the face in each frame cheaply: search a square around where it was last seen,
//! and the whole frame only when that misses or every few seconds.
//!
//! The square is cut out at the frame's own scale and aligned to the detector's 32-pixel
//! grid, so the detector sees the same pixels a whole-frame search would. Resizing the square
//! instead shifted face size by up to 10%; this way it agrees to within a few percent.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use yunet::{Detector, Face, Landmarks};

use crate::frames::crop;

const MODEL: &[u8] = include_bytes!("../../../models/face_detection_yunet_2023mar.onnx");
const GRID: usize = 32;
/// How much wider than the face the square is, leaving room to move between frames.
const MARGIN: f32 = 1.6;
/// Look at the whole frame now and then, in case a nearer face has appeared elsewhere. Rarely,
/// because readings from the square and the whole frame differ by a percent or two.
const WHOLE_FRAME_EVERY: Duration = Duration::from_secs(30);

pub struct FaceFinder {
    /// One detector per input size, since a detector is built for one size.
    detectors: HashMap<(usize, usize), Detector>,
    last: Option<Face>,
    last_whole: Option<Instant>,
}

impl FaceFinder {
    pub fn new() -> Self {
        Self {
            detectors: HashMap::new(),
            last: None,
            last_whole: None,
        }
    }

    /// The largest face in tightly packed RGB, in its pixels.
    pub fn find(&mut self, rgb: &[u8], width: usize, height: usize) -> Option<Face> {
        let whole_due = self
            .last_whole
            .is_none_or(|at| at.elapsed() >= WHOLE_FRAME_EVERY);
        if !whole_due
            && let Some(last) = self.last
            && let Some(face) = self.near(&last, rgb, width, height)
        {
            self.last = Some(face);
            return Some(face);
        }
        self.last_whole = Some(Instant::now());
        self.last = largest(self.detector(width, height).detect(rgb, width, height));
        self.last
    }

    fn detector(&mut self, width: usize, height: usize) -> &Detector {
        self.detectors
            .entry((width, height))
            .or_insert_with(|| Detector::new(MODEL, width, height).expect("bundled model loads"))
    }

    fn near(&mut self, last: &Face, rgb: &[u8], width: usize, height: usize) -> Option<Face> {
        let side = ((last.width.max(last.height) * MARGIN) as usize).div_ceil(GRID) * GRID;
        if side * side * 2 > width * height {
            return None; // Barely cheaper than the whole frame.
        }
        let align = |centre: f32| (centre - side as f32 / 2.0).max(0.0) as usize / GRID * GRID;
        let left = align(last.x + last.width / 2.0);
        let top = align(last.y + last.height / 2.0);
        let square = crop(rgb, width, height, left, top, side);
        let face = largest(self.detector(side, side).detect(&square, side, side))?;
        let (dx, dy) = (left as f32, top as f32);
        let shift = |[x, y]: [f32; 2]| [x + dx, y + dy];
        let l = face.landmarks;
        Some(Face {
            x: face.x + dx,
            y: face.y + dy,
            landmarks: Landmarks {
                right_eye: shift(l.right_eye),
                left_eye: shift(l.left_eye),
                nose: shift(l.nose),
                right_mouth: shift(l.right_mouth),
                left_mouth: shift(l.left_mouth),
            },
            ..face
        })
    }
}

fn largest(faces: yunet::Result<Vec<Face>>) -> Option<Face> {
    match faces {
        Ok(faces) => faces
            .into_iter()
            .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height))),
        Err(error) => {
            tracing::warn!("detection failed: {error}");
            None
        }
    }
}
