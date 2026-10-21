//! User-adjustable settings for the camera pipeline.

use serde::{Deserialize, Serialize};

/// How the background behind the person is changed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum BackgroundMode {
    /// Leave the background as it is.
    #[default]
    None,
    /// Blur the background by the given radius.
    Blur {
        /// Blur radius in pixels.
        radius: u32,
    },
    /// Replace the background with an image file.
    Replace {
        /// Path to the replacement image.
        image_path: String,
    },
}

/// How much microphone noise is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseMode {
    /// No noise removal.
    #[default]
    Off,
    /// Light noise removal.
    Light,
    /// Strong noise removal.
    Strong,
}

/// Camera framing: zoom and position of the person in the frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Framing {
    /// Zoom level: 1.0 shows the full frame, up to 4.0.
    pub zoom: f32,
    /// Horizontal pan from -1.0 (left) to 1.0 (right); 0.0 is centred.
    pub pan_x: f32,
    /// Vertical pan from -1.0 (up) to 1.0 (down); 0.0 is centred.
    pub pan_y: f32,
    /// Flip the image left to right.
    pub mirror: bool,
    /// Follow the person automatically.
    pub auto_frame: bool,
}

impl Default for Framing {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            mirror: false,
            auto_frame: false,
        }
    }
}

impl Framing {
    /// Returns a copy with zoom limited to 1.0..=4.0 and pans to -1.0..=1.0.
    /// A NaN value is replaced by its default.
    pub fn clamped(&self) -> Framing {
        Framing {
            zoom: clamp_or(self.zoom, 1.0, 4.0, 1.0),
            pan_x: clamp_or(self.pan_x, -1.0, 1.0, 0.0),
            pan_y: clamp_or(self.pan_y, -1.0, 1.0, 0.0),
            mirror: self.mirror,
            auto_frame: self.auto_frame,
        }
    }
}

/// Clamps `value` to `lo..=hi`, or returns `nan_value` if `value` is NaN.
fn clamp_or(value: f32, lo: f32, hi: f32, nan_value: f32) -> f32 {
    if value.is_nan() {
        nan_value
    } else {
        value.clamp(lo, hi)
    }
}

/// All settings the pipeline reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Zoom and position of the person.
    pub framing: Framing,
    /// Background effect.
    pub background: BackgroundMode,
    /// Microphone noise removal.
    pub noise: NoiseMode,
    /// Output width in pixels.
    pub output_width: u32,
    /// Output height in pixels.
    pub output_height: u32,
    /// Output frames per second.
    pub output_fps: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            framing: Framing::default(),
            background: BackgroundMode::default(),
            noise: NoiseMode::default(),
            output_width: 1280,
            output_height: 720,
            output_fps: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamped_limits_zoom_and_pan() {
        let framing = Framing {
            zoom: 9.0,
            pan_x: -3.0,
            pan_y: 2.5,
            mirror: true,
            auto_frame: true,
        };
        let c = framing.clamped();
        assert_eq!(c.zoom, 4.0);
        assert_eq!(c.pan_x, -1.0);
        assert_eq!(c.pan_y, 1.0);
        assert!(c.mirror);
        assert!(c.auto_frame);

        let low = Framing {
            zoom: 0.2,
            ..Framing::default()
        }
        .clamped();
        assert_eq!(low.zoom, 1.0);
    }

    #[test]
    fn clamped_replaces_nan_with_default() {
        let framing = Framing {
            zoom: f32::NAN,
            pan_x: f32::NAN,
            pan_y: f32::NAN,
            mirror: false,
            auto_frame: false,
        };
        assert_eq!(framing.clamped(), Framing::default());
    }

    #[test]
    fn settings_json_round_trip() {
        let settings = Settings {
            framing: Framing {
                zoom: 2.0,
                pan_x: 0.5,
                pan_y: -0.25,
                mirror: true,
                auto_frame: false,
            },
            background: BackgroundMode::Replace {
                image_path: "/tmp/bg.png".to_string(),
            },
            noise: NoiseMode::Strong,
            output_width: 1920,
            output_height: 1080,
            output_fps: 60,
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn empty_json_is_default_settings() {
        let parsed: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, Settings::default());
        assert_eq!(parsed.output_width, 1280);
        assert_eq!(parsed.output_height, 720);
        assert_eq!(parsed.output_fps, 30);
    }

    #[test]
    fn blur_background_parses() {
        let parsed: Settings =
            serde_json::from_str(r#"{"background":{"mode":"blur","radius":8}}"#).unwrap();
        assert_eq!(parsed.background, BackgroundMode::Blur { radius: 8 });
        assert_eq!(parsed.output_fps, 30);
    }
}
