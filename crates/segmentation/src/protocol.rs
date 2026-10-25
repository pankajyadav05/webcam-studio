//! Little-endian, versioned raw IPC. Input: WSI1/id/quality/stream_epoch/JPEG.
//! Output: WSM1/id/width/height/inference_ms/u8 mask. Coordinates are top-left.
use crate::{Mask, Quality, person::Error};
pub const HEADER: usize = 16;
pub fn decode(bytes: &[u8]) -> Result<(u32, Quality, u32, &[u8]), Error> {
    if bytes.len() <= HEADER || bytes.len() > 8 * 1024 * 1024 || &bytes[..4] != b"WSI1" {
        return Err(Error::InvalidImage("invalid WSI1 frame".into()));
    }
    let id = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let quality = match u32::from_le_bytes(bytes[8..12].try_into().unwrap()) {
        0 => Quality::Balanced,
        1 => Quality::Accurate,
        _ => return Err(Error::InvalidImage("unknown quality".into())),
    };
    let epoch = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    Ok((id, quality, epoch, &bytes[HEADER..]))
}
pub fn encode(id: u32, mask: &Mask, ms: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(20 + mask.alpha.len());
    out.extend_from_slice(b"WSM1");
    for v in [id, mask.width, mask.height] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&ms.to_le_bytes());
    out.extend_from_slice(&mask.alpha);
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_headers_and_quality() {
        for b in [vec![], vec![0; 20], b"WSI1".to_vec()] {
            assert!(decode(&b).is_err());
        }
        let mut b = b"WSI1".to_vec();
        b.extend_from_slice(&[0; 13]);
        assert!(decode(&b).is_ok());
        b[8] = 2;
        assert!(decode(&b).is_err());
    }
    #[test]
    fn compact_mask_is_row_major() {
        let out = encode(
            7,
            &Mask {
                width: 2,
                height: 1,
                alpha: vec![0, 255],
            },
            18.5,
        );
        assert_eq!(&out[..4], b"WSM1");
        assert_eq!(out.len(), 22);
        assert_eq!(&out[20..], &[0, 255]);
    }
}
