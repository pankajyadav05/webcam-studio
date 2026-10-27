//! The Webcam Studio processing engine: turns a camera frame into the finished output frame and
//! cleans microphone audio.
//!
//! Video steps, in order: framing (zoom, pan, mirror, or auto-frame), the touch-ups, then the
//! background effect. Audio goes
//! through the denoiser picked by the noise mode. The engine has no threads or devices of its
//! own; the app calls [`Engine::process_video`] and [`Engine::process_audio`] from its loops.

pub mod appearance;
pub mod autoframe;
pub mod away;
pub mod composite;
pub mod eyecontact;
pub mod framing;
pub mod gaze;
pub mod matte;
pub mod showtell;
pub mod studio_light;

use studio_audio::{Denoiser, denoiser_for};
use studio_core::frame::{AudioChunk, VideoFrame};
use studio_core::settings::{BackgroundMode, Framing, Settings};
use studio_segmentation::{FaceBox, FullForegroundSegmenter, Mask, Segmenter, Sighting, Want};

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
    /// Background blur and replacement; keeps its working memory from frame to frame.
    compositor: composite::Compositor,
    /// Smooths person masks from a real model over time.
    matte: matte::MatteFilter,
    /// Capture time of the frame behind the last model mask, for the smoothing step.
    last_mask_us: Option<u64>,
    /// The last finished mask from the model, reused if one mask fails.
    last_mask: Option<Mask>,
    /// Follows the faces when auto-frame is on.
    framer: autoframe::AutoFramer,
    /// The framing used for the last picture (auto-frame's, when it is on).
    framing_in_use: Framing,
    /// Skin smoothing, low light and warmth.
    look: appearance::Look,
    /// Lights the face when Studio Light is on.
    light: studio_light::StudioLight,
    /// Show-and-tell: notices something held up to the camera.
    show: showtell::ShowTell,
    /// Eye contact: lifts a downward look.
    eye_contact: eyecontact::EyeContact,
    /// Be right back: on, the card drawn by the window, and the still made when it started.
    away: bool,
    away_card: Option<VideoFrame>,
    away_still: Option<VideoFrame>,
    away_frame: Option<VideoFrame>,
}

/// Returns the raw person mask for a frame, or `None` if the model failed on it.
pub type PersonModel<'a> = &'a mut dyn FnMut(&VideoFrame) -> Option<Mask>;

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
            compositor: composite::Compositor::new(),
            matte: matte::MatteFilter::new(),
            last_mask_us: None,
            last_mask: None,
            framer: autoframe::AutoFramer::new(),
            framing_in_use: Framing::default(),
            look: appearance::Look::new(),
            light: studio_light::StudioLight::new(),
            show: showtell::ShowTell::new(),
            eye_contact: eyecontact::EyeContact::new(),
            away: false,
            away_card: None,
            away_still: None,
            away_frame: None,
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

    /// Takes in the faces found in a camera picture of `width` × `height` taken at
    /// `timestamp_us`, for auto-frame. Faces are in shares of the picture, not mirrored.
    pub fn observe_faces(&mut self, faces: &[FaceBox], width: u32, height: u32, timestamp_us: u64) {
        let sighting = Sighting {
            faces: faces.to_vec(),
            ..Sighting::default()
        };
        self.observe(&sighting, width, height, timestamp_us);
    }

    /// Takes in what Apple Vision saw in a camera picture of `width` × `height` taken at
    /// `timestamp_us`: faces (auto-frame, Studio Light), hands (Show-and-tell) and eyes
    /// (Eye contact). Everything is in shares of the picture, not mirrored.
    pub fn observe(&mut self, seen: &Sighting, width: u32, height: u32, timestamp_us: u64) {
        if self.settings.framing.auto_frame {
            let out = (
                self.settings.output_width.max(1),
                self.settings.output_height.max(1),
            );
            self.framer
                .observe(&seen.faces, (width, height), out, timestamp_us);
        }
        if self.settings.appearance.studio_light > 0.001 && height > 0 {
            self.light
                .observe(&seen.faces, width as f32 / height as f32, timestamp_us);
        }
        if self.settings.framing.show_and_tell {
            self.show.observe(&seen.hands, &seen.faces, timestamp_us);
        }
        if self.settings.appearance.eye_contact {
            self.eye_contact.observe(&seen.eyes, timestamp_us);
        }
    }

    /// What the engine needs Apple Vision to look for now, if anything.
    pub fn wants(&self) -> Option<Want> {
        let s = &self.settings;
        let want = Want {
            hands: s.framing.show_and_tell,
            eyes: s.appearance.eye_contact,
        };
        let faces = s.framing.auto_frame || s.appearance.studio_light > 0.001;
        (faces || want.hands || want.eyes).then_some(want)
    }

    /// Whether the engine uses faces now.
    pub fn wants_faces(&self) -> bool {
        self.wants().is_some()
    }

    /// The framing used for the last picture. With auto-frame on, this is where it moved to.
    pub fn framing_in_use(&self) -> &Framing {
        &self.framing_in_use
    }

    /// Sets the picture shown behind the person in Replace mode, or clears it with `None`.
    ///
    /// The engine does not read files: the app loads the file named by
    /// `BackgroundMode::Replace::image_path` and passes the decoded frame here. Until an image
    /// is set, Replace mode leaves the picture unchanged.
    /// Be right back: while on, the output is a still of the last picture with the card on
    /// top, and the live camera is not used.
    pub fn set_away(&mut self, away: bool) {
        if away != self.away {
            self.away = away;
            self.away_still = None;
            self.away_frame = None;
        }
    }

    /// Whether Be right back is on.
    pub fn is_away(&self) -> bool {
        self.away
    }

    /// The card shown while away: BGRA with premultiplied colour and real alpha, at the
    /// output size.
    pub fn set_away_card(&mut self, card: Option<VideoFrame>) {
        self.away_card = card;
        self.away_frame = None;
    }

    pub fn set_replacement_image(&mut self, image: Option<VideoFrame>) {
        self.replacement = image;
        self.replacement_scaled = None;
    }

    /// Turns a camera frame into an output frame: framing first, then the touch-ups, then the
    /// background effect. The output is BGRA at the configured output size and carries the
    /// input's timestamp.
    ///
    /// With `Framing::auto_frame` on, the zoom and pan come from the faces given to
    /// [`Engine::observe_faces`] instead of the settings.
    pub fn process_video(&mut self, input: &VideoFrame) -> VideoFrame {
        self.render(input, None)
    }

    /// Like [`Engine::process_video`], with masks from a real person model instead of the
    /// built-in segmenter. The model sees the framed picture; its masks are smoothed over time
    /// and shaped by `Settings::cutout` the same way as in the window.
    pub fn process_video_with(&mut self, input: &VideoFrame, model: PersonModel<'_>) -> VideoFrame {
        self.render(input, Some(model))
    }

    fn person_mask(&mut self, frame: &VideoFrame, model: Option<PersonModel<'_>>) -> Mask {
        let Some(model) = model else {
            return self.segmenter.segment(frame);
        };
        let Some(raw) = model(frame) else {
            return match &self.last_mask {
                Some(mask) if (mask.width, mask.height) == (frame.width, frame.height) => {
                    mask.clone()
                }
                _ => Mask::full(frame.width, frame.height),
            };
        };
        let elapsed_ms = match self.last_mask_us {
            Some(last) if frame.timestamp_us > last => (frame.timestamp_us - last) as f32 / 1000.0,
            _ => 33.333,
        };
        self.last_mask_us = Some(frame.timestamp_us);
        let params = matte::Params::from(self.settings.cutout);
        let grid = matte::luma_grid(
            frame,
            (raw.width as usize / 4).max(1),
            (raw.height as usize / 4).max(1),
        );
        let alpha = self
            .matte
            .apply(&raw, elapsed_ms, Some(&grid), params.still_ms);
        let mask = matte::finish(
            alpha,
            raw.width as usize,
            raw.height as usize,
            frame.width as usize,
            frame.height as usize,
            &params,
        );
        self.last_mask = Some(mask.clone());
        mask
    }

    fn render(&mut self, input: &VideoFrame, model: Option<PersonModel<'_>>) -> VideoFrame {
        if !self.away {
            return self.render_live(input, model);
        }
        if self.away_still.is_none() {
            // The last live picture, once, as the still.
            let live = self.render_live(input, model);
            self.away_still = Some(away::freeze(&live));
        }
        if self.away_frame.is_none() {
            if let Some(still) = &self.away_still {
                self.away_frame = Some(away::compose(still, self.away_card.as_ref()));
            }
        }
        let mut out = self
            .away_frame
            .clone()
            .unwrap_or_else(|| VideoFrame::new_bgra(input.width, input.height, 0));
        out.timestamp_us = input.timestamp_us;
        out
    }

    fn render_live(&mut self, input: &VideoFrame, model: Option<PersonModel<'_>>) -> VideoFrame {
        let out_w = self.settings.output_width.max(1);
        let out_h = self.settings.output_height.max(1);
        // Step 1: framing. Auto-frame and Show-and-tell move the picture smoothly; with both
        // off (or once Show-and-tell has handed back), the manual framing is used as it is.
        let auto = self.settings.framing.auto_frame;
        let focus = if self.settings.framing.show_and_tell {
            self.show.focus()
        } else {
            self.show.reset();
            None
        };
        self.framer.set_aim(auto, focus);
        let framing = if auto || focus.is_some() || self.framer.busy() {
            self.framer.step(
                &self.settings.framing,
                (input.width, input.height),
                (out_w, out_h),
                input.timestamp_us,
            )
        } else {
            self.framer.reset();
            self.settings.framing.clone()
        };
        let rect = crop_rect(input.width, input.height, out_w, out_h, &framing);
        let mut frame = framing::scale_frame(input, &rect, out_w, out_h, framing.mirror);
        frame.timestamp_us = input.timestamp_us;
        self.framing_in_use = framing;

        // Step 2: eye contact, the touch-ups, then Studio Light on the face.
        if self.settings.appearance.eye_contact {
            self.eye_contact.apply(
                &mut frame,
                (input.width, input.height),
                &rect,
                self.framing_in_use.mirror,
                input.timestamp_us,
            );
        } else {
            self.eye_contact.reset();
        }
        self.look.apply(&mut frame, &self.settings.appearance);
        let strength = self.settings.appearance.studio_light;
        if strength > 0.001 {
            if let Some((spot, presence)) = self.light.step(input.timestamp_us) {
                let spot = studio_light::spot_in_output(
                    spot,
                    (input.width, input.height),
                    &rect,
                    (out_w, out_h),
                    self.framing_in_use.mirror,
                );
                studio_light::apply(&mut frame, spot, strength, presence);
            }
        } else {
            self.light.reset();
        }

        // Step 3: background.
        match self.settings.background.clone() {
            BackgroundMode::None => {
                // Start fresh when an effect is turned on again.
                self.matte.reset();
                self.last_mask_us = None;
                self.last_mask = None;
                frame
            }
            BackgroundMode::Blur { radius } => {
                let mask = self.person_mask(&frame, model);
                if composite::is_all_foreground(&mask) {
                    return frame;
                }
                self.compositor.blur_background(frame, &mask, radius)
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
                        Some(framing::scale_frame(image, &fit, out_w, out_h, false));
                }
                if self.replacement_scaled.is_none() {
                    return frame;
                }
                let mask = self.person_mask(&frame, model);
                if composite::is_all_foreground(&mask) {
                    return frame;
                }
                let Some(background) = &self.replacement_scaled else {
                    return frame;
                };
                self.compositor.replace_background(frame, background, &mask)
            }
        }
    }

    /// Cleans one chunk of microphone audio in place using the current noise mode.
    pub fn process_audio(&mut self, chunk: &mut AudioChunk) {
        self.denoiser.process(chunk);
    }

    fn clamp_settings(mut settings: Settings) -> Settings {
        settings.framing = settings.framing.clamped();
        settings.appearance = settings.appearance.clamped();
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

    #[test]
    fn blur_with_a_person_model_keeps_the_person_and_blurs_the_rest() {
        let mut engine = Engine::new(Settings {
            background: BackgroundMode::Blur { radius: 4 },
            ..small_settings()
        });
        // The model says the left half is the person.
        let mut model = |frame: &VideoFrame| {
            let (w, h) = (frame.width as usize, frame.height as usize);
            Some(Mask {
                width: frame.width,
                height: frame.height,
                alpha: (0..w * h)
                    .map(|i| if i % w < w / 2 { 255 } else { 0 })
                    .collect(),
            })
        };
        let input = checkerboard(32, 18);
        let out = engine.process_video_with(&input, &mut model);
        assert_eq!(out.pixel(2, 9), input.pixel(2, 9), "person pixel changed");
        let right = out.pixel(28, 9)[0];
        assert!(right > 60 && right < 195, "background not blurred: {right}");
        // A failed mask reuses the last good one instead of dropping the effect.
        let again = engine.process_video_with(&input, &mut |_: &VideoFrame| None);
        assert_eq!(again.pixel(28, 9), out.pixel(28, 9));
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
    fn auto_frame_follows_the_faces_and_manual_framing_does_not() {
        let mut settings = Settings {
            output_width: 64,
            output_height: 36,
            ..Settings::default()
        };
        settings.framing.auto_frame = true;
        let mut engine = Engine::new(settings.clone());
        let input = checkerboard(128, 72);
        let face = FaceBox {
            x: 0.65,
            y: 0.25,
            width: 0.1,
            height: 0.18,
            confidence: 1.0,
        };
        for i in 0..120u64 {
            let mut frame = input.clone();
            frame.timestamp_us = i * 33_333;
            engine.observe_faces(&[face], 128, 72, frame.timestamp_us);
            engine.process_video(&frame);
        }
        let auto = engine.framing_in_use().clone();
        assert!(auto.zoom > 1.3 && auto.pan_x > 0.3, "{auto:?}");

        // Turning it off glides back to the manual framing, then uses it exactly.
        settings.framing.auto_frame = false;
        engine.update_settings(settings);
        for i in 120..300u64 {
            let mut frame = input.clone();
            frame.timestamp_us = i * 33_333;
            engine.process_video(&frame);
        }
        assert_eq!(engine.framing_in_use().zoom, 1.0);
    }

    #[test]
    fn touch_ups_change_the_picture_only_when_on() {
        let mut settings = small_settings();
        let input = checkerboard(32, 18);
        let mut engine = Engine::new(settings.clone());
        assert_eq!(engine.process_video(&input).data, input.data);
        settings.appearance.warmth = 1.0;
        engine.update_settings(settings);
        assert_ne!(engine.process_video(&input).data, input.data);
    }

    #[test]
    fn away_shows_a_still_and_never_the_live_camera() {
        let mut engine = Engine::new(small_settings());
        let first = checkerboard(32, 18);
        engine.set_away(true);
        assert!(engine.is_away());
        let still = engine.process_video(&first);
        assert_ne!(still.data, first.data, "blurred and darker");
        // A different live picture changes nothing while away.
        let mut other = VideoFrame::new_bgra(32, 18, 99);
        other.data.fill(255);
        let again = engine.process_video(&other);
        assert_eq!(again.data, still.data);
        assert_eq!(again.timestamp_us, 99);
        // The card goes on top; leaving shows the live camera again.
        let mut card = VideoFrame::new_bgra(32, 18, 0);
        card.set_pixel(0, 0, [10, 20, 30, 255]);
        engine.set_away_card(Some(card));
        assert_eq!(engine.process_video(&other).pixel(0, 0)[..3], [10, 20, 30]);
        engine.set_away(false);
        assert_eq!(engine.process_video(&first).data, first.data);
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
