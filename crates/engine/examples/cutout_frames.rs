//! Development harness: runs the native cutout (Apple Vision + the engine) over a folder of
//! JPEG frames and writes the results, so meetings output can be checked offline.
//!
//!   cargo run --release -p studio-engine --example cutout_frames -- <in dir> <out dir> blur|replace
use std::time::Instant;
use studio_core::frame::VideoFrame;
use studio_core::settings::{BackgroundMode, Settings};
use studio_engine::Engine;
use studio_segmentation::{PersonSegmenter, Quality, VisionSegmenter};

fn load(path: &std::path::Path) -> VideoFrame {
    let rgb = image::open(path).expect("decode").to_rgba8();
    let mut frame = VideoFrame::new_bgra(rgb.width(), rgb.height(), 0);
    for (dst, src) in frame.data.chunks_exact_mut(4).zip(rgb.pixels()) {
        dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
    }
    frame
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (input, output, mode) = (&args[1], &args[2], args[3].as_str());
    std::fs::create_dir_all(output).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(input)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        .collect();
    files.sort();
    let mut engine = Engine::new(Settings {
        background: if mode == "blur" {
            BackgroundMode::Blur { radius: 12 }
        } else {
            BackgroundMode::Replace {
                image_path: "x".into(),
            }
        },
        ..Settings::default()
    });
    if mode != "blur" {
        // A flat green-grey picture makes leftovers easy to see.
        let mut bg = VideoFrame::new_bgra(1280, 720, 0);
        for (i, px) in bg.data.chunks_exact_mut(4).enumerate() {
            let x = (i % 1280) as u32;
            px.copy_from_slice(&[(60 + x / 16) as u8, 140, 70, 255]);
        }
        engine.set_replacement_image(Some(bg));
    }
    let mut vision = VisionSegmenter::new().expect("Vision");
    let (mut model_ms, mut total_ms) = (Vec::new(), Vec::new());
    for (i, path) in files.iter().enumerate() {
        let mut frame = load(path);
        frame.timestamp_us = i as u64 * 33_333;
        let start = Instant::now();
        let mut spent = 0.0;
        let out = engine.process_video_with(&frame, &mut |f: &VideoFrame| {
            let t = Instant::now();
            let mask = vision
                .segment_frame(f, Quality::Balanced)
                .map_err(|e| eprintln!("{e}"))
                .ok();
            spent = t.elapsed().as_secs_f64() * 1000.0;
            mask
        });
        total_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        model_ms.push(spent);
        let mut rgb = image::RgbImage::new(out.width, out.height);
        for (dst, src) in rgb.pixels_mut().zip(out.data.chunks_exact(4)) {
            *dst = image::Rgb([src[2], src[1], src[0]]);
        }
        rgb.save(format!("{output}/{:04}.jpg", i + 1)).unwrap();
    }
    let p = |v: &mut Vec<f64>, q: f64| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[((v.len() - 1) as f64 * q) as usize]
    };
    println!(
        "frames {} model ms p50 {:.1} p90 {:.1} | whole frame ms p50 {:.1} p90 {:.1}",
        files.len(),
        p(&mut model_ms, 0.5),
        p(&mut model_ms, 0.9),
        p(&mut total_ms, 0.5),
        p(&mut total_ms, 0.9)
    );
}
