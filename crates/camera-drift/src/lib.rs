//! Estimate how far a webcam has tilted since a reference frame, ignoring the person in front of it.
//!
//! Tilting a laptop lid moves the whole image, whereas a person moving only moves themselves.
//! Rotating a camera shifts near and far things equally, so the background's vertical shift can
//! be subtracted straight from anything measured on the person.
//!
//! Textured patches of background are picked from the reference frame, away from the person,
//! and found again in each new frame with a coarse-to-fine normalised cross-correlation search.
//! The median of their vertical movement is robust to the odd patch the person walks across.

/// A region to keep out of the background, typically a detected face. Pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// Whether `(x, y)` is somewhere the person's head or body could be, given their face.
    fn covers_person(&self, x: f32, y: f32) -> bool {
        x >= self.x - self.width
            && x <= self.x + 2.0 * self.width
            && y >= self.y - 0.6 * self.height
    }
}

/// A borrowed 8-bit greyscale image, tightly packed.
#[derive(Clone, Copy, Debug)]
pub struct Grey<'a> {
    pixels: &'a [u8],
    width: usize,
    height: usize,
}

impl<'a> Grey<'a> {
    /// `None` unless `pixels` holds exactly `width * height` bytes.
    pub fn new(pixels: &'a [u8], width: usize, height: usize) -> Option<Self> {
        (pixels.len() == width * height).then_some(Self {
            pixels,
            width,
            height,
        })
    }
}

const PATCH_HALF: isize = 6;
const MAX_PATCHES: usize = 40;
const MIN_MATCHES: usize = 8;
const MIN_CORRELATION: f32 = 0.8;
/// Below this share of patches still matching, the scene has changed too much to keep comparing.
const MIN_SURVIVING: f32 = 0.4;
/// Furthest the search looks, as fractions of the frame size.
const REACH_Y: f32 = 0.15;
const REACH_X: f32 = 0.05;

/// Tracks the background against a reference frame. See the crate docs.
#[derive(Debug)]
pub struct CameraDrift {
    reference: Pyramid,
    patches: Vec<(isize, isize)>,
    carried: f32,
    last: f32,
}

impl CameraDrift {
    pub fn new(frame: Grey, person: Option<Rect>) -> Self {
        let mut drift = Self {
            reference: Pyramid::new(frame),
            patches: Vec::new(),
            carried: 0.0,
            last: 0.0,
        };
        drift.patches = pick_patches(&drift.reference.fine, person);
        drift
    }

    /// The most recent estimate: vertical shift since the reference, as a fraction of frame height.
    pub fn last(&self) -> f32 {
        self.last
    }

    /// Compare `frame` with the reference and return the vertical shift as a fraction of frame height.
    /// Positive means the scene moved down the image, as when the camera tilts up.
    pub fn update(&mut self, frame: Grey, person: Option<Rect>) -> f32 {
        let current = Pyramid::new(frame);
        let height = frame.height as f32;
        let mut shifts: Vec<f32> = self
            .patches
            .iter()
            .filter_map(|&(x, y)| self.track(&current, x, y))
            .filter(|&(x, y, _)| person.is_none_or(|p| !p.covers_person(x, y)))
            .map(|(_, _, dy)| dy)
            .collect();
        if shifts.len() >= MIN_MATCHES {
            shifts.sort_by(f32::total_cmp);
            self.last = self.carried + shifts[shifts.len() / 2] / height;
        }
        // Too few patches to ever reach MIN_MATCHES (a dark or blank reference, say) also means
        // starting again, or the estimate would be stuck where it is.
        let too_few_tracked = (shifts.len() as f32) < self.patches.len() as f32 * MIN_SURVIVING;
        if too_few_tracked || self.patches.len() < MIN_MATCHES {
            self.carried = self.last;
            self.patches = pick_patches(&current.fine, person);
            self.reference = current;
        }
        self.last
    }

    /// Find the patch at `(x, y)` of the reference in `current`; returns where it is now and its vertical move.
    fn track(&self, current: &Pyramid, x: isize, y: isize) -> Option<(f32, f32, f32)> {
        let fine = &self.reference.fine;
        let reach_y = (fine.height as f32 * REACH_Y / 2.0) as isize;
        let reach_x = (fine.width as f32 * REACH_X / 2.0) as isize;
        let (coarse_dx, coarse_dy, _) = best_match(
            &self.reference.coarse,
            (x / 2, y / 2),
            &current.coarse,
            (0, 0),
            (reach_x, reach_y),
            PATCH_HALF / 2,
        )?;
        let (dx, dy, scores) = best_match(
            fine,
            (x, y),
            &current.fine,
            (coarse_dx * 2, coarse_dy * 2),
            (2, 2),
            PATCH_HALF,
        )?;
        if scores[1] < MIN_CORRELATION {
            return None;
        }
        let dy = dy as f32 + parabola_peak(scores);
        Some(((x + dx) as f32, (y as f32) + dy, dy))
    }
}

#[derive(Debug)]
struct Image {
    pixels: Vec<f32>,
    width: usize,
    height: usize,
}

impl Image {
    fn at(&self, x: isize, y: isize) -> f32 {
        self.pixels[y as usize * self.width + x as usize]
    }

    fn contains(&self, x: isize, y: isize, half: isize) -> bool {
        x - half >= 0
            && y - half >= 0
            && x + half < self.width as isize
            && y + half < self.height as isize
    }
}

#[derive(Debug)]
struct Pyramid {
    fine: Image,
    coarse: Image,
}

impl Pyramid {
    fn new(frame: Grey) -> Self {
        let fine = Image {
            pixels: frame.pixels.iter().map(|&p| p as f32).collect(),
            width: frame.width,
            height: frame.height,
        };
        let (width, height) = (frame.width / 2, frame.height / 2);
        let mut coarse = Image {
            pixels: Vec::with_capacity(width * height),
            width,
            height,
        };
        for y in 0..height as isize {
            for x in 0..width as isize {
                let (fx, fy) = (2 * x, 2 * y);
                let sum = fine.at(fx, fy)
                    + fine.at(fx + 1, fy)
                    + fine.at(fx, fy + 1)
                    + fine.at(fx + 1, fy + 1);
                coarse.pixels.push(sum / 4.0);
            }
        }
        Self { fine, coarse }
    }
}

/// The most textured patch centre in each cell of a grid over the background, best first.
fn pick_patches(image: &Image, person: Option<Rect>) -> Vec<(isize, isize)> {
    let cell = 3 * PATCH_HALF as usize;
    let margin = PATCH_HALF + 1;
    let mut candidates = Vec::new();
    // The coarse level searches half-size patches at half resolution, so it needs this much too.
    if image.width < 4 * margin as usize || image.height < 4 * margin as usize {
        return Vec::new();
    }
    for cy in (margin as usize..image.height - margin as usize).step_by(cell) {
        for cx in (margin as usize..image.width - margin as usize).step_by(cell) {
            let (x, y) = (cx as isize, cy as isize);
            if person.is_some_and(|p| p.covers_person(x as f32, y as f32)) {
                continue;
            }
            let texture = corner_strength(image, x, y);
            if texture > 0.0 {
                candidates.push((texture, x, y));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    candidates
        .into_iter()
        .take(MAX_PATCHES)
        .map(|(_, x, y)| (x, y))
        .collect()
}

/// Shi-Tomasi corner strength: the weaker direction of gradient energy, so flat walls and plain
/// edges (which can't pin down vertical movement) score low.
fn corner_strength(image: &Image, x: isize, y: isize) -> f32 {
    let (mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0);
    for py in y - PATCH_HALF..=y + PATCH_HALF {
        for px in x - PATCH_HALF..=x + PATCH_HALF {
            let gx = image.at(px + 1, py) - image.at(px - 1, py);
            let gy = image.at(px, py + 1) - image.at(px, py - 1);
            xx += gx * gx;
            yy += gy * gy;
            xy += gx * gy;
        }
    }
    let mean = (xx + yy) / 2.0;
    mean - (((xx - yy) / 2.0).powi(2) + xy * xy).sqrt()
}

/// Search `current` around `centre + offset` for the patch of `reference` at `centre`.
/// Returns the best offset and the correlation just above, at, and below it.
fn best_match(
    reference: &Image,
    (x, y): (isize, isize),
    current: &Image,
    (ox, oy): (isize, isize),
    (reach_x, reach_y): (isize, isize),
    half: isize,
) -> Option<(isize, isize, [f32; 3])> {
    if !reference.contains(x, y, half) {
        return None;
    }
    let template = Window::new(reference, x, y, half);
    let score = |dx: isize, dy: isize| {
        let (cx, cy) = (x + dx, y + dy);
        current
            .contains(cx, cy, half)
            .then(|| template.correlate_at(current, cx, cy, half))
    };
    let mut best: Option<(isize, isize, f32)> = None;
    for dy in oy - reach_y..=oy + reach_y {
        for dx in ox - reach_x..=ox + reach_x {
            if let Some(s) = score(dx, dy)
                && best.is_none_or(|b| s > b.2)
            {
                best = Some((dx, dy, s));
            }
        }
    }
    let (dx, dy, peak) = best?;
    Some((
        dx,
        dy,
        [
            score(dx, dy - 1).unwrap_or(peak),
            peak,
            score(dx, dy + 1).unwrap_or(peak),
        ],
    ))
}

/// Sub-pixel offset of a peak from three samples one pixel apart.
fn parabola_peak([above, peak, below]: [f32; 3]) -> f32 {
    let curvature = above - 2.0 * peak + below;
    if curvature >= 0.0 {
        0.0
    } else {
        (0.5 * (above - below) / curvature).clamp(-0.5, 0.5)
    }
}

/// The widest patch side, at the fine level.
const MAX_SIDE: usize = 2 * PATCH_HALF as usize + 1;

/// A patch with its mean removed, ready for normalised cross-correlation.
struct Window {
    values: [f32; MAX_SIDE * MAX_SIDE],
    norm: f32,
}

impl Window {
    fn new(image: &Image, x: isize, y: isize, half: isize) -> Self {
        let side = (2 * half + 1) as usize;
        let len = side * side;
        let mut values = [0f32; MAX_SIDE * MAX_SIDE];
        for (row, py) in (y - half..=y + half).enumerate() {
            let start = py as usize * image.width + (x - half) as usize;
            values[row * side..(row + 1) * side]
                .copy_from_slice(&image.pixels[start..start + side]);
        }
        let mean = values[..len].iter().sum::<f32>() / len as f32;
        values[..len].iter_mut().for_each(|v| *v -= mean);
        let norm = values[..len].iter().map(|v| v * v).sum::<f32>().sqrt();
        Self { values, norm }
    }

    /// Correlate with the window of `image` at `(x, y)` without copying it. The template is
    /// zero-mean, so the other window's mean drops out of the dot product and only its own
    /// sums are needed for its norm.
    fn correlate_at(&self, image: &Image, x: isize, y: isize, half: isize) -> f32 {
        let side = (2 * half + 1) as usize;
        // Per-column accumulators keep the inner loop free of dependencies so it vectorises.
        let (mut dot, mut sum, mut squares) =
            ([0f32; MAX_SIDE], [0f32; MAX_SIDE], [0f32; MAX_SIDE]);
        for row in 0..side {
            let start = (y - half) as usize * image.width + row * image.width + (x - half) as usize;
            let pixels = &image.pixels[start..start + side];
            let template = &self.values[row * side..(row + 1) * side];
            for i in 0..side {
                dot[i] += template[i] * pixels[i];
                sum[i] += pixels[i];
                squares[i] += pixels[i] * pixels[i];
            }
        }
        let (dot, sum, squares): (f32, f32, f32) =
            (dot.iter().sum(), sum.iter().sum(), squares.iter().sum());
        let variance = squares - sum * sum / (side * side) as f32;
        if self.norm == 0.0 || variance <= 0.0 {
            return 0.0;
        }
        dot / (self.norm * variance.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 320;
    const HEIGHT: usize = 240;

    /// A busy, blurred random scene, so every patch has texture.
    fn scene(seed: u32) -> Vec<u8> {
        let mut state = seed;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state % 256) as f32
        };
        let (w, h) = (WIDTH + 200, HEIGHT + 200);
        let raw: Vec<f32> = (0..w * h).map(|_| noise()).collect();
        let mut blurred = vec![0u8; w * h];
        for y in 2..h - 2 {
            for x in 2..w - 2 {
                let mut sum = 0.0;
                for dy in 0..5 {
                    for dx in 0..5 {
                        sum += raw[(y + dy - 2) * w + x + dx - 2];
                    }
                }
                blurred[y * w + x] = (sum / 25.0 * 2.0 - 128.0).clamp(0.0, 255.0) as u8;
            }
        }
        blurred
    }

    /// The window of `scene` seen by a camera whose view has moved `dy` pixels down the scene.
    fn view(scene: &[u8], dy: isize) -> Vec<u8> {
        let w = WIDTH + 200;
        let mut out = Vec::with_capacity(WIDTH * HEIGHT);
        for y in 0..HEIGHT {
            let sy = (y as isize + 100 - dy) as usize;
            out.extend_from_slice(&scene[sy * w + 100..sy * w + 100 + WIDTH]);
        }
        out
    }

    fn grey(pixels: &[u8]) -> Grey<'_> {
        Grey::new(pixels, WIDTH, HEIGHT).unwrap()
    }

    fn pixels(fraction: f32) -> f32 {
        fraction * HEIGHT as f32
    }

    #[test]
    fn measures_a_tilt() {
        let world = scene(7);
        let mut drift = CameraDrift::new(grey(&view(&world, 0)), None);
        for dy in [0, 12, -12, 30] {
            let shift = pixels(drift.update(grey(&view(&world, dy)), None));
            assert!(
                (shift - dy as f32).abs() < 0.5,
                "moved {dy}, measured {shift}"
            );
        }
    }

    #[test]
    fn ignores_the_person() {
        let world = scene(11);
        let other = scene(99);
        let face = Rect {
            x: 130.0,
            y: 60.0,
            width: 60.0,
            height: 80.0,
        };
        let reference = view(&world, 0);
        let mut drift = CameraDrift::new(grey(&reference), Some(face));
        // The person's region changes completely while the background stays put.
        let mut frame = reference.clone();
        let replaced = view(&other, 0);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if face.covers_person(x as f32, y as f32) {
                    frame[y * WIDTH + x] = replaced[y * WIDTH + x];
                }
            }
        }
        let shift = pixels(drift.update(grey(&frame), Some(face)));
        assert!(shift.abs() < 0.5, "measured {shift} with a still camera");
    }

    #[test]
    fn recovers_from_a_blank_first_frame() {
        let blank = vec![40u8; WIDTH * HEIGHT];
        let world = scene(5);
        let mut drift = CameraDrift::new(grey(&blank), None);
        drift.update(grey(&view(&world, 0)), None);
        let shift = pixels(drift.update(grey(&view(&world, 12)), None));
        assert!(
            (shift - 12.0).abs() < 0.5,
            "stuck after a blank start: {shift}"
        );
    }

    #[test]
    fn tiny_frames_are_harmless() {
        let tiny = [0u8; 6 * 5];
        let tiny = Grey::new(&tiny, 6, 5).unwrap();
        let mut drift = CameraDrift::new(tiny, None);
        assert_eq!(drift.update(tiny, None), 0.0);
        assert!(Grey::new(&[0; 10], 6, 5).is_none());
    }

    #[test]
    fn keeps_the_offset_when_the_scene_changes() {
        let world = scene(3);
        let mut drift = CameraDrift::new(grey(&view(&world, 0)), None);
        drift.update(grey(&view(&world, 10)), None);
        let unrecognisable = scene(1234);
        let shift = pixels(drift.update(grey(&view(&unrecognisable, 0)), None));
        assert!((shift - 10.0).abs() < 0.5, "lost the offset: {shift}");
    }
}
