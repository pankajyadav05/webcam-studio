//! Development harness: runs auto-frame (Apple Vision faces) and the touch-ups over a folder
//! of JPEG frames, as the meeting camera does, and writes every 30th result with timings.
//!
//!   cargo run --release -p studio-engine --example look_frames -- <in dir> <out dir> [smoothing] [warmth] [low-light|-] [studio light] [eyes,show,noauto]
use std::time::{Duration, Instant};
use studio_core::frame::VideoFrame;
use studio_core::settings::{Appearance, Settings};
use studio_engine::Engine;
use studio_segmentation::{FaceDetector, VisionFaceDetector};

fn load(path: &std::path::Path) -> VideoFrame {
    let rgb = image::open(path).expect("decode").to_rgba8();
    let mut frame = VideoFrame::new_bgra(rgb.width(), rgb.height(), 0);
    for (dst, src) in frame.data.chunks_exact_mut(4).zip(rgb.pixels()) {
        dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
    }
    frame
}

fn save(frame: &VideoFrame, path: &std::path::Path) {
    let mut rgb = image::RgbImage::new(frame.width, frame.height);
    for (dst, src) in rgb.pixels_mut().zip(frame.data.chunks_exact(4)) {
        *dst = image::Rgb([src[2], src[1], src[0]]);
    }
    rgb.save(path).expect("save");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (input, output) = (&args[1], &args[2]);
    let number =
        |i: usize, default: f32| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(default);
    // Extra switches, comma separated: eyes, show, noauto.
    let flags = args.get(7).cloned().unwrap_or_default();
    std::fs::create_dir_all(output).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(input)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        .collect();
    files.sort();
    let mut settings = Settings {
        appearance: Appearance {
            smoothing: number(3, 0.7),
            warmth: number(4, 0.0),
            low_light: args.get(5).is_some_and(|v| v == "low-light"),
            studio_light: number(6, 0.0),
            eye_contact: flags.contains("eyes"),
        },
        ..Settings::default()
    };
    settings.framing.auto_frame = !flags.contains("noauto");
    settings.framing.show_and_tell = flags.contains("show");
    let mut engine = Engine::new(settings.clone());
    let mut plain = Engine::new(Settings::default());
    let mut faces = VisionFaceDetector::new().expect("Vision");
    let mut look = studio_engine::appearance::Look::new();
    let (mut face_time, mut frame_time, mut look_time) =
        (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let mut found = 0;
    for (i, path) in files.iter().enumerate() {
        let mut frame = load(path);
        frame.timestamp_us = i as u64 * 33_333;
        let want = engine.wants().unwrap_or_default();
        if want.eyes || i % 3 == 0 {
            let start = Instant::now();
            let seen = faces.detect_frame(&frame, want).expect("faces");
            face_time += start.elapsed();
            found += usize::from(!seen.faces.is_empty());
            if !seen.hands.is_empty() && i % 15 == 0 {
                println!("frame {i}: hands {:?}", seen.hands);
            }
            engine.observe(&seen, frame.width, frame.height, frame.timestamp_us);
        }
        let start = Instant::now();
        let out = engine.process_video(&frame);
        frame_time += start.elapsed();
        // The touch-ups alone, on the plain framed picture.
        let mut framed = plain.process_video(&frame);
        let start = Instant::now();
        look.apply(&mut framed, &settings.appearance);
        look_time += start.elapsed();
        if i % 30 == 0 || files.len() < 30 {
            save(
                &out,
                &std::path::Path::new(output).join(format!("auto-{i:04}.jpg")),
            );
            save(
                &framed,
                &std::path::Path::new(output).join(format!("look-{i:04}.jpg")),
            );
            let f = engine.framing_in_use();
            println!(
                "frame {i}: zoom {:.2} pan {:.2},{:.2}",
                f.zoom, f.pan_x, f.pan_y
            );
        }
    }
    let n = files.len().max(1) as f64;
    println!(
        "{} frames; faces in {found} of {} searches, {:.1} ms each; engine {:.1} ms a frame; touch-ups alone {:.1} ms",
        files.len(),
        files.len().div_ceil(3),
        face_time.as_secs_f64() * 1000.0 / (n / 3.0),
        frame_time.as_secs_f64() * 1000.0 / n,
        look_time.as_secs_f64() * 1000.0 / n,
    );
}
