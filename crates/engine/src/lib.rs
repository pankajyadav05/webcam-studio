//! The Webcam Studio processing engine: turns a camera frame into the finished output frame and
//! cleans microphone audio.
//!
//! Video steps, in order: framing (zoom, pan, mirror), then the background effect. Audio goes
//! through the denoiser picked by the noise mode. The engine has no threads or devices of its
//! own; the app calls [`Engine::process_video`] and [`Engine::process_audio`] from its loops.

pub mod composite;
pub mod framing;

use studio_audio::{Denoiser, denoiser_for};
use studio_core::frame::{AudioChunk, VideoFrame};
use studio_core::settings::{BackgroundMode, Framing, Settings};
use studio_segmentation::{FullForegroundSegmenter, Segmenter};

pub use framing::{CropRect, crop_rect};

/// Runs the video and audio effects described by a [`Settings`] value.
pub struct Engine {
    settings: Settings,
    segmenter: Box<dyn Segmenter>,
    denoiser: Box<dyn Denoiser>,
    /// Image used as the background in Replace mode, as given by the app.
    replacement: Option<VideoFrame>,
    /// The replacement image scaled to the output size, kept so it is not rescaled every frame.
    replacement_scaled: Option<VideoFrame>,
}

impl Engine {
    /// Creates an engine with the full-foreground segmenter and the denoiser for
    /// `settings.noise`. The framing in `settings` is clamped to its allowed range.
    pub fn new(settings: Settings) -> Self {
        let settings = Self::clamp_settings(settings);
        let denoiser = denoiser_for(settings.noise);
        Self {
            settings,
            segmenter: Box::new(FullForegroundSegmenter),
            denoiser,
            replacement: None,
            replacement_scaled: None,
        }
    }

    /// Swaps in a different segmenter (for example a real model).
    pub fn with_segmenter(mut self, segmenter: Box<dyn Segmenter>) -> Self {
        self.segmenter = segmenter;
        self
    }

    /// The settings currently in use (with framing already clamped).
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Name of the segmenter in use.
    pub fn segmenter_name(&self) -> &'static str {
        self.segmenter.name()
    }

    /// Name of the denoiser in use.
    pub fn denoiser_name(&self) -> &'static str {
        self.denoiser.name()
    }

    /// Replaces the settings. The framing is clamped, and the denoiser is swapped only if the
    /// noise mode changed (so the gate keeps its state otherwise).
    pub fn update_settings(&mut self, settings: Settings) {
        let settings = Self::clamp_settings(settings);
        if settings.noise != self.settings.noise {
            self.denoiser = denoiser_for(settings.noise);
        }
        self.settings = settings;
    }

    /// Sets the picture shown behind the person in Replace mode, or clears it with `None`.
    ///
    /// The engine does not read files: the app loads the file named by
    /// `BackgroundMode::Replace::image_path` and passes the decoded frame here. Until an image
    /// is set, Replace mode leaves the picture unchanged.
    pub fn set_replacement_image(&mut self, image: Option<VideoFrame>) {
        self.replacement = image;
        self.replacement_scaled = None;
    }

    /// Turns a camera frame into an output frame: framing first, then the background effect.
    /// The output is BGRA at the configured output size and carries the input's timestamp.
    ///
    /// Auto-framing (`Framing::auto_frame`) is not implemented yet. It needs a face detector
    /// (MediaPipe Face Detection is planned) feeding a smoothed crop box, which would replace
    /// the manual zoom and pan used here.
    pub fn process_video(&mut self, input: &VideoFrame) -> VideoFrame {
        let out_w = self.settings.output_width.max(1);
        let out_h = self.settings.output_height.max(1);
        let framing = &self.settings.framing;

        // Step 1: framing.
        let rect = crop_rect(input.width, input.height, out_w, out_h, framing);
        let mut frame = framing::scale_bilinear(input, &rect, out_w, out_h, framing.mirror);
        frame.timestamp_us = input.timestamp_us;

        // Step 2: background.
        match &self.settings.background {
            BackgroundMode::None => frame,
            BackgroundMode::Blur { radius } => {
                let mask = self.segmenter.segment(&frame);
                if composite::is_all_foreground(&mask) {
                    return frame;
                }
                let blurred = composite::box_blur(&frame, *radius);
                composite::blend(&frame, &blurred, &mask)
            }
            BackgroundMode::Replace { .. } => {
                let Some(image) = &self.replacement else {
                    return frame;
                };
                let stale = self
                    .replacement_scaled
                    .as_ref()
                    .is_none_or(|s| s.width != out_w || s.height != out_h);
                if stale {
                    // Fill the output without stretching: centre-crop to the output's shape.
                    let fit =
                        crop_rect(image.width, image.height, out_w, out_h, &Framing::default());
                    self.replacement_scaled =
                        Some(framing::scale_bilinear(image, &fit, out_w, out_h, false));
                }
                let Some(background) = &self.replacement_scaled else {
                    return frame;
                };
                let mask = self.segmenter.segment(&frame);
                composite::blend(&frame, background, &mask)
            }
        }
    }

    /// Cleans one chunk of microphone audio in place using the current noise mode.
    pub fn process_audio(&mut self, chunk: &mut AudioChunk) {
        self.denoiser.process(chunk);
    }

    fn clamp_settings(mut settings: Settings) -> Settings {
        settings.framing = settings.framing.clamped();
        settings
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("settings", &self.settings)
            .field("segmenter", &self.segmenter.name())
            .field("denoiser", &self.denoiser.name())
            .field("has_replacement_image", &self.replacement.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::settings::NoiseMode;
    use studio_segmentation::Mask;

    /// Segmenter that returns the same mask value for every pixel.
    struct ConstSegmenter(u8);

    impl Segmenter for ConstSegmenter {
        fn segment(&mut self, frame: &VideoFrame) -> Mask {
            Mask {
                width: frame.width,
                height: frame.height,
                alpha: vec![self.0; frame.width as usize * frame.height as usize],
            }
        }

        fn name(&self) -> &'static str {
            "const"
        }
    }

    fn small_settings() -> Settings {
        Settings {
            output_width: 32,
            output_height: 18,
            ..Settings::default()
        }
    }

    fn checkerboard(w: u32, h: u32) -> VideoFrame {
        let mut f = VideoFrame::new_bgra(w, h, 9);
        for y in 0..h {
            for x in 0..w {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                f.set_pixel(x, y, [v, v, v, 255]);
            }
        }
        f
    }

    #[test]
    fn output_has_configured_size_and_timestamp() {
        let mut engine = Engine::new(small_settings());
        let mut input = VideoFrame::new_bgra(64, 48, 1234);
        input.data.fill(100);
        let out = engine.process_video(&input);
        assert_eq!((out.width, out.height), (32, 18));
        assert_eq!(out.stride, 32 * 4);
        assert_eq!(out.data.len(), 32 * 18 * 4);
        assert_eq!(out.timestamp_us, 1234);
    }

    #[test]
    fn identity_when_nothing_is_changed() {
        let mut engine = Engine::new(small_settings());
        let input = checkerboard(32, 18);
        assert_eq!(engine.process_video(&input).data, input.data);
    }

    #[test]
    fn mirror_reverses_a_gradient() {
        let mut settings = small_settings();
        settings.framing.mirror = true;
        let mut engine = Engine::new(settings);
        let mut input = VideoFrame::new_bgra(32, 18, 0);
        for y in 0..18 {
            for x in 0..32 {
                input.set_pixel(x, y, [(x * 8) as u8, 0, 0, 255]);
            }
        }
        let out = engine.process_video(&input);
        for y in 0..18 {
            for x in 0..32 {
                assert_eq!(out.pixel(x, y), input.pixel(31 - x, y));
            }
        }
    }

    #[test]
    fn high_zoom_shows_only_the_centre() {
        let mut settings = small_settings();
        settings.framing.zoom = 4.0;
        let mut engine = Engine::new(settings);
        let (w, h) = (64, 36);
        let mut input = VideoFrame::new_bgra(w, h, 0);
        for y in 0..h {
            for x in 0..w {
                let centre = (16..48).contains(&x) && (9..27).contains(&y);
                // BGRA: red is the third byte.
                input.set_pixel(
                    x,
                    y,
                    if centre {
                        [0, 0, 255, 255]
                    } else {
                        [255, 0, 0, 255]
                    },
                );
            }
        }
        // Zoom 1 sees the blue border; zoom 4 sees only the red middle.
        let mut wide = Engine::new(small_settings());
        assert_eq!(wide.process_video(&input).pixel(0, 0), [255, 0, 0, 255]);

        let out = engine.process_video(&input);
        for y in 0..18 {
            for x in 0..32 {
                assert_eq!(out.pixel(x, y), [0, 0, 255, 255], "at ({x}, {y})");
            }
        }
    }

    #[test]
    fn zoom_and_pan_are_clamped() {
        let mut settings = small_settings();
        settings.framing.zoom = 100.0;
        settings.framing.pan_x = f32::NAN;
        let engine = Engine::new(settings);
        assert_eq!(engine.settings().framing.zoom, 4.0);
        assert_eq!(engine.settings().framing.pan_x, 0.0);
    }

    #[test]
    fn blur_changes_checkerboard_only_with_background_mask() {
        let mut settings = small_settings();
        settings.background = BackgroundMode::Blur { radius: 2 };
        let input = checkerboard(32, 18);

        // Everything is background: the picture is blurred.
        let mut engine = Engine::new(settings.clone()).with_segmenter(Box::new(ConstSegmenter(0)));
        assert_eq!(engine.segmenter_name(), "const");
        let blurred = engine.process_video(&input);
        assert_ne!(blurred.data, input.data);

        // Everything is person: the picture is untouched.
        let mut engine = Engine::new(settings);
        assert_eq!(engine.segmenter_name(), "full-foreground");
        assert_eq!(engine.process_video(&input).data, input.data);
    }

    #[test]
    fn blur_keeps_the_person_sharp() {
        let mut settings = small_settings();
        settings.background = BackgroundMode::Blur { radius: 3 };
        let input = checkerboard(32, 18);
        let mut engine = Engine::new(settings).with_segmenter(Box::new(ConstSegmenter(255)));
        assert_eq!(engine.process_video(&input).data, input.data);
    }

    #[test]
    fn replace_uses_the_image_for_background() {
        let mut settings = small_settings();
        settings.background = BackgroundMode::Replace {
            image_path: "unused-by-engine.png".to_string(),
        };
        let mut engine = Engine::new(settings).with_segmenter(Box::new(ConstSegmenter(0)));

        let mut image = VideoFrame::new_bgra(10, 10, 0);
        for y in 0..10 {
            for x in 0..10 {
                image.set_pixel(x, y, [10, 200, 30, 255]);
            }
        }
        let input = checkerboard(32, 18);

        // Without an image the picture is left alone.
        assert_eq!(engine.process_video(&input).data, input.data);

        engine.set_replacement_image(Some(image));
        for _ in 0..2 {
            // The second pass uses the cached scaled image.
            let out = engine.process_video(&input);
            assert_eq!((out.width, out.height), (32, 18));
            for y in 0..18 {
                for x in 0..32 {
                    assert_eq!(out.pixel(x, y), [10, 200, 30, 255]);
                }
            }
        }

        engine.set_replacement_image(None);
        assert_eq!(engine.process_video(&input).data, input.data);
    }

    #[test]
    fn replace_keeps_the_person() {
        let mut settings = small_settings();
        settings.background = BackgroundMode::Replace {
            image_path: String::new(),
        };
        let mut engine = Engine::new(settings).with_segmenter(Box::new(ConstSegmenter(255)));
        engine.set_replacement_image(Some(VideoFrame::new_bgra(4, 4, 0)));
        let input = checkerboard(32, 18);
        assert_eq!(engine.process_video(&input).data, input.data);
    }

    #[test]
    fn replacement_follows_output_size_changes() {
        let mut settings = small_settings();
        settings.background = BackgroundMode::Replace {
            image_path: String::new(),
        };
        let mut engine = Engine::new(settings.clone()).with_segmenter(Box::new(ConstSegmenter(0)));
        let mut image = VideoFrame::new_bgra(8, 8, 0);
        image.data.fill(77);
        engine.set_replacement_image(Some(image));
        assert_eq!(engine.process_video(&checkerboard(32, 18)).width, 32);

        settings.output_width = 16;
        settings.output_height = 9;
        engine.update_settings(settings);
        let out = engine.process_video(&checkerboard(32, 18));
        assert_eq!((out.width, out.height), (16, 9));
        // The colour channels come from the image; alpha stays that of the camera frame.
        assert!(out.data.chunks(4).all(|px| px[..3] == [77, 77, 77]));
    }

    #[test]
    fn empty_input_gives_black_output() {
        let mut engine = Engine::new(small_settings());
        let out = engine.process_video(&VideoFrame::new_bgra(0, 0, 5));
        assert_eq!((out.width, out.height, out.timestamp_us), (32, 18, 5));
        assert!(out.data.iter().all(|&b| b == 0));
    }

    #[test]
    fn update_settings_switches_the_denoiser() {
        let mut engine = Engine::new(small_settings());
        assert_eq!(engine.denoiser_name(), "passthrough");

        let mut settings = small_settings();
        settings.noise = NoiseMode::Light;
        engine.update_settings(settings.clone());
        assert_eq!(engine.denoiser_name(), "noise-gate");
        assert_eq!(engine.settings().noise, NoiseMode::Light);

        settings.noise = NoiseMode::Off;
        engine.update_settings(settings);
        assert_eq!(engine.denoiser_name(), "passthrough");
    }

    #[test]
    fn process_audio_uses_the_denoiser() {
        let mut settings = small_settings();
        settings.noise = NoiseMode::Strong;
        let mut engine = Engine::new(settings);
        let mut chunk = AudioChunk {
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0.0001; 48_000],
            timestamp_us: 0,
        };
        engine.process_audio(&mut chunk);
        assert!(chunk.samples.last().unwrap().abs() < 0.0001);
        assert!(chunk.samples.iter().all(|s| s.is_finite()));
    }
}
