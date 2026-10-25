#[cfg(target_os = "macos")]
#[test]
fn real_vision_portrait_and_error_paths() {
    use studio_segmentation::{PersonSegmenter, Quality, VisionSegmenter};
    let mut segmenter = VisionSegmenter::new().unwrap();
    assert!(segmenter.segment(&[], Quality::Balanced).is_err());
    assert!(
        segmenter
            .segment(b"invalid image bytes", Quality::Balanced)
            .is_err()
    );
    for quality in [Quality::Balanced, Quality::Accurate] {
        let mask = segmenter
            .segment(include_bytes!("fixtures/astronaut.png"), quality)
            .unwrap();
        assert!(mask.width >= 128 && mask.height >= 128);
        assert_eq!(mask.alpha.len(), (mask.width * mask.height) as usize);
        let area = mask.alpha.iter().filter(|&&a| a > 127).count() as f64 / mask.alpha.len() as f64;
        assert!((0.1..0.9).contains(&area), "implausible person area {area}");
        assert!(mask.alpha.iter().any(|&a| a < 10));
        assert!(mask.alpha.iter().any(|&a| a > 245));
        eprintln!(
            "Vision {quality:?}: {}x{}, area {area:.3}",
            mask.width, mask.height
        );
    }
}
