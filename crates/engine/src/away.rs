//! Be right back: while you are away, meetings see a still, blurred and darkened copy of the
//! last picture with a card on top, and nothing from the live camera.

use rayon::prelude::*;
use studio_core::frame::VideoFrame;

use crate::composite::box_blur;

/// Blur radius of the still, as a share of the picture height (about 25 px at 720p), applied
/// twice so it reads as a soft blur rather than boxes.
pub const BLUR: f32 = 0.035;
/// How bright the still stays.
pub const DARKEN: f32 = 0.55;

/// The blurred, darkened still made from the last live picture.
pub fn freeze(frame: &VideoFrame) -> VideoFrame {
    let radius = ((frame.height as f32 * BLUR).round() as u32).max(1);
    let mut still = box_blur(&box_blur(frame, radius), radius);
    let width = still.width as usize * 4;
    still.data.par_chunks_mut(still.stride).for_each(|row| {
        let end = width.min(row.len());
        for px in row[..end].chunks_exact_mut(4) {
            for c in &mut px[..3] {
                *c = (f32::from(*c) * DARKEN).round() as u8;
            }
        }
    });
    still
}

/// The still with the card on top. The card is BGRA with premultiplied colour and real
/// alpha, at the size of the still; a card of another size is left out.
pub fn compose(still: &VideoFrame, card: Option<&VideoFrame>) -> VideoFrame {
    let mut out = still.clone();
    let Some(card) = card.filter(|c| (c.width, c.height) == (still.width, still.height)) else {
        return out;
    };
    let width = out.width as usize * 4;
    out.data
        .par_chunks_mut(out.stride)
        .zip(card.data.par_chunks(card.stride))
        .for_each(|(row, over)| {
            for (px, top) in row[..width].chunks_exact_mut(4).zip(over.chunks_exact(4)) {
                let keep = 255 - u32::from(top[3]);
                for i in 0..3 {
                    px[i] = (u32::from(px[i]) * keep / 255 + u32::from(top[i])).min(255) as u8;
                }
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_still_is_blurred_and_darker() {
        let mut frame = VideoFrame::new_bgra(320, 180, 7);
        for y in 0..180 {
            for x in 0..320 {
                let v = if x < 160 { 240 } else { 40 };
                frame.set_pixel(x, y, [v, v, v, 255]);
            }
        }
        let still = freeze(&frame);
        assert!(still.pixel(0, 0)[0] < 240 && still.pixel(0, 0)[0] > 100);
        // The hard edge is gone.
        let step = i32::from(still.pixel(159, 90)[0]) - i32::from(still.pixel(160, 90)[0]);
        assert!(step.abs() < 20, "{step}");
    }

    #[test]
    fn the_card_goes_on_top_where_it_is_opaque() {
        let mut still = VideoFrame::new_bgra(4, 2, 0);
        still.data.fill(100);
        let mut card = VideoFrame::new_bgra(4, 2, 0);
        card.set_pixel(0, 0, [255, 255, 255, 255]);
        card.set_pixel(1, 0, [64, 64, 64, 128]);
        let out = compose(&still, Some(&card));
        assert_eq!(out.pixel(0, 0)[..3], [255, 255, 255]);
        assert_eq!(u32::from(out.pixel(1, 0)[0]), 100 * 127 / 255 + 64);
        assert_eq!(out.pixel(3, 1)[..3], [100, 100, 100]);
        let wrong_size = VideoFrame::new_bgra(8, 8, 0);
        assert_eq!(compose(&still, Some(&wrong_size)), still);
    }
}
