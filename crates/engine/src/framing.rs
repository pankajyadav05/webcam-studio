//! Framing maths: which part of the camera image to show, and how to scale it.

use studio_core::frame::VideoFrame;
use studio_core::settings::Framing;

/// A rectangle inside the input image, in input pixels. Fractions are allowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Works out the part of the input image to show.
///
/// The rectangle has the output's aspect ratio and is as large as fits in the input, divided by
/// the zoom. The pan values slide it inside the leftover margin: -1 puts it against the left or
/// top edge, +1 against the right or bottom edge, 0 centres it. The framing is clamped first (see
/// [`Framing::clamped`]), so out-of-range values are safe. The rectangle never leaves the input
/// (apart from tiny floating point rounding).
///
/// If the output size is zero, the input's own aspect ratio is used instead.
pub fn crop_rect(
    input_w: u32,
    input_h: u32,
    out_w: u32,
    out_h: u32,
    framing: &Framing,
) -> CropRect {
    let iw = input_w as f64;
    let ih = input_h as f64;
    if input_w == 0 || input_h == 0 {
        return CropRect {
            x: 0.0,
            y: 0.0,
            width: iw as f32,
            height: ih as f32,
        };
    }
    let framing = framing.clamped();
    let aspect = if out_w == 0 || out_h == 0 {
        iw / ih
    } else {
        out_w as f64 / out_h as f64
    };

    // Largest rectangle with the output aspect that fits in the input.
    let (max_w, max_h) = if iw / ih > aspect {
        (ih * aspect, ih)
    } else {
        (iw, iw / aspect)
    };
    let zoom = framing.zoom as f64;
    let width = ((max_w / zoom) as f32).min(input_w as f32);
    let height = ((max_h / zoom) as f32).min(input_h as f32);

    let margin_x = (input_w as f32 - width).max(0.0);
    let margin_y = (input_h as f32 - height).max(0.0);
    let x = (margin_x * (framing.pan_x + 1.0) * 0.5).clamp(0.0, margin_x);
    let y = (margin_y * (framing.pan_y + 1.0) * 0.5).clamp(0.0, margin_y);
    CropRect {
        x,
        y,
        width,
        height,
    }
}

/// For one axis: for each output position, the two source indexes to blend and the weight of
/// the second one.
fn axis_samples(
    start: f32,
    length: f32,
    out_n: u32,
    src_n: u32,
    flip: bool,
) -> Vec<(usize, usize, f32)> {
    let last = (src_n - 1) as f32;
    (0..out_n)
        .map(|o| {
            let o = if flip { out_n - 1 - o } else { o };
            // Sample at the middle of the output pixel, mapped back into the source.
            let pos = (start + (o as f32 + 0.5) * length / out_n as f32 - 0.5).clamp(0.0, last);
            let i0 = pos.floor();
            let frac = pos - i0;
            let i0 = i0 as usize;
            let i1 = (i0 + 1).min(src_n as usize - 1);
            (i0, i1, frac)
        })
        .collect()
}

/// Copies the `rect` part of `src` into a new `out_w` x `out_h` frame using bilinear sampling,
/// optionally flipping it left to right. The new frame has no row padding and keeps the source
/// timestamp. A source that is empty or has too little data gives a black frame.
pub(crate) fn scale_bilinear(
    src: &VideoFrame,
    rect: &CropRect,
    out_w: u32,
    out_h: u32,
    mirror: bool,
) -> VideoFrame {
    let mut out = VideoFrame::new_bgra(out_w, out_h, src.timestamp_us);
    let bpp = src.format.bytes_per_pixel();
    let needed = if src.height == 0 {
        0
    } else {
        src.stride * (src.height as usize - 1) + src.width as usize * bpp
    };
    if src.width == 0 || src.height == 0 || out_w == 0 || out_h == 0 || src.data.len() < needed {
        return out;
    }

    let columns = axis_samples(rect.x, rect.width, out_w, src.width, mirror);
    let rows = axis_samples(rect.y, rect.height, out_h, src.height, false);
    let out_stride = out.stride;
    for (oy, &(y0, y1, fy)) in rows.iter().enumerate() {
        let row0 = &src.data[y0 * src.stride..];
        let row1 = &src.data[y1 * src.stride..];
        let dst = &mut out.data[oy * out_stride..oy * out_stride + out_w as usize * 4];
        for (ox, &(x0, x1, fx)) in columns.iter().enumerate() {
            for c in 0..4 {
                let top = row0[x0 * bpp + c] as f32 * (1.0 - fx) + row0[x1 * bpp + c] as f32 * fx;
                let bottom =
                    row1[x0 * bpp + c] as f32 * (1.0 - fx) + row1[x1 * bpp + c] as f32 * fx;
                let value = top * (1.0 - fy) + bottom * fy;
                dst[ox * 4 + c] = (value + 0.5) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-3;

    fn framing(zoom: f32, pan_x: f32, pan_y: f32) -> Framing {
        Framing {
            zoom,
            pan_x,
            pan_y,
            ..Framing::default()
        }
    }

    fn inside(rect: &CropRect, w: u32, h: u32) -> bool {
        rect.x >= 0.0
            && rect.y >= 0.0
            && rect.width > 0.0
            && rect.height > 0.0
            && rect.x + rect.width <= w as f32 + EPS
            && rect.y + rect.height <= h as f32 + EPS
    }

    #[test]
    fn zoom_one_is_full_frame_when_aspect_matches() {
        let r = crop_rect(1920, 1080, 1280, 720, &Framing::default());
        assert_eq!(
            r,
            CropRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0
            }
        );
    }

    #[test]
    fn zoom_one_is_centred_with_output_aspect() {
        // 4:3 input to 16:9 output: full width, 360 of 480 rows, centred.
        let r = crop_rect(640, 480, 1280, 720, &Framing::default());
        assert!((r.width - 640.0).abs() < EPS);
        assert!((r.height - 360.0).abs() < EPS);
        assert!((r.x - 0.0).abs() < EPS);
        assert!((r.y - 60.0).abs() < EPS);

        // 16:9 input to 4:3 output: full height, 480 of 640 columns, centred.
        let r = crop_rect(640, 360, 800, 600, &Framing::default());
        assert!((r.height - 360.0).abs() < EPS);
        assert!((r.width - 480.0).abs() < EPS);
        assert!((r.x - 80.0).abs() < EPS);
        assert!((r.y - 0.0).abs() < EPS);
    }

    #[test]
    fn zoom_two_halves_the_rectangle() {
        let r = crop_rect(1920, 1080, 1280, 720, &framing(2.0, 0.0, 0.0));
        assert!((r.width - 960.0).abs() < EPS);
        assert!((r.height - 540.0).abs() < EPS);
        assert!((r.x - 480.0).abs() < EPS);
        assert!((r.y - 270.0).abs() < EPS);
    }

    #[test]
    fn pan_reaches_the_edges() {
        let right_bottom = crop_rect(1920, 1080, 1280, 720, &framing(2.0, 1.0, 1.0));
        assert!((right_bottom.x + right_bottom.width - 1920.0).abs() < EPS);
        assert!((right_bottom.y + right_bottom.height - 1080.0).abs() < EPS);

        let left_top = crop_rect(1920, 1080, 1280, 720, &framing(2.0, -1.0, -1.0));
        assert_eq!(left_top.x, 0.0);
        assert_eq!(left_top.y, 0.0);
    }

    #[test]
    fn rectangle_always_inside_input() {
        let sizes = [
            (1, 1),
            (7, 5),
            (641, 479),
            (640, 480),
            (1920, 1080),
            (1081, 1921),
            (33, 1000),
            (1000, 33),
        ];
        let outputs = [(1280, 720), (800, 600), (3, 7), (1, 1), (720, 1280)];
        let zooms = [0.5, 1.0, 1.37, 2.0, 3.99, 4.0, 9.0, f32::NAN];
        let pans = [-5.0, -1.0, -0.37, 0.0, 0.5, 1.0, 2.0, f32::NAN];
        for &(iw, ih) in &sizes {
            for &(ow, oh) in &outputs {
                for &zoom in &zooms {
                    for &pan in &pans {
                        let r = crop_rect(iw, ih, ow, oh, &framing(zoom, pan, -pan));
                        assert!(
                            inside(&r, iw, ih),
                            "{iw}x{ih} -> {ow}x{oh} {zoom} {pan}: {r:?}"
                        );
                        let want = ow as f32 / oh as f32;
                        assert!(
                            (r.width / r.height - want).abs() < want * 0.01 + 0.01
                                || r.width < 2.0
                                || r.height < 2.0,
                            "aspect {r:?} vs {want}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn empty_sizes_do_not_panic() {
        let r = crop_rect(0, 0, 100, 100, &Framing::default());
        assert_eq!((r.width, r.height), (0.0, 0.0));
        let r = crop_rect(100, 50, 0, 0, &Framing::default());
        assert!(inside(&r, 100, 50));
    }

    #[test]
    fn scaling_keeps_a_flat_colour() {
        let mut src = VideoFrame::new_bgra(5, 3, 77);
        for y in 0..3 {
            for x in 0..5 {
                src.set_pixel(x, y, [10, 20, 30, 255]);
            }
        }
        let rect = crop_rect(5, 3, 8, 4, &Framing::default());
        let out = scale_bilinear(&src, &rect, 8, 4, true);
        assert_eq!((out.width, out.height, out.timestamp_us), (8, 4, 77));
        for y in 0..4 {
            for x in 0..8 {
                assert_eq!(out.pixel(x, y), [10, 20, 30, 255]);
            }
        }
    }

    #[test]
    fn short_source_data_gives_black() {
        let mut src = VideoFrame::new_bgra(4, 4, 0);
        src.data.truncate(10);
        let rect = crop_rect(4, 4, 4, 4, &Framing::default());
        let out = scale_bilinear(&src, &rect, 4, 4, false);
        assert!(out.data.iter().all(|&b| b == 0));
    }
}
