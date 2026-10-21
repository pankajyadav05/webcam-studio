//! Blending a person over a background: box blur and mask-based compositing.

use studio_core::frame::VideoFrame;
use studio_segmentation::Mask;

/// Smallest and largest blur radius in pixels.
pub const MIN_BLUR_RADIUS: u32 = 1;
/// Largest blur radius in pixels.
pub const MAX_BLUR_RADIUS: u32 = 64;

/// Blurs a frame with a separable box filter. The radius is clamped to 1..=64 and pixels beyond
/// the edge count as copies of the edge pixel. The cost is proportional to the number of pixels,
/// not the radius, because each pass keeps a running sum. The result has no row padding.
pub fn box_blur(frame: &VideoFrame, radius: u32) -> VideoFrame {
    let radius = radius.clamp(MIN_BLUR_RADIUS, MAX_BLUR_RADIUS) as usize;
    let width = frame.width as usize;
    let height = frame.height as usize;
    let mut out = VideoFrame::new_bgra(frame.width, frame.height, frame.timestamp_us);
    if width == 0 || height == 0 {
        return out;
    }
    let row_bytes = width * 4;
    let window = (2 * radius + 1) as u32;
    let half = window / 2;

    // Horizontal pass: frame -> tmp.
    let mut tmp = vec![0u8; row_bytes * height];
    for y in 0..height {
        let src = &frame.data[y * frame.stride..y * frame.stride + row_bytes];
        let dst = &mut tmp[y * row_bytes..(y + 1) * row_bytes];
        let at = |x: isize| -> &[u8] {
            let x = x.clamp(0, width as isize - 1) as usize;
            &src[x * 4..x * 4 + 4]
        };
        let mut sum = [0u32; 4];
        for k in -(radius as isize)..=radius as isize {
            for (s, v) in sum.iter_mut().zip(at(k)) {
                *s += *v as u32;
            }
        }
        for x in 0..width {
            for c in 0..4 {
                dst[x * 4 + c] = ((sum[c] + half) / window) as u8;
            }
            let add = at(x as isize + radius as isize + 1);
            let sub = at(x as isize - radius as isize);
            for c in 0..4 {
                sum[c] = sum[c] + add[c] as u32 - sub[c] as u32;
            }
        }
    }

    // Vertical pass: tmp -> out. One running sum per column, moved down a row at a time, so
    // memory is read row by row.
    let row_at = |y: isize| -> &[u8] {
        let y = y.clamp(0, height as isize - 1) as usize;
        &tmp[y * row_bytes..(y + 1) * row_bytes]
    };
    let mut sums = vec![0u32; row_bytes];
    for k in -(radius as isize)..=radius as isize {
        for (s, v) in sums.iter_mut().zip(row_at(k)) {
            *s += *v as u32;
        }
    }
    for y in 0..height {
        let dst = &mut out.data[y * out.stride..y * out.stride + row_bytes];
        for (d, s) in dst.iter_mut().zip(&sums) {
            *d = ((*s + half) / window) as u8;
        }
        let add = row_at(y as isize + radius as isize + 1);
        let sub = row_at(y as isize - radius as isize);
        for ((s, a), b) in sums.iter_mut().zip(add).zip(sub) {
            *s = *s + *a as u32 - *b as u32;
        }
    }
    out
}

/// Mask value for pixel `(x, y)` of a `width` x `height` frame. A mask of a different size is
/// stretched with nearest-neighbour lookup. A mask that is empty or too short counts as "all
/// person" so a broken segmenter never blanks the picture.
fn mask_value(mask: &Mask, x: u32, y: u32, width: u32, height: u32) -> u8 {
    let (mw, mh) = (mask.width as usize, mask.height as usize);
    if mw == 0 || mh == 0 || mask.alpha.len() < mw * mh {
        return 255;
    }
    let mx = (x as usize * mw / width as usize).min(mw - 1);
    let my = (y as usize * mh / height as usize).min(mh - 1);
    mask.alpha[my * mw + mx]
}

/// Blends `foreground` over `background` using `mask`: out = fg * a + bg * (1 - a).
///
/// Both frames should be the same size; the colour channels are blended and the foreground's
/// alpha channel is kept. If the background is smaller than the foreground, the foreground is
/// returned unchanged.
pub fn blend(foreground: &VideoFrame, background: &VideoFrame, mask: &Mask) -> VideoFrame {
    let mut out = foreground.clone();
    if background.width != foreground.width || background.height != foreground.height {
        return out;
    }
    let (w, h) = (foreground.width, foreground.height);
    for y in 0..h {
        for x in 0..w {
            let a = mask_value(mask, x, y, w, h) as u32;
            if a == 255 {
                continue;
            }
            let fg = foreground.pixel(x, y);
            let bg = background.pixel(x, y);
            let mut px = fg;
            for c in 0..3 {
                px[c] = ((fg[c] as u32 * a + bg[c] as u32 * (255 - a) + 127) / 255) as u8;
            }
            out.set_pixel(x, y, px);
        }
    }
    out
}

/// True when every pixel of the mask is fully person, so no background work is needed.
pub fn is_all_foreground(mask: &Mask) -> bool {
    mask.alpha.iter().all(|&a| a == 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> VideoFrame {
        let mut f = VideoFrame::new_bgra(w, h, 5);
        for y in 0..h {
            for x in 0..w {
                f.set_pixel(x, y, px);
            }
        }
        f
    }

    #[test]
    fn blur_keeps_flat_colour() {
        let frame = solid(9, 7, [40, 80, 120, 255]);
        for radius in [0, 1, 3, 200] {
            let out = box_blur(&frame, radius);
            assert_eq!(out.data, frame.data, "radius {radius}");
        }
    }

    #[test]
    fn blur_averages_a_neighbourhood() {
        // A single white pixel in a black row spreads out over 2r+1 pixels.
        let mut frame = solid(11, 1, [0, 0, 0, 255]);
        frame.set_pixel(5, 0, [255, 255, 255, 255]);
        let out = box_blur(&frame, 1);
        assert_eq!(out.pixel(4, 0)[0], 85);
        assert_eq!(out.pixel(5, 0)[0], 85);
        assert_eq!(out.pixel(6, 0)[0], 85);
        assert_eq!(out.pixel(3, 0)[0], 0);
        assert_eq!(out.pixel(7, 0)[0], 0);
    }

    #[test]
    fn blur_clamps_at_the_edges() {
        // Edge pixels are repeated, so a bright left edge stays bright near the border.
        let mut frame = solid(6, 1, [0, 0, 0, 255]);
        frame.set_pixel(0, 0, [255, 255, 255, 255]);
        let out = box_blur(&frame, 1);
        assert_eq!(out.pixel(0, 0)[0], 170);
    }

    #[test]
    fn blur_handles_frames_smaller_than_the_radius() {
        let frame = solid(2, 2, [9, 9, 9, 255]);
        assert_eq!(box_blur(&frame, 64).data, frame.data);
        let empty = VideoFrame::new_bgra(0, 0, 0);
        assert!(box_blur(&empty, 5).data.is_empty());
    }

    #[test]
    fn blend_follows_the_mask() {
        let fg = solid(2, 1, [200, 0, 0, 255]);
        let bg = solid(2, 1, [0, 100, 0, 255]);
        let mask = Mask {
            width: 2,
            height: 1,
            alpha: vec![255, 0],
        };
        let out = blend(&fg, &bg, &mask);
        assert_eq!(out.pixel(0, 0), [200, 0, 0, 255]);
        assert_eq!(out.pixel(1, 0), [0, 100, 0, 255]);

        let half = Mask {
            width: 1,
            height: 1,
            alpha: vec![128],
        };
        let out = blend(&fg, &bg, &half);
        assert_eq!(out.pixel(0, 0), [100, 50, 0, 255]);
    }

    #[test]
    fn blend_survives_bad_masks() {
        let fg = solid(4, 4, [1, 2, 3, 255]);
        let bg = solid(4, 4, [9, 9, 9, 255]);
        let short = Mask {
            width: 4,
            height: 4,
            alpha: vec![0; 3],
        };
        assert_eq!(blend(&fg, &bg, &short), fg);
        let empty = Mask {
            width: 0,
            height: 0,
            alpha: Vec::new(),
        };
        assert_eq!(blend(&fg, &bg, &empty), fg);
        // A half-size mask is stretched.
        let small = Mask {
            width: 2,
            height: 2,
            alpha: vec![0; 4],
        };
        assert_eq!(blend(&fg, &bg, &small).pixel(3, 3), [9, 9, 9, 255]);
    }

    #[test]
    fn all_foreground_check() {
        assert!(is_all_foreground(&Mask::full(3, 3)));
        let mut mask = Mask::full(3, 3);
        mask.alpha[4] = 0;
        assert!(!is_all_foreground(&mask));
    }
}
