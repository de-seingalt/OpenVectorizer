use crate::Error;

/// Maximum accepted size (pixels). Larger images must be downscaled by the caller.
pub const MAX_PIXELS: u64 = 16_000_000;

/// An 8-bit RGBA image, row-major, non-premultiplied alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl RgbaImage {
    /// # Errors
    /// Fails if a dimension is zero, the image is too large, or `data.len() != width*height*4`.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self, Error> {
        let pixels = u64::from(width) * u64::from(height);
        if pixels == 0 {
            return Err(Error::InvalidImage("width and height must be > 0".into()));
        }
        if pixels > MAX_PIXELS {
            return Err(Error::InvalidImage(format!("{pixels} pixels exceeds the {MAX_PIXELS} limit")));
        }
        if data.len() as u64 != pixels * 4 {
            return Err(Error::InvalidImage(format!(
                "buffer has {} bytes, expected {} for {width}x{height} RGBA",
                data.len(),
                pixels * 4
            )));
        }
        Ok(Self { width, height, data })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Raw RGBA bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}
