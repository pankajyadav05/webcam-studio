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
    /// Zoom in on something held up to the camera.
    pub show_and_tell: bool,
}

impl Default for Framing {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            mirror: false,
            auto_frame: false,
            show_and_tell: false,
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
            show_and_tell: self.show_and_tell,
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

/// The Advanced cutout sliders, each 0..=1 with the shipped look at 0.5.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cutout {
    /// "Cut uncertain areas": higher removes more of what the model is unsure about.
    pub cutoff: f32,
    /// "Edge softness": higher gives a softer edge.
    pub softness: f32,
    /// "Steadiness": higher smooths the mask over a longer time.
    pub steadiness: f32,
}

impl Default for Cutout {
    fn default() -> Self {
        Self {
            cutoff: 0.5,
            softness: 0.5,
            steadiness: 0.5,
        }
    }
}

impl Cutout {
    /// Each value clamped to 0..=1; NaN takes the default.
    pub fn clamped(self) -> Self {
        Self {
            cutoff: clamp_or(self.cutoff, 0.0, 1.0, 0.5),
            softness: clamp_or(self.softness, 0.0, 1.0, 0.5),
            steadiness: clamp_or(self.steadiness, 0.0, 1.0, 0.5),
        }
    }
}

/// Touch-ups to the person's look, applied to the camera picture before the background.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// "Touch up": how much skin is smoothed, 0 (off) to 1.
    pub smoothing: f32,
    /// Brighten a dark picture automatically.
    pub low_light: bool,
    /// Colour temperature, -1 (cooler) to 1 (warmer); 0 leaves the colours alone.
    pub warmth: f32,
    /// "Studio Light": lights the face and dims the room around it, 0 (off) to 1.
    pub studio_light: f32,
    /// Redraw the eyes looking at the camera while you read the screen.
    pub eye_contact: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            smoothing: 0.0,
            low_light: false,
            warmth: 0.0,
            studio_light: 0.0,
            eye_contact: false,
        }
    }
}

impl Appearance {
    /// Each value clamped to its range; NaN takes the default.
    pub fn clamped(self) -> Self {
        Self {
            smoothing: clamp_or(self.smoothing, 0.0, 1.0, 0.0),
            low_light: self.low_light,
            warmth: clamp_or(self.warmth, -1.0, 1.0, 0.0),
            studio_light: clamp_or(self.studio_light, 0.0, 1.0, 0.0),
            eye_contact: self.eye_contact,
        }
    }

    /// Whether any touch-up changes the picture.
    pub fn is_active(&self) -> bool {
        self.changes_colour() || self.studio_light > 0.001 || self.eye_contact
    }

    /// Whether smoothing, low light or warmth is on (the steps that need no face).
    pub fn changes_colour(&self) -> bool {
        self.smoothing > 0.001 || self.low_light || self.warmth.abs() > 0.001
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
    /// How the person is cut out in Blur and Replace.
    pub cutout: Cutout,
    /// Skin smoothing, low-light correction and warmth.
    pub appearance: Appearance,
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
            cutout: Cutout::default(),
            appearance: Appearance::default(),
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
            show_and_tell: false,
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
            show_and_tell: false,
        };
        assert_eq!(framing.clamped(), Framing::default());
    }

    #[test]
    fn cutout_is_clamped_and_missing_fields_take_defaults() {
        let cutout = Cutout {
            cutoff: 2.0,
            softness: f32::NAN,
            steadiness: -1.0,
        }
        .clamped();
        assert_eq!(
            cutout,
            Cutout {
                cutoff: 1.0,
                softness: 0.5,
                steadiness: 0.0
            }
        );
        let old: Settings = serde_json::from_str(r#"{"output_fps":30}"#).unwrap();
        assert_eq!(old.cutout, Cutout::default());
    }

    #[test]
    fn appearance_is_clamped_and_old_settings_have_none() {
        let a = Appearance {
            smoothing: 3.0,
            low_light: true,
            warmth: f32::NAN,
            studio_light: 2.0,
            eye_contact: false,
        }
        .clamped();
        assert_eq!(a.studio_light, 1.0);
        assert_eq!((a.smoothing, a.warmth), (1.0, 0.0));
        assert!(a.is_active());
        let old: Settings = serde_json::from_str(r#"{"output_fps":30}"#).unwrap();
        assert_eq!(old.appearance, Appearance::default());
        assert!(!old.appearance.is_active());
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
                show_and_tell: true,
            },
            background: BackgroundMode::Replace {
                image_path: "/tmp/bg.png".to_string(),
            },
            noise: NoiseMode::Strong,
            output_width: 1920,
            output_height: 1080,
            output_fps: 60,
            cutout: Cutout {
                cutoff: 0.75,
                softness: 0.25,
                steadiness: 1.0,
            },
            appearance: Appearance {
                smoothing: 0.6,
                low_light: true,
                warmth: -0.25,
                studio_light: 0.5,
                eye_contact: true,
            },
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
