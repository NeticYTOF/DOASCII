use std::io::{self, Read};

pub const FRAME_WIDTH: usize = 640;
pub const FRAME_HEIGHT: usize = 400;
const FRAME_MAGIC: &[u8; 4] = b"DGF1";
const ASCII_RAMP: &[u8] =
    b" .'`^\"\\,:;Il!i~+_-?][}{1)(|/*tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$@";
const MAX_FRAME_PIXELS: usize = 16_777_216;

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

    pub fn to_ascii(&self, columns: usize, rows: usize) -> Vec<String> {
        if columns == 0 || rows == 0 || self.pixels.len() != self.width * self.height {
            return Vec::new();
        }
        let mut lines = Vec::with_capacity(rows);
        for row in 0..rows {
            let y0 = row * self.height / rows;
            let y1 = ((row + 1) * self.height / rows)
                .max(y0 + 1)
                .min(self.height);
            let mut line = String::with_capacity(columns);
            for col in 0..columns {
                let x0 = col * self.width / columns;
                let x1 = ((col + 1) * self.width / columns)
                    .max(x0 + 1)
                    .min(self.width);
                let mut light = 0_u64;
                let mut samples = 0_u64;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let pixel = self.pixels[y * self.width + x];
                        let red = ((pixel >> 16) & 0xff) as u64;
                        let green = ((pixel >> 8) & 0xff) as u64;
                        let blue = (pixel & 0xff) as u64;
                        light += (red * 2126 + green * 7152 + blue * 722) / 10_000;
                        samples += 1;
                    }
                }
                let brightness = (light / samples.max(1)) as usize;
                let shade = brightness * (ASCII_RAMP.len() - 1) / 255;
                line.push(ASCII_RAMP[shade] as char);
            }
            lines.push(line);
        }
        lines
    }
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
