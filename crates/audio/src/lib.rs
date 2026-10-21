//! Microphone noise removal.
//!
//! The planned denoiser is DeepFilterNet 3 (Apache-2.0 or MIT, written in Rust), with RNNoise as
//! a low-CPU mode. Until one of them is integrated, [`NoiseGate`] is a simple stand-in that
//! turns down the microphone when it is quiet. [`denoiser_for`] picks the right one for a
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

/// Length of one measuring block in seconds (10 ms).
const BLOCK_SECONDS: f32 = 0.010;
/// Gain the gate settles to when the block is quiet. Not zero, so the room does not go dead.
const CLOSED_GAIN: f32 = 0.03;
/// How fast the gate opens, as a time constant in seconds.
const ATTACK_SECONDS: f32 = 0.002;
/// How fast the gate closes, as a time constant in seconds.
const RELEASE_SECONDS: f32 = 0.060;

/// Simple noise gate: measures the loudness (RMS) of each 10 ms block and turns the block down
/// when it is quieter than the threshold. The gain moves toward its target one sample at a time
/// with separate attack (opening) and release (closing) speeds, so there are no clicks.
///
/// This is a stand-in until DeepFilterNet is integrated. It works with any channel count
/// (interleaved samples) and any sample rate. The gain is shared by all channels so the stereo
/// image does not shift. Samples that are NaN or infinite are replaced by silence.
///
/// Each call to [`Denoiser::process`] splits the chunk into 10 ms blocks and decides each block
/// from its own loudness, so the gate opens in time for the start of speech. A shorter block at
/// the end of a chunk is judged on its own.
#[derive(Debug, Clone)]
pub struct NoiseGate {
    /// Loudness below which a block counts as noise, as a linear RMS value.
    threshold_rms: f32,
    /// Gain currently applied (1.0 = unchanged).
    gain: f32,
}

impl NoiseGate {
    /// Creates a gate that closes for blocks quieter than `threshold_dbfs` (for example -50.0).
    /// A threshold that is not a number is treated as -120 dBFS, which never closes the gate.
    pub fn new(threshold_dbfs: f32) -> Self {
        let dbfs = if threshold_dbfs.is_nan() {
            -120.0
        } else {
            threshold_dbfs.clamp(-120.0, 0.0)
        };
        Self {
            threshold_rms: 10f32.powf(dbfs / 20.0),
            gain: 1.0,
        }
    }

    /// Gain applied to the most recent sample (1.0 = unchanged, small = gate closed).
    pub fn current_gain(&self) -> f32 {
        self.gain
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
        let sample_rate = chunk.sample_rate.max(1) as f32;
        let block_frames = ((sample_rate * BLOCK_SECONDS) as usize).max(1);
        let attack = smoothing(ATTACK_SECONDS, sample_rate);
        let release = smoothing(RELEASE_SECONDS, sample_rate);

        for block in chunk.samples.chunks_mut(block_frames * channels) {
            let mut sum_squares = 0.0f32;
            for sample in block.iter_mut() {
                if !sample.is_finite() {
                    *sample = 0.0;
                }
                sum_squares += *sample * *sample;
            }
            let rms = (sum_squares / block.len() as f32).sqrt();
            let target = if rms >= self.threshold_rms {
                1.0
            } else {
                CLOSED_GAIN
            };
            let speed = if target > self.gain { attack } else { release };

            for frame in block.chunks_mut(channels) {
                self.gain += (target - self.gain) * speed;
                for sample in frame {
                    *sample *= self.gain;
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "noise-gate"
    }
}

/// Picks the denoiser for a noise mode: Off is [`Passthrough`], Light is a [`NoiseGate`] at
/// -50 dBFS and Strong is a [`NoiseGate`] at -40 dBFS.
pub fn denoiser_for(mode: NoiseMode) -> Box<dyn Denoiser> {
    match mode {
        NoiseMode::Off => Box::new(Passthrough),
        NoiseMode::Light => Box::new(NoiseGate::new(-50.0)),
        NoiseMode::Strong => Box::new(NoiseGate::new(-40.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Builds a chunk holding a sine wave of the given RMS level, same on every channel.
    fn sine_chunk(rms_dbfs: f32, channels: u16, frames: usize, start: usize) -> AudioChunk {
        let peak = 10f32.powf(rms_dbfs / 20.0) * std::f32::consts::SQRT_2;
        let mut samples = Vec::with_capacity(frames * channels as usize);
        for i in 0..frames {
            let t = (start + i) as f32 / RATE as f32;
            let v = peak * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
            for _ in 0..channels {
                samples.push(v);
            }
        }
        AudioChunk {
            sample_rate: RATE,
            channels,
            samples,
            timestamp_us: 0,
        }
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// Runs one second of sine through `denoiser` in 10 ms chunks and returns the output RMS
    /// ratio over the last half second.
    fn settled_ratio(denoiser: &mut dyn Denoiser, rms_dbfs: f32, channels: u16) -> f32 {
        let chunk_frames = 480;
        let mut in_tail = Vec::new();
        let mut out_tail = Vec::new();
        for n in 0..100 {
            let mut chunk = sine_chunk(rms_dbfs, channels, chunk_frames, n * chunk_frames);
            let original = chunk.samples.clone();
            denoiser.process(&mut chunk);
            if n >= 50 {
                in_tail.extend(original);
                out_tail.extend(chunk.samples);
            }
        }
        rms(&out_tail) / rms(&in_tail)
    }

    #[test]
    fn passthrough_leaves_samples_identical() {
        let mut chunk = sine_chunk(-20.0, 2, 480, 0);
        let before = chunk.clone();
        let mut denoiser = Passthrough;
        denoiser.process(&mut chunk);
        assert_eq!(chunk, before);
        assert_eq!(denoiser.name(), "passthrough");
    }

    #[test]
    fn gate_attenuates_quiet_sine() {
        let mut gate = NoiseGate::new(-50.0);
        let ratio = settled_ratio(&mut gate, -70.0, 1);
        assert!(ratio < 0.1, "ratio was {ratio}");
    }

    #[test]
    fn gate_keeps_loud_sine_near_unity() {
        let mut gate = NoiseGate::new(-40.0);
        let ratio = settled_ratio(&mut gate, -10.0, 1);
        assert!(ratio > 0.98 && ratio <= 1.0001, "ratio was {ratio}");
    }

    #[test]
    fn gate_works_with_two_channels() {
        let mut gate = NoiseGate::new(-50.0);
        assert!(settled_ratio(&mut gate, -70.0, 2) < 0.1);
        let mut gate = NoiseGate::new(-50.0);
        assert!(settled_ratio(&mut gate, -10.0, 2) > 0.98);
    }

    #[test]
    fn gate_keeps_channels_in_step() {
        let mut gate = NoiseGate::new(-50.0);
        let mut chunk = sine_chunk(-20.0, 2, 480, 0);
        gate.process(&mut chunk);
        for frame in chunk.samples.chunks(2) {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn gate_handles_silence_and_bad_input() {
        let mut gate = NoiseGate::new(-50.0);
        let mut chunk = AudioChunk {
            sample_rate: RATE,
            channels: 2,
            samples: vec![0.0; 960],
            timestamp_us: 0,
        };
        gate.process(&mut chunk);
        assert!(chunk.samples.iter().all(|s| *s == 0.0));

        chunk.samples = vec![f32::NAN, f32::INFINITY, 0.5, -0.5];
        gate.process(&mut chunk);
        assert!(chunk.samples.iter().all(|s| s.is_finite()));
        assert!(gate.current_gain().is_finite());
    }

    #[test]
    fn gate_handles_odd_shapes() {
        let mut gate = NoiseGate::new(f32::NAN);
        // No channels, no samples, a tiny sample rate and a ragged last frame.
        let mut zero_channels = AudioChunk {
            sample_rate: RATE,
            channels: 0,
            samples: vec![0.1; 10],
            timestamp_us: 0,
        };
        gate.process(&mut zero_channels);
        assert_eq!(zero_channels.samples, vec![0.1; 10]);

        let mut empty = AudioChunk {
            sample_rate: RATE,
            channels: 1,
            samples: Vec::new(),
            timestamp_us: 0,
        };
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
    fn gate_gain_changes_smoothly() {
        // A steady quiet signal under the threshold: the gain must glide down, not jump.
        let mut gate = NoiseGate::new(-40.0);
        let mut chunk = AudioChunk {
            sample_rate: RATE,
            channels: 1,
            samples: vec![0.005; 4800],
            timestamp_us: 0,
        };
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
            let mut gate = NoiseGate::new(-50.0);
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
    fn strong_gate_is_stricter_than_light() {
        // A -45 dBFS signal passes the light gate (-50) but is gated by the strong one (-40).
        let mut light = denoiser_for(NoiseMode::Light);
        let mut strong = denoiser_for(NoiseMode::Strong);
        assert!(settled_ratio(light.as_mut(), -45.0, 1) > 0.98);
        assert!(settled_ratio(strong.as_mut(), -45.0, 1) < 0.1);
    }
}
