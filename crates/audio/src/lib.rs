//! Microphone noise removal.
//!
//! The planned denoiser is DeepFilterNet 3 (Apache-2.0 or MIT, written in Rust), with RNNoise as
//! a low-CPU mode. Until one of them is integrated, [`NoiseGate`] is a simple stand-in that
//! turns down the microphone when it is quiet; it learns how loud the background noise is and
//! only opens for sound clearly above it. [`denoiser_for`] picks the right one for a
//! [`NoiseMode`].

use studio_core::frame::AudioChunk;
use studio_core::settings::NoiseMode;

/// Something that cleans up microphone audio.
pub trait Denoiser: Send {
    /// Cleans `chunk` in place.
    fn process(&mut self, chunk: &mut AudioChunk);
    /// Short name of this denoiser, for logs and the settings screen.
    fn name(&self) -> &'static str;
}

/// Leaves the audio exactly as it is.
#[derive(Debug, Clone, Copy, Default)]
pub struct Passthrough;

impl Denoiser for Passthrough {
    fn process(&mut self, _chunk: &mut AudioChunk) {}

    fn name(&self) -> &'static str {
        "passthrough"
    }
}

/// Gain the gate closes to for Light, in dB.
const LIGHT_CLOSED_GAIN_DB: f32 = -12.0;
/// Gain the gate closes to for Strong, in dB.
const STRONG_CLOSED_GAIN_DB: f32 = -30.0;
/// Light: how far above the noise floor a block must be to open the gate, in dB.
const LIGHT_OPEN_MARGIN_DB: f32 = 6.0;
/// Light: a block below this margin over the noise floor starts the hold countdown, in dB.
const LIGHT_CLOSE_MARGIN_DB: f32 = 3.0;
/// Strong: how far above the noise floor a block must be to open the gate, in dB.
const STRONG_OPEN_MARGIN_DB: f32 = 9.0;
/// Strong: a block below this margin over the noise floor starts the hold countdown, in dB.
const STRONG_CLOSE_MARGIN_DB: f32 = 5.0;
/// Lowest value the noise floor can take, in dBFS. Also the absolute floor used by
/// [`NoiseGate::light`] and [`NoiseGate::strong`].
const MIN_FLOOR_DBFS: f32 = -90.0;
/// Level reported for a block of pure silence, in dBFS.
const SILENT_DBFS: f32 = -120.0;

/// How fast the gate opens, as a time constant in seconds.
const ATTACK_SECONDS: f32 = 0.002;
/// How fast the gate closes, as a time constant in seconds.
const RELEASE_SECONDS: f32 = 0.080;
/// How long the gate stays open after the sound drops, in seconds.
const HOLD_SECONDS: f32 = 0.150;
/// Time constant with which the noise floor follows a quieter block downward, in seconds.
const FLOOR_FALL_SECONDS: f32 = 0.050;
/// How fast the noise floor may rise, in dB per second. Slow, so speech does not raise it.
const FLOOR_RISE_DB_PER_SECOND: f32 = 3.0;

/// Settings that depend on the sample rate and channel count of the stream being processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamFormat {
    sample_rate: u32,
    channels: u16,
}

/// Adaptive noise gate.
///
/// The gate measures the loudness (RMS) of consecutive 10 ms blocks and keeps a running estimate
/// of the noise floor: the estimate follows a quieter block down within about 50 ms and creeps
/// up by only about 3 dB per second, so speech does not raise it. A block more than the "open
/// margin" above the noise floor opens the gate. Once open, the gate stays open until blocks have
/// stayed under the (lower) "close margin" for the hold time of about 150 ms. A closed gate turns
/// the sound down to a fixed closed gain instead of muting it. The gain moves toward its target
/// one sample at a time (about 2 ms to open, 80 ms to close), so there are no clicks.
///
/// Because the threshold follows the room, a quiet voice is still heard in a quiet room, and a
/// steady sound that never changes is treated as noise.
///
/// * [`NoiseGate::light`]: opens 6 dB above the noise floor and closes to -12 dB.
/// * [`NoiseGate::strong`]: opens 9 dB above the noise floor and closes to -30 dB.
///
/// Audio quieter than the absolute floor (-90 dBFS for both) is always treated as noise, and a
/// completely silent input keeps the gate closed.
///
/// The gate is streaming and causal: it decides from the samples it has already seen and keeps
/// its state between calls to [`Denoiser::process`]. The output depends only on the samples, not
/// on how they are split into chunks, so any chunk size gives the same result. The first
/// block of a stream starts the gate open with the noise floor set from that block. If the
/// sample rate or channel count changes between calls, the gate starts over.
///
/// A decision is made when a 10 ms block is complete, so a sound that starts is let through
/// about 10 to 15 ms later (up to about 20 ms if it starts at the very end of a block).
///
/// This is a stand-in until DeepFilterNet is integrated. It works with any channel count
/// (interleaved samples) and any sample rate. The gain is shared by all channels so the stereo
/// image does not shift. Samples that are NaN or infinite are replaced by silence. Chunks should
/// hold whole frames; a trailing partial frame is treated as a frame of its own.
#[derive(Debug, Clone)]
pub struct NoiseGate {
    /// Blocks quieter than this (dBFS) are always noise.
    absolute_floor_dbfs: f32,
    /// A block this far above the noise floor opens the gate (dB).
    open_margin_db: f32,
    /// A block under this margin above the noise floor counts towards closing the gate (dB).
    close_margin_db: f32,
    /// Linear gain the gate settles to when closed.
    closed_gain: f32,

    /// Format the running state belongs to; `None` before the first call.
    format: Option<StreamFormat>,
    /// Frames in one analysis block at the current sample rate.
    block_frames: usize,
    /// Number of quiet blocks the gate holds open for.
    hold_blocks: u32,
    /// Per-sample smoothing factor when the gain rises.
    attack: f32,
    /// Per-sample smoothing factor when the gain falls.
    release: f32,
    /// Per-block factor with which the noise floor follows a lower block.
    floor_fall: f32,
    /// Most the noise floor may rise per block (dB).
    floor_rise_db: f32,

    /// Sum of squares of the samples in the current block.
    block_energy: f64,
    /// Frames seen in the current block.
    block_count: usize,
    /// Noise floor in dBFS; `None` until the first block is complete.
    noise_floor_dbfs: Option<f32>,
    /// Whether the gate is open (as decided at the last block).
    open: bool,
    /// Quiet blocks left before an open gate closes.
    hold_left: u32,
    /// Gain the smoothing moves towards.
    target_gain: f32,
    /// Gain currently applied (1.0 = unchanged).
    gain: f32,
}

impl NoiseGate {
    /// Creates a gate with the Light settings and an absolute floor of `floor_dbfs`: audio
    /// quieter than that (for example -90.0) is always treated as noise, whatever the room
    /// sounds like. Values are limited to -120..=0 dBFS; a value that is not a number is treated
    /// as -120 dBFS.
    pub fn new(floor_dbfs: f32) -> Self {
        Self::with_settings(
            floor_dbfs,
            LIGHT_OPEN_MARGIN_DB,
            LIGHT_CLOSE_MARGIN_DB,
            LIGHT_CLOSED_GAIN_DB,
        )
    }

    /// Gate for [`NoiseMode::Light`]: opens 6 dB above the noise floor, closes to -12 dB.
    pub fn light() -> Self {
        Self::new(MIN_FLOOR_DBFS)
    }

    /// Gate for [`NoiseMode::Strong`]: opens 9 dB above the noise floor, closes to -30 dB.
    pub fn strong() -> Self {
        Self::with_settings(
            MIN_FLOOR_DBFS,
            STRONG_OPEN_MARGIN_DB,
            STRONG_CLOSE_MARGIN_DB,
            STRONG_CLOSED_GAIN_DB,
        )
    }

    fn with_settings(
        floor_dbfs: f32,
        open_margin_db: f32,
        close_margin_db: f32,
        closed_gain_db: f32,
    ) -> Self {
        let absolute_floor_dbfs = if floor_dbfs.is_nan() {
            SILENT_DBFS
        } else {
            floor_dbfs.clamp(SILENT_DBFS, 0.0)
        };
        Self {
            absolute_floor_dbfs,
            open_margin_db,
            close_margin_db,
            closed_gain: 10f32.powf(closed_gain_db / 20.0),
            format: None,
            block_frames: 1,
            hold_blocks: 0,
            attack: 1.0,
            release: 1.0,
            floor_fall: 1.0,
            floor_rise_db: 0.0,
            block_energy: 0.0,
            block_count: 0,
            noise_floor_dbfs: None,
            open: true,
            hold_left: 0,
            target_gain: 1.0,
            gain: 1.0,
        }
    }

    /// Gain applied to the most recent sample (1.0 = unchanged, small = gate closed).
    pub fn current_gain(&self) -> f32 {
        self.gain
    }

    /// The gate's current estimate of the background noise, in dBFS, or `None` before the first
    /// 10 ms block has been measured.
    pub fn noise_floor_dbfs(&self) -> Option<f32> {
        self.noise_floor_dbfs
    }

    /// Starts over for a stream with the given format: gate open, no noise floor yet.
    fn start_stream(&mut self, sample_rate: u32, channels: u16) {
        let rate = sample_rate.max(1);
        let block_frames = ((rate / 100) as usize).max(1);
        let block_seconds = block_frames as f32 / rate as f32;
        self.format = Some(StreamFormat {
            sample_rate,
            channels,
        });
        self.block_frames = block_frames;
        self.hold_blocks = (HOLD_SECONDS / block_seconds).ceil().max(1.0) as u32;
        self.attack = smoothing(ATTACK_SECONDS, rate as f32);
        self.release = smoothing(RELEASE_SECONDS, rate as f32);
        self.floor_fall = smoothing(FLOOR_FALL_SECONDS, 1.0 / block_seconds);
        self.floor_rise_db = FLOOR_RISE_DB_PER_SECOND * block_seconds;
        self.block_energy = 0.0;
        self.block_count = 0;
        self.noise_floor_dbfs = None;
        self.open = true;
        self.hold_left = self.hold_blocks;
        self.target_gain = 1.0;
        self.gain = 1.0;
    }

    /// Called when a block is complete: updates the noise floor and the target gain.
    fn finish_block(&mut self, channels: usize) {
        let mean_square = self.block_energy / (self.block_count * channels) as f64;
        self.block_energy = 0.0;
        self.block_count = 0;
        let level = if mean_square > 0.0 {
            ((10.0 * mean_square.log10()) as f32).max(SILENT_DBFS)
        } else {
            SILENT_DBFS
        };

        // Noise floor: quick to fall, slow to rise, never below the minimum.
        let measured = level.max(MIN_FLOOR_DBFS);
        let floor = match self.noise_floor_dbfs {
            None => measured,
            Some(floor) if measured < floor => floor + (measured - floor) * self.floor_fall,
            Some(floor) => (floor + self.floor_rise_db).min(measured),
        };
        self.noise_floor_dbfs = Some(floor);

        // Gate decision with hysteresis and hold.
        let audible = level >= self.absolute_floor_dbfs;
        if audible && level > floor + self.open_margin_db {
            self.open = true;
            self.hold_left = self.hold_blocks;
        } else if self.open {
            if audible && level >= floor + self.close_margin_db {
                self.hold_left = self.hold_blocks;
            } else if self.hold_left > 0 {
                self.hold_left -= 1;
            } else {
                self.open = false;
            }
        }
        self.target_gain = if self.open { 1.0 } else { self.closed_gain };
    }
}

/// Smoothing factor for a one-pole filter with the given time constant.
fn smoothing(seconds: f32, sample_rate: f32) -> f32 {
    1.0 - (-1.0 / (seconds * sample_rate)).exp()
}

impl Denoiser for NoiseGate {
    fn process(&mut self, chunk: &mut AudioChunk) {
        let channels = chunk.channels as usize;
        if channels == 0 {
            return;
        }
        let format = StreamFormat {
            sample_rate: chunk.sample_rate,
            channels: chunk.channels,
        };
        if self.format != Some(format) {
            self.start_stream(chunk.sample_rate, chunk.channels);
        }

        for frame in chunk.samples.chunks_mut(channels) {
            let mut energy = 0.0f64;
            for sample in frame.iter_mut() {
                if !sample.is_finite() {
                    *sample = 0.0;
                }
                energy += f64::from(*sample) * f64::from(*sample);
            }

            let speed = if self.target_gain > self.gain {
                self.attack
            } else {
                self.release
            };
            self.gain += (self.target_gain - self.gain) * speed;
            for sample in frame.iter_mut() {
                *sample *= self.gain;
            }

            self.block_energy += energy;
            self.block_count += 1;
            if self.block_count >= self.block_frames {
                self.finish_block(channels);
            }
        }
    }

    fn name(&self) -> &'static str {
        "noise-gate"
    }
}

/// Picks the denoiser for a noise mode: Off is [`Passthrough`] (the audio is not touched),
/// Light is [`NoiseGate::light`] and Strong is [`NoiseGate::strong`].
pub fn denoiser_for(mode: NoiseMode) -> Box<dyn Denoiser> {
    match mode {
        NoiseMode::Off => Box::new(Passthrough),
        NoiseMode::Light => Box::new(NoiseGate::light()),
        NoiseMode::Strong => Box::new(NoiseGate::strong()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const RATE: u32 = 48_000;
    const RATE_USIZE: usize = RATE as usize;

    struct Rng(u64);

    impl Rng {
        fn uniform(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }

        fn gauss(&mut self) -> f64 {
            let (u1, u2) = (self.uniform().max(1e-12), self.uniform());
            (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()
        }
    }

    fn db(x: f64) -> f64 {
        20.0 * x.max(1e-12).log10()
    }

    fn rms(samples: &[f32]) -> f64 {
        let sum: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
        (sum / samples.len().max(1) as f64).sqrt()
    }

    /// Scales `v` so that its RMS is `dbfs`.
    fn scaled(mut v: Vec<f64>, dbfs: f64) -> Vec<f32> {
        let cur = (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
        let k = 10f64.powf(dbfs / 20.0) / cur;
        v.iter_mut().for_each(|x| *x *= k);
        v.into_iter().map(|x| x as f32).collect()
    }

    fn noise(seed: u64, dbfs: f64, frames: usize) -> Vec<f32> {
        let mut rng = Rng(seed);
        scaled((0..frames).map(|_| rng.gauss()).collect(), dbfs)
    }

    /// One second of voice-like sound: 200 + 400 + 800 Hz with a 4 Hz wobble and 1 ms fades.
    fn voice(dbfs: f64) -> Vec<f32> {
        let n = RATE_USIZE;
        let fade = 48usize;
        let v = (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                let wobble = 1.0 + 0.5 * (2.0 * PI * 4.0 * t).sin();
                let tone: f64 = [200.0, 400.0, 800.0]
                    .iter()
                    .map(|f| (2.0 * PI * f * t).sin())
                    .sum();
                let edge = if i < fade {
                    0.5 - 0.5 * (PI * i as f64 / fade as f64).cos()
                } else if i >= n - fade {
                    0.5 - 0.5 * (PI * (n - 1 - i) as f64 / fade as f64).cos()
                } else {
                    1.0
                };
                wobble * tone * edge
            })
            .collect();
        scaled(v, dbfs)
    }

    /// Interleaved signal of `seconds`: -60 dBFS noise, with a one second voice burst starting
    /// at 1.0 s when `voice_dbfs` is given.
    fn scenario(voice_dbfs: Option<f64>, channels: usize, seconds: f64) -> Vec<f32> {
        let frames = (seconds * f64::from(RATE)) as usize;
        let per_channel: Vec<Vec<f32>> = (0..channels)
            .map(|c| {
                let mut n = noise(1234 + c as u64 * 77, -60.0, frames);
                if let Some(d) = voice_dbfs {
                    for (slot, v) in n[RATE_USIZE..].iter_mut().zip(voice(d)) {
                        *slot += v;
                    }
                }
                n
            })
            .collect();
        (0..frames)
            .flat_map(|i| per_channel.iter().map(move |c| c[i]))
            .collect()
    }

    fn chunk_of(samples: Vec<f32>, channels: u16) -> AudioChunk {
        AudioChunk {
            sample_rate: RATE,
            channels,
            samples,
            timestamp_us: 0,
        }
    }

    /// Runs `input` through `denoiser` in chunks of `chunk_frames` frames.
    fn run(
        denoiser: &mut dyn Denoiser,
        input: &[f32],
        channels: usize,
        chunk_frames: usize,
    ) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        for part in input.chunks(chunk_frames * channels) {
            let mut chunk = chunk_of(part.to_vec(), channels as u16);
            denoiser.process(&mut chunk);
            out.extend(chunk.samples);
        }
        out
    }

    /// The part of an interleaved signal between `t0` and `t1` seconds.
    fn seg(v: &[f32], channels: usize, t0: f64, t1: f64) -> &[f32] {
        let at = |t: f64| (t * f64::from(RATE)) as usize * channels;
        &v[at(t0)..at(t1)]
    }

    /// Gain after every frame of a mono signal, found by feeding one frame at a time.
    fn gain_trace(gate: &mut NoiseGate, input: &[f32]) -> Vec<f32> {
        input
            .iter()
            .map(|s| {
                gate.process(&mut chunk_of(vec![*s], 1));
                gate.current_gain()
            })
            .collect()
    }

    fn gates() -> [(&'static str, NoiseGate); 2] {
        [
            ("light", NoiseGate::light()),
            ("strong", NoiseGate::strong()),
        ]
    }

    #[test]
    fn passthrough_leaves_samples_identical() {
        let mut chunk = chunk_of(scenario(None, 2, 0.1), 2);
        let before = chunk.clone();
        let mut denoiser = Passthrough;
        denoiser.process(&mut chunk);
        assert_eq!(chunk, before);
        assert_eq!(denoiser.name(), "passthrough");
    }

    #[test]
    fn output_does_not_depend_on_chunk_size() {
        let input = scenario(Some(-45.0), 2, 3.0);
        for (name, gate) in gates() {
            let reference = run(&mut gate.clone(), &input, 2, 1);
            for frames in [64, 240, 480, 1000, 4096, input.len()] {
                let out = run(&mut gate.clone(), &input, 2, frames);
                assert_eq!(out, reference, "{name}, chunks of {frames} frames");
            }
        }
    }

    #[test]
    fn quiet_voice_keeps_its_level() {
        for channels in [1, 2] {
            for voice_dbfs in [-45.0, -18.0] {
                let input = scenario(Some(voice_dbfs), channels, 3.5);
                for (name, mut gate) in gates() {
                    let out = run(&mut gate, &input, channels, 480);
                    let change = db(rms(seg(&out, channels, 1.05, 1.95)))
                        - db(rms(seg(&input, channels, 1.05, 1.95)));
                    assert!(
                        change.abs() < 1.5,
                        "{name}, {channels} ch, {voice_dbfs} dBFS voice changed by {change} dB"
                    );
                }
            }
        }
    }

    #[test]
    fn noise_only_sections_are_reduced() {
        let with_voice = scenario(Some(-45.0), 2, 3.5);
        let without = scenario(None, 2, 3.5);
        for (name, mut gate, needed) in [
            ("light", NoiseGate::light(), 10.0),
            ("strong", NoiseGate::strong(), 25.0),
        ] {
            let out = run(&mut gate.clone(), &without, 2, 480);
            let drop = db(rms(seg(&without, 2, 0.5, 3.5))) - db(rms(seg(&out, 2, 0.5, 3.5)));
            assert!(
                drop >= needed,
                "{name}: noise-only input reduced by {drop} dB"
            );

            let out = run(&mut gate, &with_voice, 2, 480);
            for (t0, t1) in [(0.5, 0.95), (2.5, 3.5)] {
                let drop = db(rms(seg(&with_voice, 2, t0, t1))) - db(rms(seg(&out, 2, t0, t1)));
                assert!(drop >= needed, "{name}: {t0}-{t1} s reduced by {drop} dB");
            }
        }
    }

    #[test]
    fn noise_floor_is_not_raised_by_speech() {
        let input = scenario(Some(-18.0), 1, 3.5);
        let mut gate = NoiseGate::strong();
        run(&mut gate, seg(&input, 1, 0.0, 1.0), 1, 480);
        let before = gate.noise_floor_dbfs().unwrap();
        run(&mut gate, seg(&input, 1, 1.0, 2.0), 1, 480);
        let after = gate.noise_floor_dbfs().unwrap();
        assert!((before + 60.0).abs() < 1.0, "floor before speech {before}");
        assert!(after - before < 3.5, "floor rose from {before} to {after}");
    }

    #[test]
    fn onset_attenuation_is_short() {
        let mono_start = RATE_USIZE;
        for voice_dbfs in [-45.0, -18.0] {
            let input = scenario(Some(voice_dbfs), 1, 3.0);
            for (name, mut gate) in gates() {
                let trace = gain_trace(&mut gate, &input);
                let open_at = trace[mono_start..]
                    .iter()
                    .position(|g| *g >= 0.891)
                    .expect("gate never opened");
                let ms = open_at as f64 * 1000.0 / f64::from(RATE);
                assert!(ms <= 15.0, "{name}, {voice_dbfs} dBFS: {ms} ms below -1 dB");
            }
        }
    }

    #[test]
    fn gain_never_jumps() {
        let input = scenario(Some(-45.0), 1, 3.5);
        for (name, mut gate) in gates() {
            let trace = gain_trace(&mut gate, &input);
            let max_step = trace
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max);
            assert!(max_step <= 0.02, "{name}: largest gain step {max_step}");
            assert!(trace.iter().any(|g| *g < 0.5), "{name} never closed");
        }
    }

    #[test]
    fn gate_attenuates_steady_quiet_sine() {
        // A steady sound that never changes is noise: the gate closes after the hold time.
        let sine: Vec<f32> = (0..RATE_USIZE)
            .map(|i| {
                let t = i as f64 / f64::from(RATE);
                (10f64.powf(-70.0 / 20.0) * 2f64.sqrt() * (2.0 * PI * 440.0 * t).sin()) as f32
            })
            .collect();
        for (name, mut gate) in gates() {
            let out = run(&mut gate, &sine, 1, 480);
            let ratio = rms(seg(&out, 1, 0.5, 1.0)) / rms(seg(&sine, 1, 0.5, 1.0));
            assert!(ratio < 0.3, "{name}: ratio was {ratio}");
        }
    }

    #[test]
    fn gate_keeps_channels_in_step() {
        let mut gate = NoiseGate::light();
        let mono = scenario(None, 1, 0.5);
        let mut chunk = chunk_of(mono.iter().flat_map(|s| [*s, *s]).collect(), 2);
        gate.process(&mut chunk);
        for frame in chunk.samples.chunks(2) {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn gate_handles_silence_and_bad_input() {
        for (_, mut gate) in gates() {
            let mut chunk = chunk_of(vec![0.0; 96_000], 2);
            gate.process(&mut chunk);
            assert!(chunk.samples.iter().all(|s| *s == 0.0));
            assert!(gate.current_gain() < 0.5, "silence should close the gate");
            assert!(gate.noise_floor_dbfs().unwrap().is_finite());

            chunk.samples = vec![f32::NAN, f32::INFINITY, 0.5, -0.5, f32::NEG_INFINITY, 1e30];
            gate.process(&mut chunk);
            assert!(chunk.samples.iter().all(|s| s.is_finite()));
            assert!(gate.current_gain().is_finite());
            assert!(gate.noise_floor_dbfs().unwrap().is_finite());
        }

        // Bad values in the middle of a long signal.
        let mut input = scenario(Some(-18.0), 1, 2.5);
        input[1000] = f32::NAN;
        input[60_000] = f32::INFINITY;
        input[100_000] = f32::NEG_INFINITY;
        let out = run(&mut NoiseGate::strong(), &input, 1, 480);
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn gate_handles_odd_shapes() {
        let mut gate = NoiseGate::new(f32::NAN);
        // No channels, no samples, a zero sample rate and a ragged last frame.
        let mut zero_channels = chunk_of(vec![0.1; 10], 0);
        gate.process(&mut zero_channels);
        assert_eq!(zero_channels.samples, vec![0.1; 10]);

        let mut empty = chunk_of(Vec::new(), 1);
        gate.process(&mut empty);

        let mut odd = AudioChunk {
            sample_rate: 0,
            channels: 2,
            samples: vec![0.2; 7],
            timestamp_us: 0,
        };
        gate.process(&mut odd);
        assert!(odd.samples.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn format_change_restarts_the_gate() {
        let mut gate = NoiseGate::strong();
        let noise_only = scenario(None, 1, 1.0);
        run(&mut gate, &noise_only, 1, 480);
        assert!(gate.current_gain() < 0.1, "gate should be closed");

        // New sample rate: no panic, finite output, and the gate starts open again.
        for (rate, channels) in [
            (16_000u32, 1u16),
            (44_100, 2),
            (8_000, 1),
            (0, 2),
            (96_000, 6),
        ] {
            let mut chunk = AudioChunk {
                sample_rate: rate,
                channels,
                samples: vec![0.01; 960 * channels as usize],
                timestamp_us: 0,
            };
            gate.process(&mut chunk);
            assert!(chunk.samples.iter().all(|s| s.is_finite()));
            assert!(
                chunk.samples[0] > 0.0099,
                "first sample was {}",
                chunk.samples[0]
            );
        }

        // Channel count change at the same sample rate also restarts.
        run(&mut gate, &noise_only, 1, 480);
        assert!(gate.current_gain() < 0.1);
        let mut stereo = chunk_of(vec![0.01; 960], 2);
        gate.process(&mut stereo);
        assert!(stereo.samples[0] > 0.0099);
    }

    #[test]
    fn gate_gain_changes_smoothly() {
        // A steady sound under the absolute floor: the gain must glide down, not jump.
        let mut gate = NoiseGate::new(-40.0);
        let mut chunk = chunk_of(vec![0.005; 9600], 1);
        gate.process(&mut chunk);
        let max_step = chunk
            .samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_step < 1e-5, "largest step was {max_step}");
        assert!(gate.current_gain() < 1.0);
    }

    #[test]
    fn gate_works_at_other_sample_rates() {
        for rate in [8_000u32, 16_000, 44_100, 96_000] {
            let mut gate = NoiseGate::light();
            let mut chunk = AudioChunk {
                sample_rate: rate,
                channels: 1,
                samples: vec![0.0; rate as usize / 10],
                timestamp_us: 0,
            };
            gate.process(&mut chunk);
            assert!(chunk.samples.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn denoiser_for_picks_by_mode() {
        assert_eq!(denoiser_for(NoiseMode::Off).name(), "passthrough");
        assert_eq!(denoiser_for(NoiseMode::Light).name(), "noise-gate");
        assert_eq!(denoiser_for(NoiseMode::Strong).name(), "noise-gate");
    }

    #[test]
    fn strong_gate_closes_further_than_light() {
        let input = scenario(None, 1, 2.0);
        let mut light = denoiser_for(NoiseMode::Light);
        let mut strong = denoiser_for(NoiseMode::Strong);
        let light_out = run(light.as_mut(), &input, 1, 480);
        let strong_out = run(strong.as_mut(), &input, 1, 480);
        assert!(rms(seg(&strong_out, 1, 1.0, 2.0)) < rms(seg(&light_out, 1, 1.0, 2.0)) / 3.0);
    }
}
