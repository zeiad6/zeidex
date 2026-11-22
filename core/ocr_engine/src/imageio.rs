//! Raster input, decoded the way the camera meant it to be seen.

use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader, metadata::Orientation};

use crate::error::OcrResult;

/// Opens a raster image and applies its EXIF or TIFF orientation tag.
///
/// A phone photo is stored in sensor order with a tag saying how to turn it
/// upright; `image::open` ignores that tag, so a portrait page could reach
/// detection sideways and read as nothing. The preview and the pipeline both
/// go through here, so the boxes and the pixels stay in one frame.
///
/// A missing or malformed tag is not a reason to refuse the file: the decoder
/// reports no transform and the image is used as stored.
pub fn open_oriented(path: &Path) -> OcrResult<DynamicImage> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::GenericImageView;

    use super::open_oriented;

    /// The camera stored this page rotated 90° CCW with `Orientation=6`, so the
    /// upright frame is 320×200 with the black block top-left and the red block
    /// bottom-right. Decoding without the tag yields 200×320 and fails here.
    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("photo-rotated.jpg")
    }

    fn close(channel: u8, expected: u8) -> bool {
        channel.abs_diff(expected) <= 24
    }

    #[test]
    fn a_rotated_phone_photo_is_decoded_upright() {
        // The bytes on disk really are sideways: a plain `image::open` ignores
        // the tag and reports the sensor frame. This keeps the fixture honest —
        // if someone re-saves it upright, the test stops proving anything.
        let raw = image::open(fixture()).expect("fixture opens");
        assert_eq!(raw.dimensions(), (200, 320), "stored frame is sideways");

        let image = open_oriented(&fixture()).expect("fixture opens");
        assert_eq!(image.dimensions(), (320, 200));

        let black = image.get_pixel(10, 10);
        assert!(
            close(black[0], 0) && close(black[1], 0) && close(black[2], 0),
            "top-left should be the black block, got {black:?}"
        );

        let red = image.get_pixel(300, 185);
        assert!(
            close(red[0], 220) && close(red[1], 30) && close(red[2], 30),
            "bottom-right should be the red block, got {red:?}"
        );

        let white = image.get_pixel(300, 10);
        assert!(
            close(white[0], 255) && close(white[1], 255) && close(white[2], 255),
            "top-right should be blank paper, got {white:?}"
        );
    }
}
