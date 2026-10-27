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

#[cfg(target_os = "macos")]
#[test]
fn real_vision_finds_the_face() {
    use studio_segmentation::{FaceDetector, VisionFaceDetector, Want};
    let mut detector = VisionFaceDetector::new().unwrap();
    assert!(detector.detect(&[], Want::default()).is_err());
    let picture = include_bytes!("fixtures/astronaut.png");
    let faces = detector.detect(picture, Want::default()).unwrap().faces;
    assert_eq!(faces.len(), 1, "{faces:?}");
    let f = faces[0];
    eprintln!("face {f:?}");
    // The astronaut's face is in the upper half, near the middle.
    assert!(f.y + f.height / 2.0 < 0.6 && (0.2..0.8).contains(&(f.x + f.width / 2.0)));
    assert!(f.width > 0.05 && f.height > 0.05 && f.x >= 0.0 && f.y >= 0.0);
}

#[cfg(target_os = "macos")]
#[test]
fn real_vision_finds_the_eyes_inside_the_face() {
    use studio_segmentation::{FaceDetector, VisionFaceDetector, Want};
    let mut detector = VisionFaceDetector::new().unwrap();
    let seen = detector
        .detect(
            include_bytes!("fixtures/astronaut.png"),
            Want {
                hands: true,
                eyes: true,
            },
        )
        .unwrap();
    assert_eq!(seen.faces.len(), 1);
    assert_eq!(seen.eyes.len(), 1, "{seen:?}");
    let (face, eyes) = (seen.faces[0], seen.eyes[0]);
    eprintln!("eyes {eyes:?}, hands {:?}", seen.hands);
    for eye in [eyes.left, eyes.right] {
        assert!(eye.cx > face.x && eye.cx < face.x + face.width);
        assert!(eye.cy > face.y && eye.cy < face.y + face.height / 2.0 + 0.05);
        assert!(eye.width > 0.01 && eye.width < face.width / 2.0);
    }
    // Vision's "left" eye is the one on the left of the picture.
    assert!(eyes.left.cx < eyes.right.cx);
}
