//! Image encoding shared by the converters: lossless WebP and PNG for RGBA sheets.
//! The C# version used ImageSharp's WebP encoder (Lossless, Level6, alpha preserved);
//! here the pure-Rust `image-webp` lossless encoder keeps every pixel identical.

use std::io::Cursor;

use anyhow::Result;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{ImageEncoder, RgbaImage};

/// libwebp lossless at quality 100 with `exact` (RGB under transparent pixels kept), the
/// settings of the C# ImageSharp encoder (Lossless, Level6, TransparentColorMode.Preserve).
/// The method is config.ini `webp_method` (default 6, like the C#; lower is faster, larger).
/// Falls back to the pure-Rust encoder if libwebp refuses the image.
pub fn encode_webp_lossless(image: &RgbaImage) -> Result<Vec<u8>> {
    if let Ok(mut config) = webp::WebPConfig::new() {
        config.lossless = 1;
        config.method = crate::settings::webp_method();
        config.quality = 100.0;
        config.exact = 1;
        let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
        if let Ok(memory) = encoder.encode_advanced(&config) {
            return Ok(memory.to_vec());
        }
    }
    let mut bytes = Vec::new();
    WebPEncoder::new_lossless(&mut bytes).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(bytes)
}

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Default, FilterType::Adaptive).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(bytes)
}

/// Encodes for a sheet extension (".webp" or ".png").
pub fn encode_for_extension(image: &RgbaImage, extension: &str) -> Result<Vec<u8>> {
    if extension.eq_ignore_ascii_case(".webp") {
        encode_webp_lossless(image)
    } else {
        encode_png(image)
    }
}

/// Decodes any supported image (png, webp, gif, jpeg) into RGBA.
pub fn decode_rgba(bytes: &[u8]) -> Result<RgbaImage> {
    Ok(image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?.decode()?.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webp_and_png_are_lossless() {
        let mut image = RgbaImage::new(7, 5);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgba([x as u8 * 30, y as u8 * 40, 200, (x * y) as u8 * 7]);
        }
        assert_eq!(decode_rgba(&encode_webp_lossless(&image).unwrap()).unwrap(), image);
        assert_eq!(decode_rgba(&encode_png(&image).unwrap()).unwrap(), image);
    }
}
