//! The eye network behind Eye contact: the warping CNN of "Look at Me! Correcting Eye Gaze in
//! Live Video Communication" (Hsu, Wang, Lei and Chen, ACM TOMM 2019), with its published
//! weights (BSD-3-Clause; see `assets/LICENSE-gaze-warp.txt`).
//!
//! Input: a 48 × 64 crop around one eye (RGB, 0..1), six eye points and the angle to move the
//! gaze by. Output: a warp field (where each pixel takes its colour from) and a brightness
//! mask. Both are applied to the full-resolution eye by `eyecontact.rs`, so the eye stays
//! sharp. The network was trained for the eye on the left of the picture; the other eye goes
//! through it mirrored.

use std::sync::OnceLock;

pub const H: usize = 48;
pub const W: usize = 64;
/// Channels of the six point maps (x and y distance to each point).
pub const POINT_MAPS: usize = 12;

const WEIGHTS: &[u8] = include_bytes!("../assets/gaze-warp.bin");
const EPS: f32 = 1e-5;

struct Conv {
    k: usize,
    cin: usize,
    cout: usize,
    kernel: Vec<f32>,
    /// Batch norm after the ReLU, folded to a scale and a shift; none for the last 1 × 1.
    norm: Option<(Vec<f32>, Vec<f32>)>,
}

struct Net {
    dense: [(Vec<f32>, Vec<f32>, usize, usize); 3],
    coarse: [Conv; 5],
    fine: [Conv; 5],
    lcm: [Conv; 3],
}

struct Reader<'a> {
    floats: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Vec<f32> {
        let bytes = &self.floats[self.at * 4..(self.at + n) * 4];
        self.at += n;
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    }

    fn conv(&mut self, k: usize, cin: usize, cout: usize, norm: bool) -> Conv {
        let kernel = self.take(k * k * cin * cout);
        let norm = norm.then(|| {
            let (gamma, beta) = (self.take(cout), self.take(cout));
            let (mean, var) = (self.take(cout), self.take(cout));
            let scale: Vec<f32> = (0..cout)
                .map(|i| gamma[i] / (var[i] + EPS).sqrt())
                .collect();
            let shift = (0..cout).map(|i| beta[i] - mean[i] * scale[i]).collect();
            (scale, shift)
        });
        Conv {
            k,
            cin,
            cout,
            kernel,
            norm,
        }
    }
}

fn net() -> &'static Net {
    static NET: OnceLock<Net> = OnceLock::new();
    NET.get_or_init(|| {
        let mut r = Reader {
            floats: WEIGHTS,
            at: 0,
        };
        let mut dense = |i: usize, o: usize| (r.take(i * o), r.take(o), i, o);
        let dense = [dense(2, 16), dense(16, 16), dense(16, 16)];
        let coarse = [
            r.conv(5, 31, 32, true),
            r.conv(3, 32, 64, true),
            r.conv(3, 96, 64, true),
            r.conv(3, 160, 32, true),
            r.conv(1, 32, 16, false),
        ];
        let fine = [
            r.conv(5, 47, 32, true),
            r.conv(3, 32, 64, true),
            r.conv(3, 96, 32, true),
            r.conv(3, 128, 16, true),
            r.conv(1, 16, 4, false),
        ];
        let lcm = [
            r.conv(3, 2, 8, true),
            r.conv(3, 8, 8, true),
            r.conv(1, 8, 2, false),
        ];
        assert_eq!(
            r.at * 4,
            WEIGHTS.len(),
            "gaze-warp.bin does not match the network"
        );
        Net {
            dense,
            coarse,
            fine,
            lcm,
        }
    })
}

/// `out (m × n) = a (m × k) · b (k × n)`, row major.
#[cfg(target_os = "macos")]
fn matmul(a: &[f32], b: &[f32], out: &mut [f32], m: usize, k: usize, n: usize) {
    #[link(name = "Accelerate", kind = "framework")]
    unsafe extern "C" {
        fn cblas_sgemm(
            order: i32,
            trans_a: i32,
            trans_b: i32,
            m: i32,
            n: i32,
            k: i32,
            alpha: f32,
            a: *const f32,
            lda: i32,
            b: *const f32,
            ldb: i32,
            beta: f32,
            c: *mut f32,
            ldc: i32,
        );
    }
    const ROW_MAJOR: i32 = 101;
    const NO_TRANS: i32 = 111;
    assert!(a.len() >= m * k && b.len() >= k * n && out.len() >= m * n);
    // The asserts above keep every read and write inside the slices.
    unsafe {
        cblas_sgemm(
            ROW_MAJOR,
            NO_TRANS,
            NO_TRANS,
            m as i32,
            n as i32,
            k as i32,
            1.0,
            a.as_ptr(),
            k as i32,
            b.as_ptr(),
            n as i32,
            0.0,
            out.as_mut_ptr(),
            n as i32,
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn matmul(a: &[f32], b: &[f32], out: &mut [f32], m: usize, k: usize, n: usize) {
    use rayon::prelude::*;
    out[..m * n]
        .par_chunks_mut(n)
        .enumerate()
        .for_each(|(i, row)| {
            row.fill(0.0);
            for (p, &x) in a[i * k..(i + 1) * k].iter().enumerate() {
                if x != 0.0 {
                    for (o, &w) in row.iter_mut().zip(&b[p * n..(p + 1) * n]) {
                        *o += x * w;
                    }
                }
            }
        });
}

/// A feature map: height, width, channels, row-major HWC.
#[derive(Clone)]
struct Map {
    h: usize,
    w: usize,
    c: usize,
    v: Vec<f32>,
}

impl Map {
    fn concat(maps: &[&Map]) -> Map {
        let (h, w) = (maps[0].h, maps[0].w);
        let c: usize = maps.iter().map(|m| m.c).sum();
        let mut v = Vec::with_capacity(h * w * c);
        for p in 0..h * w {
            for m in maps {
                v.extend_from_slice(&m.v[p * m.c..(p + 1) * m.c]);
            }
        }
        Map { h, w, c, v }
    }
}

/// TF "same" convolution, then ReLU and the folded batch norm when the layer has one.
fn conv(x: &Map, layer: &Conv) -> Map {
    debug_assert_eq!(x.c, layer.cin);
    let (h, w, ci, k) = (x.h, x.w, x.c, layer.k);
    let half = (k / 2) as isize;
    let cols = k * k * ci;
    let mut im = vec![0f32; h * w * cols];
    for y in 0..h {
        for xx in 0..w {
            let row = &mut im[(y * w + xx) * cols..][..cols];
            let mut i = 0;
            for dy in -half..=half {
                for dx in -half..=half {
                    let (sy, sx) = (y as isize + dy, xx as isize + dx);
                    if sy >= 0 && sx >= 0 && (sy as usize) < h && (sx as usize) < w {
                        let at = (sy as usize * w + sx as usize) * ci;
                        row[i..i + ci].copy_from_slice(&x.v[at..at + ci]);
                    }
                    i += ci;
                }
            }
        }
    }
    let mut out = vec![0f32; h * w * layer.cout];
    matmul(&im, &layer.kernel, &mut out, h * w, cols, layer.cout);
    if let Some((scale, shift)) = &layer.norm {
        for px in out.chunks_exact_mut(layer.cout) {
            for (i, v) in px.iter_mut().enumerate() {
                *v = v.max(0.0) * scale[i] + shift[i];
            }
        }
    }
    Map {
        h,
        w,
        c: layer.cout,
        v: out,
    }
}

fn trans(x: &Map, layers: &[Conv; 5]) -> Map {
    let b0 = conv(x, &layers[0]);
    let b1 = conv(&b0, &layers[1]);
    let b2 = conv(&Map::concat(&[&b0, &b1]), &layers[2]);
    let b3 = conv(&Map::concat(&[&b0, &b1, &b2]), &layers[3]);
    conv(&b3, &layers[4])
}

fn avg_pool_half(x: &Map) -> Map {
    let (h, w, c) = (x.h / 2, x.w / 2, x.c);
    let mut v = vec![0f32; h * w * c];
    for y in 0..h {
        for xx in 0..w {
            for ch in 0..c {
                let at = |yy: usize, xs: usize| x.v[(yy * x.w + xs) * c + ch];
                v[(y * w + xx) * c + ch] = (at(2 * y, 2 * xx)
                    + at(2 * y + 1, 2 * xx)
                    + at(2 * y, 2 * xx + 1)
                    + at(2 * y + 1, 2 * xx + 1))
                    * 0.25;
            }
        }
    }
    Map { h, w, c, v }
}

/// Nearest-neighbour doubling, then TF's 2 × 2 stride-1 "same" average (valid cells only).
fn double_and_smooth(x: &Map) -> Map {
    let (h, w, c) = (x.h * 2, x.w * 2, x.c);
    let at = |y: usize, xx: usize, ch: usize| x.v[((y / 2) * x.w + xx / 2) * c + ch];
    let mut v = vec![0f32; h * w * c];
    for y in 0..h {
        for xx in 0..w {
            for ch in 0..c {
                let (mut sum, mut n) = (0.0, 0.0);
                for (dy, dx) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    if y + dy < h && xx + dx < w {
                        sum += at(y + dy, xx + dx, ch);
                        n += 1.0;
                    }
                }
                v[(y * w + xx) * c + ch] = sum / n;
            }
        }
    }
    Map { h, w, c, v }
}

/// What the network says to do with one eye crop.
#[derive(Debug, Clone)]
pub struct EyeWarp {
    /// H × W × 2: where each pixel samples from, added to a -1..1 grid (x, y).
    pub flow: Vec<f32>,
    /// H × W: how much of the warped picture to keep; the rest is white.
    pub keep: Vec<f32>,
}

/// Runs the network on one eye. `eye` is H × W × 3 (RGB, 0..1), `points` H × W × 12, and
/// `angle` the (vertical, horizontal) change of gaze in degrees.
pub fn infer(eye: &[f32], points: &[f32], angle: [f32; 2]) -> EyeWarp {
    let net = net();
    let mut a = angle.to_vec();
    for (kernel, bias, i, o) in &net.dense {
        a = (0..*o)
            .map(|j| (bias[j] + (0..*i).map(|p| a[p] * kernel[p * o + j]).sum::<f32>()).max(0.0))
            .collect();
    }
    let image = Map {
        h: H,
        w: W,
        c: 3,
        v: eye.to_vec(),
    };
    let maps = Map {
        h: H,
        w: W,
        c: POINT_MAPS,
        v: points.to_vec(),
    };
    let angle_map = Map {
        h: H,
        w: W,
        c: 16,
        v: a.repeat(H * W),
    };
    let x = Map::concat(&[&image, &maps, &angle_map]);
    let mut coarse = trans(&avg_pool_half(&x), &net.coarse);
    coarse.v.iter_mut().for_each(|v| *v = v.tanh());
    let coarse = double_and_smooth(&coarse);
    let fine = trans(&Map::concat(&[&x, &coarse]), &net.fine);
    let mut flow = Vec::with_capacity(H * W * 2);
    let mut lcm_in = Vec::with_capacity(H * W * 2);
    for px in fine.v.chunks_exact(4) {
        flow.extend([px[0].tanh(), px[1].tanh()]);
        lcm_in.extend([px[2], px[3]]);
    }
    let lcm = Map {
        h: H,
        w: W,
        c: 2,
        v: lcm_in,
    };
    let logits = conv(&conv(&conv(&lcm, &net.lcm[0]), &net.lcm[1]), &net.lcm[2]);
    let keep = logits
        .v
        .chunks_exact(2)
        .map(|l| {
            let m = l[0].max(l[1]);
            let (a, b) = ((l[0] - m).exp(), (l[1] - m).exp());
            a / (a + b)
        })
        .collect();
    EyeWarp { flow, keep }
}

/// The 12 point maps for six points given in crop pixels (x, y), as the network expects.
pub fn point_maps(points: &[(f32, f32); 6]) -> Vec<f32> {
    let mut v = Vec::with_capacity(H * W * POINT_MAPS);
    let rounded: Vec<(f32, f32)> = points
        .iter()
        .map(|&(x, y)| (x.trunc(), y.trunc()))
        .collect();
    for y in 0..H {
        for x in 0..W {
            for &(px, py) in &rounded {
                v.push(x as f32 - px);
                v.push(y as f32 - py);
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same made-up eye as the NumPy reference used to check this port.
    fn reference_input() -> (Vec<f32>, Vec<f32>) {
        let mut eye = Vec::with_capacity(H * W * 3);
        for y in 0..H {
            for x in 0..W {
                let (xf, yf) = (x as f32, y as f32);
                eye.push(((xf * 0.3).sin() * 0.5 + 0.5) * 0.8);
                eye.push(((yf * 0.25).cos() * 0.5 + 0.5) * 0.7);
                eye.push(((x + y) % 7) as f32 / 7.0);
            }
        }
        let points = point_maps(&[
            (10.0, 26.0),
            (22.0, 20.0),
            (40.0, 20.0),
            (54.0, 26.0),
            (40.0, 31.0),
            (22.0, 31.0),
        ]);
        (eye, points)
    }

    #[test]
    fn matches_the_reference_implementation() {
        let (eye, points) = reference_input();
        let out = infer(&eye, &points, [15.0, 0.0]);
        let flow_sum: f32 = out.flow.iter().sum();
        let keep_sum: f32 = out.keep.iter().sum();
        assert!((flow_sum - -102.919_68).abs() < 0.05, "flow sum {flow_sum}");
        assert!((keep_sum - 3021.767).abs() < 0.05, "keep sum {keep_sum}");
        for (r, c, fx, fy, k) in [
            (0, 0, 0.001_666, 0.000_036, 0.999_996),
            (24, 32, -0.013_129, 0.006_209, 0.956_690),
            (20, 40, -0.014_922, 0.028_516, 0.994_333),
            (47, 63, -0.045_013, -0.047_737, 0.999_997),
            (30, 10, -0.021_402, -0.015_027, 0.992_148),
        ] {
            let i = r * W + c;
            assert!((out.flow[i * 2] - fx).abs() < 1e-4, "flow x at {r},{c}");
            assert!((out.flow[i * 2 + 1] - fy).abs() < 1e-4, "flow y at {r},{c}");
            assert!((out.keep[i] - k).abs() < 1e-4, "keep at {r},{c}");
        }
    }

    #[test]
    fn no_angle_changes_almost_nothing() {
        let (eye, points) = reference_input();
        let still = infer(&eye, &points, [0.0, 0.0]);
        let moved = infer(&eye, &points, [15.0, 0.0]);
        let size = |w: &EyeWarp| w.flow.iter().map(|v| v.abs()).sum::<f32>();
        assert!(size(&still) < size(&moved));
    }
}
