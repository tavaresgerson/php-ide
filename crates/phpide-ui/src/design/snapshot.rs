//! CPU snapshots of egui frames, for tests.
//!
//! Runs a context headlessly, tessellates the output and rasterizes the
//! meshes in software (gamma-space, premultiplied alpha — the same blending
//! the GPU renderers use), so snapshots need no GPU and no extra
//! dependencies, and render the same on every machine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, epaint, Color32, Pos2, Rect, TextureId};

struct Texture {
    size: [usize; 2],
    pixels: Vec<Color32>,
}

impl Texture {
    fn sample(&self, uv: Pos2) -> [f32; 4] {
        let (w, h) = (self.size[0] as f32, self.size[1] as f32);
        let x = (uv.x * w - 0.5).clamp(0.0, w - 1.0);
        let y = (uv.y * h - 0.5).clamp(0.0, h - 1.0);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = (
            (x0 + 1).min(self.size[0] - 1),
            (y0 + 1).min(self.size[1] - 1),
        );
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let texel = |x: usize, y: usize| {
            let c = self.pixels[y * self.size[0] + x];
            [c.r() as f32, c.g() as f32, c.b() as f32, c.a() as f32]
        };
        let (a, b, c, d) = (texel(x0, y0), texel(x1, y0), texel(x0, y1), texel(x1, y1));
        let mut out = [0.0; 4];
        for i in 0..4 {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bottom = c[i] + (d[i] - c[i]) * fx;
            out[i] = (top + (bottom - top) * fy) / 255.0;
        }
        out
    }
}

/// An RGBA8 image (straight alpha is not needed: the canvas is opaque).
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn crop_height(mut self, height: usize) -> Image {
        let height = height.min(self.height);
        self.rgba.truncate(self.width * height * 4);
        self.height = height;
        self
    }

    pub fn write_png(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(file),
            self.width as u32,
            self.height as u32,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::High);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&self.rgba)
            .unwrap();
    }

    pub fn read_png(path: &Path) -> Option<Image> {
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).ok()?));
        let mut reader = decoder.read_info().ok()?;
        let mut rgba = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut rgba).ok()?;
        if info.color_type != png::ColorType::Rgba {
            return None;
        }
        rgba.truncate(info.buffer_size());
        Some(Image {
            width: info.width as usize,
            height: info.height as usize,
            rgba,
        })
    }

    /// Number of pixels whose channels differ by more than `tolerance`.
    pub fn diff_count(&self, other: &Image, tolerance: u8) -> usize {
        if (self.width, self.height) != (other.width, other.height) {
            return usize::MAX;
        }
        self.rgba
            .chunks_exact(4)
            .zip(other.rgba.chunks_exact(4))
            .filter(|(a, b)| {
                a.iter()
                    .zip(b.iter())
                    .any(|(x, y)| x.abs_diff(*y) > tolerance)
            })
            .count()
    }
}

/// Renders `frames` frames of `ui_fn` at `size` and returns the last frame.
pub fn render(
    ctx: &egui::Context,
    size: egui::Vec2,
    frames: usize,
    mut ui_fn: impl FnMut(&mut egui::Ui),
) -> Image {
    let mut textures: HashMap<TextureId, Texture> = HashMap::new();
    let mut last = None;
    for frame in 0..frames {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            time: Some(1.0 + frame as f64 / 60.0),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, &mut ui_fn);
        for (id, delta) in output
            .textures_delta
            .set
            .iter()
            .flat_map(|(id, deltas)| deltas.iter().map(move |delta| (id, delta)))
        {
            let egui::ImageData::Color(image) = &delta.image;
            match delta.pos {
                None => {
                    textures.insert(
                        *id,
                        Texture {
                            size: image.size,
                            pixels: image.pixels.clone(),
                        },
                    );
                }
                Some([px, py]) => {
                    let texture = textures.get_mut(id).expect("patch of unknown texture");
                    for y in 0..image.size[1] {
                        for x in 0..image.size[0] {
                            texture.pixels[(py + y) * texture.size[0] + px + x] =
                                image.pixels[y * image.size[0] + x];
                        }
                    }
                }
            }
        }
        for id in &output.textures_delta.free {
            textures.remove(id);
        }
        output.textures_delta.clear();
        last = Some(output);
    }
    let output = last.expect("at least one frame");
    let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
    rasterize(&primitives, &textures, size.x as usize, size.y as usize)
}

fn rasterize(
    primitives: &[epaint::ClippedPrimitive],
    textures: &HashMap<TextureId, Texture>,
    width: usize,
    height: usize,
) -> Image {
    let mut canvas = vec![[0.0_f32, 0.0, 0.0, 1.0]; width * height];
    for clipped in primitives {
        let epaint::Primitive::Mesh(mesh) = &clipped.primitive else {
            continue;
        };
        let Some(texture) = textures.get(&mesh.texture_id) else {
            continue;
        };
        let clip = clipped.clip_rect.intersect(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(width as f32, height as f32),
        ));
        if !clip.is_positive() {
            continue;
        }
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [
                &mesh.vertices[triangle[0] as usize],
                &mesh.vertices[triangle[1] as usize],
                &mesh.vertices[triangle[2] as usize],
            ];
            let area = edge(a.pos, b.pos, c.pos);
            if area.abs() < 1e-8 {
                continue;
            }
            let min_x = a.pos.x.min(b.pos.x).min(c.pos.x).max(clip.min.x).floor() as usize;
            let max_x = a.pos.x.max(b.pos.x).max(c.pos.x).min(clip.max.x).ceil() as usize;
            let min_y = a.pos.y.min(b.pos.y).min(c.pos.y).max(clip.min.y).floor() as usize;
            let max_y = a.pos.y.max(b.pos.y).max(c.pos.y).min(clip.max.y).ceil() as usize;
            for y in min_y..max_y.min(height) {
                for x in min_x..max_x.min(width) {
                    let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    if !clip.contains(p) {
                        continue;
                    }
                    let w0 = edge(b.pos, c.pos, p) / area;
                    let w1 = edge(c.pos, a.pos, p) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let uv = Pos2::new(
                        a.uv.x * w0 + b.uv.x * w1 + c.uv.x * w2,
                        a.uv.y * w0 + b.uv.y * w1 + c.uv.y * w2,
                    );
                    let tex = texture.sample(uv);
                    let mut src = [0.0; 4];
                    for (i, value) in src.iter_mut().enumerate() {
                        let channel = |v: &epaint::Vertex| v.color.to_array()[i] as f32 / 255.0;
                        *value = (channel(a) * w0 + channel(b) * w1 + channel(c) * w2) * tex[i];
                    }
                    let dst = &mut canvas[y * width + x];
                    for i in 0..4 {
                        dst[i] = src[i] + dst[i] * (1.0 - src[3]);
                    }
                }
            }
        }
    }
    let rgba = canvas
        .iter()
        .flat_map(|p| {
            [
                (p[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (p[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (p[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                255,
            ]
        })
        .collect();
    Image {
        width,
        height,
        rgba,
    }
}

fn edge(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// Compares `image` with the committed snapshot `name`. Set
/// `UPDATE_SNAPSHOTS=1` to (re)write it. On mismatch the new image is saved
/// under `target/snapshots/` for review.
pub fn assert_snapshot(name: &str, image: &Image) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let expected_path = manifest.join("tests/snapshots").join(format!("{name}.png"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        image.write_png(&expected_path);
        return;
    }
    let actual_path: PathBuf = manifest
        .join("../../target/snapshots")
        .join(format!("{name}.new.png"));
    let Some(expected) = Image::read_png(&expected_path) else {
        image.write_png(&actual_path);
        panic!(
            "missing snapshot {}; review {} and run with UPDATE_SNAPSHOTS=1",
            expected_path.display(),
            actual_path.display()
        );
    };
    let differing = expected.diff_count(image, 24);
    // Small tolerance for float differences across CPUs.
    let allowed = image.width * image.height / 2000;
    if differing > allowed {
        image.write_png(&actual_path);
        panic!(
            "snapshot {name} differs in {differing} pixels (allowed {allowed}); new image at {}. \
             Run with UPDATE_SNAPSHOTS=1 if the change is intended.",
            actual_path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_filled_rect_and_text() {
        let ctx = egui::Context::default();
        let theme = crate::design::Theme::default();
        crate::design::install(&ctx, theme.clone());
        let image = render(&ctx, egui::vec2(64.0, 32.0), 2, |ui| {
            ui.painter().rect_filled(
                Rect::from_min_size(Pos2::ZERO, egui::vec2(32.0, 32.0)),
                0.0,
                theme.palette.accent,
            );
            ui.painter().text(
                egui::pos2(40.0, 4.0),
                egui::Align2::LEFT_TOP,
                "A",
                egui::FontId::proportional(20.0),
                theme.palette.text_primary,
            );
        });
        let pixel = |x: usize, y: usize| {
            let i = (y * image.width + x) * 4;
            [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]]
        };
        let accent = theme.palette.accent;
        assert_eq!(pixel(10, 10), [accent.r(), accent.g(), accent.b()]);
        let lit = (40..60)
            .flat_map(|x| (4..28).map(move |y| (x, y)))
            .filter(|&(x, y)| pixel(x, y)[0] > 128)
            .count();
        assert!(lit > 20, "glyph pixels: {lit}");
    }
}
