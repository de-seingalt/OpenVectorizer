use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Error;
use crate::palette::PaletteEdit;

/// Kind of source image; selects sensible defaults for the other options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Preset {
    /// Logos, icons, flat illustrations: few colors, sharp corners.
    #[default]
    Logo,
    /// Detailed illustrations and drawings.
    Illustration,
    /// Photographs: many colors, smooth shapes.
    Photo,
}

/// How color regions are layered in the SVG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Stacking {
    /// Shapes are piled on top of each other (smaller file, easy to edit).
    #[default]
    Stacked,
    /// Each region has holes cut for the shapes above it (no overlap).
    Cutout,
}

/// User-facing vectorization options. Keep this small: every field is a UI control.
/// Values outside their documented range are rejected by [`Options::validated`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Options {
    pub preset: Preset,
    /// 0.0 = coarse, 1.0 = maximum detail.
    pub detail: f32,
    /// 0.0 = keep every corner sharp, 1.0 = round everything.
    pub smoothness: f32,
    /// Discard shapes smaller than this many pixels (0..=128).
    pub min_area_px: u32,
    pub stacking: Stacking,
    /// Maximum number of colors: 0 = automatic, otherwise 2..=64. Ignored by the Photo preset.
    pub max_colors: u32,
    /// Manual regrouping of the colors (palette editor). Ignored when it was made for other
    /// source colors (its key no longer matches).
    pub palette_edit: Option<PaletteEdit>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            preset: Preset::Logo,
            detail: 0.6,
            smoothness: 0.4,
            min_area_px: 4,
            stacking: Stacking::Stacked,
            max_colors: 0,
            palette_edit: None,
        }
    }
}

impl Options {
    /// # Errors
    /// Returns [`Error::InvalidOption`] for out-of-range values.
    pub fn validated(&self) -> Result<Self, Error> {
        let unit = |name: &str, v: f32| {
            if (0.0..=1.0).contains(&v) {
                Ok(())
            } else {
                Err(Error::InvalidOption(format!("{name} must be in 0..=1, got {v}")))
            }
        };
        unit("detail", self.detail)?;
        unit("smoothness", self.smoothness)?;
        if self.min_area_px > 128 {
            return Err(Error::InvalidOption(format!("minAreaPx must be <= 128, got {}", self.min_area_px)));
        }
        if self.max_colors == 1 || self.max_colors > 64 {
            return Err(Error::InvalidOption(format!(
                "maxColors must be 0 or 2..=64, got {}",
                self.max_colors
            )));
        }
        if let Some(edit) = &self.palette_edit {
            edit.validate()?;
        }
        Ok(self.clone())
    }
}
