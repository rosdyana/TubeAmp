/// A decoded skin bitmap: row-major pixels packed as `0xAARRGGBB`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

/// Classic Winamp transparency key: pure magenta pixels in skin bitmaps are
/// drawn transparent.
pub const TRANSPARENT_KEY: (u8, u8, u8) = (255, 0, 255);

/// Packs channels into `0xAARRGGBB`.
pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// Decodes a BMP or PNG (format is sniffed from the bytes) into `Rgba`,
/// applying the magenta transparency key.
pub fn decode(bytes: &[u8]) -> Result<Rgba, image::ImageError> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let (w, h) = img.dimensions();
    let px = img
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            if (r, g, b) == TRANSPARENT_KEY {
                0
            } else {
                argb(a, r, g, b)
            }
        })
        .collect();
    Ok(Rgba { w, h, px })
}

#[cfg(test)]
mod tests {
    use super::{argb, decode, Rgba};
    use std::io::Cursor;

    fn bmp(img: &image::RgbImage) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img.clone())
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Bmp)
            .expect("encode bmp");
        out
    }

    #[test]
    fn magenta_decodes_as_transparent_and_other_pixels_are_opaque() {
        let mut img = image::RgbImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgb([255, 0, 255]));
        img.put_pixel(1, 0, image::Rgb([1, 2, 3]));
        let decoded: Rgba = decode(&bmp(&img)).expect("decode");
        assert_eq!((decoded.w, decoded.h), (2, 1));
        assert_eq!(decoded.px[0], 0);
        assert_eq!(decoded.px[1], argb(0xFF, 1, 2, 3));
    }

    #[test]
    fn garbage_bytes_are_an_error() {
        assert!(decode(b"not an image").is_err());
    }
}
