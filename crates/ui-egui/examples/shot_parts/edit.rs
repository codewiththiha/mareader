//! Last touches on an egui shot: late keys, marked boxes and a crop.

use egui_kittest::Harness;
use image::{Rgba, RgbaImage};

/// Pixels per point in every shot.
pub const SCALE: f32 = 2.0;
/// A marked box's outline, in pixels.
const THICK: u32 = 4;
const INK: Rgba<u8> = Rgba([255, 45, 120, 255]);

/// What runs after everything else, and what is drawn on the image.
#[derive(Default)]
pub struct Finish {
    keys: Vec<egui::Key>,
    /// Words typed after the keys, `_` for a space.
    typed: Vec<String>,
    frames: usize,
    marks: Vec<[f32; 4]>,
    crop: Option<[f32; 4]>,
}

impl Finish {
    /// Takes `key`, `typed`, `frames`, `mark` and `crop`; false otherwise.
    pub fn take(&mut self, k: &str, v: &str) -> Result<bool, String> {
        match k {
            "key" => {
                let key = egui::Key::from_name(v);
                self.keys.push(key.ok_or(format!("no key named {v}"))?);
            }
            "typed" => self.typed.push(v.replace('_', " ")),
            "frames" => self.frames = v.parse().map_err(|_| "frames=count")?,
            "mark" => self.marks.push(rect(v, "mark")?),
            "crop" => self.crop = Some(rect(v, "crop")?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Presses the keys, then runs exactly `frames`: `key=Space frames=8`.
    pub fn press<S>(&self, harness: &mut Harness<'_, S>) {
        for key in &self.keys {
            harness.key_press(*key);
            harness.run_steps(1);
        }
        for text in &self.typed {
            harness.event(egui::Event::Text(text.clone()));
            harness.run_steps(1);
        }
        harness.run_steps(self.frames);
    }

    /// Outlines each marked box, then keeps only the crop.
    pub fn touch(&self, mut image: RgbaImage) -> RgbaImage {
        for b in &self.marks {
            mark(&mut image, *b);
        }
        match self.crop {
            Some(b) => crop(&image, b),
            None => image,
        }
    }
}

/// A box as `x,y,w,h` in points.
fn rect(v: &str, what: &str) -> Result<[f32; 4], String> {
    let n: Vec<f32> = v.split(',').filter_map(|n| n.parse().ok()).collect();
    match n.as_slice() {
        [x, y, w, h] => Ok([*x, *y, *w, *h]),
        _ => Err(format!("{what}=x,y,w,h")),
    }
}

/// The box in pixels, clamped to the image: left, top, right, bottom.
fn pixels(b: [f32; 4], image: &RgbaImage) -> (u32, u32, u32, u32) {
    let (w, h) = image.dimensions();
    let px = |v: f32, most: u32| ((v * SCALE).max(0.0) as u32).min(most);
    let [x, y, bw, bh] = b;
    (px(x, w), px(y, h), px(x + bw, w), px(y + bh, h))
}

fn mark(image: &mut RgbaImage, b: [f32; 4]) {
    let (l, t, r, btm) = pixels(b, image);
    for (x, y) in (l..r).flat_map(|x| (t..btm).map(move |y| (x, y))) {
        let edge = x < l + THICK || x + THICK >= r || y < t + THICK || y + THICK >= btm;
        if edge {
            image.put_pixel(x, y, INK);
        }
    }
}

fn crop(image: &RgbaImage, b: [f32; 4]) -> RgbaImage {
    let (l, t, r, btm) = pixels(b, image);
    image::imageops::crop_imm(image, l, t, r - l, btm - t).to_image()
}
