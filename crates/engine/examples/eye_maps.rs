//! Development harness: writes the bytes the window's `eye_contact_maps` command would answer
//! for one JPEG, so the preview shader can be checked against the meeting engine.
//!
//!   cargo run --release -p studio-engine --example eye_maps -- <in.jpg> <out.bin>
use studio_core::frame::VideoFrame;
use studio_engine::{CropRect, eyecontact};
use studio_segmentation::{FaceDetector, VisionFaceDetector, Want};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rgb = image::open(&args[1]).expect("decode").to_rgba8();
    let mut frame = VideoFrame::new_bgra(rgb.width(), rgb.height(), 0);
    for (dst, src) in frame.data.chunks_exact_mut(4).zip(rgb.pixels()) {
        dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
    }
    let seen = VisionFaceDetector::new()
        .expect("Vision")
        .detect_frame(
            &frame,
            Want {
                hands: false,
                eyes: true,
            },
        )
        .expect("eyes");
    let size = (frame.width, frame.height);
    let whole = CropRect {
        x: 0.0,
        y: 0.0,
        width: size.0 as f32,
        height: size.1 as f32,
    };
    let plans = eyecontact::biggest_pair(&seen.eyes, size, &whole, size, false)
        .filter(|(l, r)| eyecontact::usable(l, r))
        .and_then(|(l, r)| eyecontact::plan_pair(&frame, &l, &r))
        .expect("usable eyes");
    let mut out: Vec<f32> = vec![size.0 as f32, size.1 as f32, plans.len() as f32];
    for p in &plans {
        out.extend([p.top as f32, p.left as f32, p.height as f32, p.width as f32]);
        out.push(if p.mirrored { 1.0 } else { 0.0 });
        for (flow, keep) in p.warp.flow.chunks_exact(2).zip(&p.warp.keep) {
            out.extend([flow[0], flow[1], *keep]);
        }
    }
    let bytes: Vec<u8> = out.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(&args[2], bytes).expect("write");
    println!(
        "{} eyes, boxes {:?}",
        plans.len(),
        plans
            .iter()
            .map(|p| (p.top, p.left, p.height, p.width, p.mirrored))
            .collect::<Vec<_>>()
    );
}
