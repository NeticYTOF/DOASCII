use std::io::{self, Read};

pub const FRAME_WIDTH: usize = 640;
pub const FRAME_HEIGHT: usize = 400;
const FRAME_MAGIC: &[u8; 4] = b"DGF1";
const ASCII_RAMP: &[u8] = b" .-=+#@";
const MAX_FRAME_PIXELS: usize = 16_777_216;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsciiCell {
    pub character: char,
    pub color: (u8, u8, u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    /// DoomGeneric's 32-bit RGB888 pixels, in native little-endian order.
    pub pixels: Vec<u32>,
}

impl Frame {
    pub fn from_reader(reader: &mut impl Read) -> io::Result<Self> {
        find_magic(reader)?;
        let mut header = [0; 8];
        reader.read_exact(&mut header)?;
        let width = u16::from_le_bytes([header[0], header[1]]) as usize;
        let height = u16::from_le_bytes([header[2], header[3]]) as usize;
        let pixel_bytes = header[4];
        let count = width
            .checked_mul(height)
            .filter(|count| *count <= MAX_FRAME_PIXELS)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid framebuffer dimensions")
            })?;
        if width == 0 || height == 0 || pixel_bytes != 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported framebuffer format",
            ));
        }
        let mut bytes = vec![0; count * 4];
        reader.read_exact(&mut bytes)?;
        let pixels = bytes
            .chunks_exact(4)
            .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
            .collect();
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn to_ascii_fit(
        &self,
        max_columns: usize,
        max_rows: usize,
        character_aspect: f32,
    ) -> Vec<String> {
        self.to_ascii_cells_fit(max_columns, max_rows, character_aspect)
            .into_iter()
            .map(|line| line.into_iter().map(|cell| cell.character).collect())
            .collect()
    }

    pub fn to_ascii_cells_fit(
        &self,
        max_columns: usize,
        max_rows: usize,
        character_aspect: f32,
    ) -> Vec<Vec<AsciiCell>> {
        if max_columns == 0
            || max_rows == 0
            || !character_aspect.is_finite()
            || character_aspect <= 0.0
        {
            return Vec::new();
        }
        let image_aspect = self.width as f32 / self.height.max(1) as f32;
        let target_columns_per_row = image_aspect / character_aspect;
        let canvas_aspect = max_columns as f32 / max_rows as f32;
        let (content_columns, content_rows) = if canvas_aspect > target_columns_per_row {
            (
                (max_rows as f32 * target_columns_per_row).round() as usize,
                max_rows,
            )
        } else {
            (
                max_columns,
                (max_columns as f32 / target_columns_per_row).round() as usize,
            )
        };
        let content_columns = content_columns.clamp(1, max_columns);
        let content_rows = content_rows.clamp(1, max_rows);
        let rendered = self.to_ascii_cells(content_columns, content_rows);
        let left = (max_columns - content_columns) / 2;
        let top = (max_rows - content_rows) / 2;
        let blank = AsciiCell {
            character: ' ',
            color: (0, 0, 0),
        };
        let mut canvas = vec![vec![blank; max_columns]; max_rows];
        for (index, line) in rendered.into_iter().enumerate() {
            let row = top + index;
            canvas[row][left..left + content_columns].copy_from_slice(&line);
        }
        canvas
    }

    pub fn to_ascii(&self, columns: usize, rows: usize) -> Vec<String> {
        self.to_ascii_cells(columns, rows)
            .into_iter()
            .map(|line| line.into_iter().map(|cell| cell.character).collect())
            .collect()
    }

    fn contrast_bounds(&self) -> (f32, f32) {
        if self.pixels.is_empty() {
            return (0.0, 255.0);
        }
        let mut histogram = [0_u64; 256];
        for pixel in &self.pixels {
            histogram[luminance(
                ((pixel >> 16) & 0xff) as u32,
                ((pixel >> 8) & 0xff) as u32,
                (pixel & 0xff) as u32,
            ) as usize] += 1;
        }
        let percentile = |percent: u64| {
            let target = (self.pixels.len() as u64 * percent).div_ceil(100);
            let mut accumulated = 0_u64;
            histogram
                .iter()
                .position(|count| {
                    accumulated += count;
                    accumulated >= target
                })
                .unwrap_or(0) as f32
        };
        let black = percentile(3);
        let white = percentile(98).max(black);
        (black, white)
    }

    pub fn to_ascii_cells(&self, columns: usize, rows: usize) -> Vec<Vec<AsciiCell>> {
        if columns == 0 || rows == 0 || self.pixels.len() != self.width * self.height {
            return Vec::new();
        }
        let contrast = self.contrast_bounds();
        let mut lines = Vec::with_capacity(rows);
        for row in 0..rows {
            let y0 = row * self.height / rows;
            let y1 = ((row + 1) * self.height / rows)
                .max(y0 + 1)
                .min(self.height);
            let mut line = Vec::with_capacity(columns);
            for col in 0..columns {
                let x0 = col * self.width / columns;
                let x1 = ((col + 1) * self.width / columns)
                    .max(x0 + 1)
                    .min(self.width);
                let mut histogram = [0_u32; 256];
                let mut samples = 0_u32;
                let mut red_total = 0_u64;
                let mut green_total = 0_u64;
                let mut blue_total = 0_u64;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let pixel = self.pixels[y * self.width + x];
                        let red = ((pixel >> 16) & 0xff) as u32;
                        let green = ((pixel >> 8) & 0xff) as u32;
                        let blue = (pixel & 0xff) as u32;
                        let brightness = luminance(red, green, blue) as usize;
                        histogram[brightness] += 1;
                        red_total += red as u64;
                        green_total += green as u64;
                        blue_total += blue as u64;
                        samples += 1;
                    }
                }
                let median_sample = samples / 2;
                let mut accumulated = 0_u32;
                let median = histogram
                    .iter()
                    .position(|count| {
                        accumulated += count;
                        accumulated > median_sample
                    })
                    .unwrap_or(0);
                let brightness = enhance_luminance(median as f32, contrast);
                let shade = (brightness * (ASCII_RAMP.len() - 1) as f32 / 255.0) as usize;
                let samples = samples.max(1) as u64;
                let average_red = red_total as f32 / samples as f32;
                let average_green = green_total as f32 / samples as f32;
                let average_blue = blue_total as f32 / samples as f32;
                let average_luminance = luminance(
                    average_red as u32,
                    average_green as u32,
                    average_blue as u32,
                ) as f32;
                let enhanced_luminance = enhance_luminance(average_luminance, contrast);
                let color_scale = if average_luminance > 0.0 {
                    enhanced_luminance / average_luminance
                } else {
                    0.0
                };
                let lift = |channel: f32| (channel * color_scale).clamp(0.0, 255.0) as u8;
                line.push(AsciiCell {
                    character: ASCII_RAMP[shade] as char,
                    color: (lift(average_red), lift(average_green), lift(average_blue)),
                });
            }
            lines.push(line);
        }
        lines
    }
}

fn luminance(red: u32, green: u32, blue: u32) -> u32 {
    (red * 2126 + green * 7152 + blue * 722) / 10_000
}

fn enhance_luminance(luminance: f32, (black, white): (f32, f32)) -> f32 {
    let range = white - black;
    let normalized = if range > 0.0 {
        ((luminance - black) / range).clamp(0.0, 1.0)
    } else {
        luminance / 255.0
    };
    normalized.powf(0.82) * 255.0
}

fn find_magic(reader: &mut impl Read) -> io::Result<()> {
    let mut rolling = [0; 4];
    let mut filled = 0;
    loop {
        let mut byte = [0];
        reader.read_exact(&mut byte)?;
        if filled < rolling.len() {
            rolling[filled] = byte[0];
            filled += 1;
        } else {
            rolling.rotate_left(1);
            rolling[3] = byte[0];
        }
        if filled == rolling.len() && &rolling == FRAME_MAGIC {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_reader_skips_engine_log_output_before_protocol_packet() {
        let mut bytes = b"Doom engine starting\nDGF1".to_vec();
        bytes.extend_from_slice(&[2, 0, 1, 0, 4, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 0]);
        let frame = Frame::from_reader(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.pixels, [0, 0x00ff_ffff]);
    }

    #[test]
    fn ascii_conversion_maps_dark_and_bright_pixels() {
        let frame = Frame {
            width: 2,
            height: 1,
            pixels: vec![0, 0x00ff_ffff],
        };
        assert_eq!(frame.to_ascii(2, 1), [" @"]);
    }

    #[test]
    fn aspect_fit_centers_full_frame_without_cropping() {
        let frame = Frame {
            width: 16,
            height: 10,
            pixels: vec![0x00ff_ffff; 160],
        };
        let lines = frame.to_ascii_fit(100, 100, 0.5);
        assert_eq!(lines.len(), 100);
        assert!(lines.iter().all(|line| line.len() == 100));
        let content_rows = lines.iter().filter(|line| line.contains('@')).count();
        assert_eq!(content_rows, 31);
        assert!(lines.first().unwrap().trim().is_empty());
    }

    #[test]
    fn ascii_cells_keep_the_source_palette_color() {
        let frame = Frame {
            width: 1,
            height: 1,
            pixels: vec![0x00ff_0000],
        };
        assert_eq!(
            frame.to_ascii_cells(1, 1),
            [[AsciiCell {
                character: '.',
                color: (255, 0, 0),
            }]]
        );
    }

    #[test]
    fn median_sampling_ignores_isolated_bright_pixels() {
        let frame = Frame {
            width: 8,
            height: 1,
            pixels: vec![0, 0, 0, 0, 0, 0, 0, 0x00ff_ffff],
        };
        assert_eq!(frame.to_ascii(1, 1), [" "]);
    }

    #[test]
    fn frame_contrast_stretches_doom_dark_values() {
        let frame = Frame {
            width: 7,
            height: 1,
            pixels: [0, 20, 40, 80, 160, 200, 240]
                .into_iter()
                .map(|value| value * 0x010101)
                .collect(),
        };
        assert_eq!(frame.to_ascii(7, 1)[0].chars().nth(4), Some('+'));
    }

    #[test]
    fn midtones_use_a_brightened_ascii_ramp() {
        let frame = Frame {
            width: 1,
            height: 1,
            pixels: vec![0x0080_8080],
        };
        assert_eq!(frame.to_ascii(1, 1), ["="]);
    }

    #[test]
    fn rejects_unsupported_pixel_formats() {
        let mut bytes = b"DGF1".to_vec();
        bytes.extend_from_slice(&[1, 0, 1, 0, 3, 0, 0, 0]);
        assert_eq!(
            Frame::from_reader(&mut Cursor::new(bytes))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
