use font8x8::{BASIC_FONTS, UnicodeFonts};
use rusttype::{Font, PositionedGlyph, Scale, point};
use std::fs;
use std::sync::OnceLock;

const CLOCK_FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
];

static CLOCK_FONT: OnceLock<Option<Font<'static>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Framebuffer {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Framebuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        self.pixels[y * self.width + x] != 0
    }

    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        if x >= self.width || y >= self.height {
            return;
        }

        self.pixels[y * self.width + x] = u8::from(value);
    }

    pub fn draw_text_centered(&mut self, text: &str, y: i32, scale: usize, x_offset: i32) {
        let text_width = self.measure_text(text, scale) as i32;
        let x = ((self.width as i32 - text_width) / 2) + x_offset;
        self.draw_text(text, x, y, scale);
    }

    pub fn draw_multiline_centered(&mut self, text: &str, top: i32, line_gap: i32, scale: usize) {
        for (index, line) in text.lines().enumerate() {
            let y = top + index as i32 * ((8 * scale) as i32 + line_gap);
            self.draw_text_centered(line, y, scale, 0);
        }
    }

    pub fn measure_text(&self, text: &str, scale: usize) -> usize {
        let glyph_width = 8 * scale;
        text.chars().count().saturating_mul(glyph_width)
    }

    pub fn draw_text(&mut self, text: &str, x: i32, y: i32, scale: usize) {
        let glyph_width = (8 * scale) as i32;
        for (index, ch) in text.chars().enumerate() {
            self.draw_char(ch, x + glyph_width * index as i32, y, scale);
        }
    }

    pub fn draw_clock_text_centered(
        &mut self,
        text: &str,
        y: i32,
        pixel_height: f32,
        x_offset: i32,
    ) -> bool {
        let Some(font) = clock_font() else {
            return false;
        };

        let scale = Scale::uniform(pixel_height);
        let v_metrics = font.v_metrics(scale);
        let glyphs: Vec<_> = font
            .layout(text, scale, point(0.0, v_metrics.ascent))
            .collect();
        let Some((min_x, max_x, min_y)) = glyph_bounds(&glyphs) else {
            return false;
        };

        let text_width = max_x - min_x;
        let target_x = ((self.width as i32 - text_width) / 2) + x_offset;
        let dx = target_x - min_x;
        let dy = y - min_y;

        for glyph in glyphs {
            let Some(bounds) = glyph.pixel_bounding_box() else {
                continue;
            };

            glyph.draw(|gx, gy, coverage| {
                let px = bounds.min.x + gx as i32 + dx;
                let py = bounds.min.y + gy as i32 + dy;
                if px < 0 || py < 0 {
                    return;
                }

                let px = px as usize;
                let py = py as usize;
                if px >= self.width || py >= self.height {
                    return;
                }

                if coverage_to_pixel(coverage, px, py) {
                    self.set(px, py, true);
                }
            });
        }

        true
    }

    pub fn draw_char(&mut self, ch: char, x: i32, y: i32, scale: usize) {
        let Some(glyph) = BASIC_FONTS.get(ch) else {
            return;
        };

        for (row, byte) in glyph.iter().enumerate() {
            for column in 0..8 {
                if (byte & (1 << column)) == 0 {
                    continue;
                }

                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = x + (column * scale + dx) as i32;
                        let py = y + (row * scale + dy) as i32;
                        if px >= 0 && py >= 0 {
                            self.set(px as usize, py as usize, true);
                        }
                    }
                }
            }
        }
    }

    pub fn from_centered_text_screen(width: usize, height: usize, text: &str) -> Self {
        let mut framebuffer = Self::new(width, height);
        let line_count = text.lines().count().max(1);
        let scale = if line_count == 1 && text.chars().count() <= 8 {
            2
        } else {
            1
        };
        let text_height =
            (line_count as i32 * 8 * scale as i32) + (line_count.saturating_sub(1) as i32 * 2);
        let top = ((height as i32 - text_height) / 2).max(0);
        framebuffer.draw_multiline_centered(text, top, 2, scale);
        framebuffer
    }
}

fn clock_font() -> Option<&'static Font<'static>> {
    CLOCK_FONT.get_or_init(load_clock_font).as_ref()
}

fn load_clock_font() -> Option<Font<'static>> {
    for path in CLOCK_FONT_PATHS {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        let leaked = Box::leak(bytes.into_boxed_slice());
        if let Some(font) = Font::try_from_bytes(leaked) {
            return Some(font);
        }
    }
    None
}

fn glyph_bounds(glyphs: &[PositionedGlyph<'_>]) -> Option<(i32, i32, i32)> {
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut saw_bounds = false;

    for bounds in glyphs.iter().filter_map(PositionedGlyph::pixel_bounding_box) {
        saw_bounds = true;
        min_x = min_x.min(bounds.min.x);
        max_x = max_x.max(bounds.max.x);
        min_y = min_y.min(bounds.min.y);
    }

    saw_bounds.then_some((min_x, max_x, min_y))
}

fn coverage_to_pixel(coverage: f32, _x: usize, _y: usize) -> bool {
    coverage >= 0.5
}

#[cfg(test)]
mod tests {
    use super::Framebuffer;

    #[test]
    fn drawing_text_sets_pixels() {
        let mut framebuffer = Framebuffer::new(128, 64);
        framebuffer.draw_text("12:34", 0, 0, 1);
        assert!(framebuffer.pixels.iter().any(|pixel| *pixel == 1));
    }

    #[test]
    fn centered_text_measurement_scales() {
        let framebuffer = Framebuffer::new(128, 64);
        assert_eq!(framebuffer.measure_text("AB", 1), 16);
        assert_eq!(framebuffer.measure_text("AB", 2), 32);
    }

    #[test]
    fn centered_text_screen_renders_pixels() {
        let framebuffer = Framebuffer::from_centered_text_screen(128, 64, "TEST");
        assert!(framebuffer.pixels.iter().any(|pixel| *pixel == 1));
    }
}
