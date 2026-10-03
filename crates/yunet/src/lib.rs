//! YuNet face detection with five facial landmarks, running on [tract] in pure Rust.
//!
//! Works with the `face_detection_yunet_2023mar.onnx` model from the OpenCV model zoo. That
//! model is exported for a fixed 640x640 input, but nothing in it depends on the size, so the
//! detector forgets the exported shapes and can run at any size divisible by 32. Smaller inputs
//! are much faster; a face filling a webcam frame is still found reliably at 320x256.

use tract_onnx::prelude::*;

/// The largest feature-map stride; input sides must be multiples of it.
const MAX_STRIDE: usize = 32;
const STRIDES: [usize; 3] = [8, 16, 32];

/// A detected face, in pixels of the image passed to [`Detector::detect`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub landmarks: Landmarks,
    pub score: f32,
}

/// The five points YuNet places, each as `[x, y]`. Left and right are the person's own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landmarks {
    pub right_eye: [f32; 2],
    pub left_eye: [f32; 2],
    pub nose: [f32; 2],
    pub right_mouth: [f32; 2],
    pub left_mouth: [f32; 2],
}

impl Landmarks {
    pub fn points(&self) -> [[f32; 2]; 5] {
        [
            self.right_eye,
            self.left_eye,
            self.nose,
            self.right_mouth,
            self.left_mouth,
        ]
    }
}

pub struct Detector {
    model: std::sync::Arc<TypedRunnableModel>,
    input_width: usize,
    input_height: usize,
    pub score_threshold: f32,
    pub nms_threshold: f32,
}

impl Detector {
    /// Load the model for images up to `width` x `height`; each side is rounded up to a multiple of 32.
    pub fn new(model: &[u8], width: usize, height: usize) -> TractResult<Self> {
        let input_width = width.div_ceil(MAX_STRIDE) * MAX_STRIDE;
        let input_height = height.div_ceil(MAX_STRIDE) * MAX_STRIDE;
        let mut graph = tract_onnx::onnx().model_for_read(&mut &*model)?;
        forget_exported_shapes(&mut graph)?;
        graph.set_input_fact(0, f32::fact([1, 3, input_height, input_width]).into())?;
        let model = graph.into_optimized()?.into_runnable()?;
        Ok(Self {
            model,
            input_width,
            input_height,
            score_threshold: 0.7,
            nms_threshold: 0.3,
        })
    }

    /// Find faces in a tightly packed RGB image no larger than the size given to [`Detector::new`].
    /// Faces come back best first.
    pub fn detect(&self, rgb: &[u8], width: usize, height: usize) -> TractResult<Vec<Face>> {
        if width > self.input_width || height > self.input_height {
            return Err(TractError::msg(format!(
                "{width}x{height} image is larger than the {}x{} detector",
                self.input_width, self.input_height
            )));
        }
        if rgb.len() != width * height * 3 {
            return Err(TractError::msg(format!(
                "expected {} bytes of RGB, got {}",
                width * height * 3,
                rgb.len()
            )));
        }
        let outputs = self
            .model
            .run(tvec!(self.blob(rgb, width, height).into()))?;
        let mut faces = Vec::new();
        for (level, stride) in STRIDES.into_iter().enumerate() {
            let (cls, obj) = (
                outputs[level].to_plain_array_view::<f32>()?,
                outputs[3 + level].to_plain_array_view::<f32>()?,
            );
            let (bbox, kps) = (
                outputs[6 + level].to_plain_array_view::<f32>()?,
                outputs[9 + level].to_plain_array_view::<f32>()?,
            );
            let (cls, obj, bbox, kps) = (
                contiguous(&cls)?,
                contiguous(&obj)?,
                contiguous(&bbox)?,
                contiguous(&kps)?,
            );
            let columns = self.input_width / stride;
            for (index, (&cls, &obj)) in cls.iter().zip(obj).enumerate() {
                let score = (cls.clamp(0.0, 1.0) * obj.clamp(0.0, 1.0)).sqrt();
                if score < self.score_threshold {
                    continue;
                }
                let s = stride as f32;
                let row = (index / columns) as f32;
                let column = (index % columns) as f32;
                let b = &bbox[index * 4..index * 4 + 4];
                let k = &kps[index * 10..index * 10 + 10];
                let point = |n: usize| [(k[2 * n] + column) * s, (k[2 * n + 1] + row) * s];
                let (face_width, face_height) = (b[2].exp() * s, b[3].exp() * s);
                faces.push(Face {
                    x: (column + b[0]) * s - face_width / 2.0,
                    y: (row + b[1]) * s - face_height / 2.0,
                    width: face_width,
                    height: face_height,
                    landmarks: Landmarks {
                        right_eye: point(0),
                        left_eye: point(1),
                        nose: point(2),
                        right_mouth: point(3),
                        left_mouth: point(4),
                    },
                    score,
                });
            }
        }
        Ok(non_maximum_suppression(faces, self.nms_threshold))
    }

    /// YuNet was trained on OpenCV's blobFromImage output: BGR planes of unscaled 0-255 floats.
    fn blob(&self, rgb: &[u8], width: usize, height: usize) -> Tensor {
        let plane = self.input_width * self.input_height;
        let mut data = vec![0f32; 3 * plane];
        for y in 0..height {
            for x in 0..width {
                let source = (y * width + x) * 3;
                let target = y * self.input_width + x;
                data[target] = rgb[source + 2] as f32;
                data[plane + target] = rgb[source + 1] as f32;
                data[2 * plane + target] = rgb[source] as f32;
            }
        }
        tract_ndarray::Array4::from_shape_vec((1, 3, self.input_height, self.input_width), data)
            .expect("blob length matches its shape")
            .into()
    }
}

fn contiguous<'a>(view: &'a tract_ndarray::ArrayViewD<'_, f32>) -> TractResult<&'a [f32]> {
    view.as_slice()
        .ok_or_else(|| TractError::msg("model output is not contiguous"))
}

/// Drop the shapes recorded at export time so tract re-derives them from the new input size.
fn forget_exported_shapes(graph: &mut InferenceModel) -> TractResult<()> {
    let inputs = graph.input_outlets()?.to_vec();
    for node in 0..graph.nodes().len() {
        if graph.nodes()[node].op_is::<tract_onnx::tract_hir::ops::konst::Const>() {
            continue;
        }
        for slot in 0..graph.nodes()[node].outputs.len() {
            let outlet = OutletId::new(node, slot);
            if !inputs.contains(&outlet) {
                graph.set_outlet_fact(outlet, InferenceFact::default())?;
            }
        }
    }
    Ok(())
}

fn non_maximum_suppression(mut faces: Vec<Face>, threshold: f32) -> Vec<Face> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Face> = Vec::new();
    for face in faces {
        if kept.iter().all(|k| overlap(k, &face) <= threshold) {
            kept.push(face);
        }
    }
    kept
}

fn overlap(a: &Face, b: &Face) -> f32 {
    let width = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let height = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    if width <= 0.0 || height <= 0.0 {
        return 0.0;
    }
    let intersection = width * height;
    intersection / (a.width * a.height + b.width * b.height - intersection)
}
