//! Development harness: length-prefixed WSI1 on stdin, WSM1 (or ERR:) on stdout.
use std::io::{self, Read, Write};
use studio_segmentation::{PersonSegmenter, VisionSegmenter, protocol};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut segmenter = VisionSegmenter::new()?;
    let mut last_epoch = None;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut size = [0; 4];
        if input.read_exact(&mut size).is_err() {
            break;
        }
        let count = u32::from_le_bytes(size) as usize;
        if count > 8 * 1024 * 1024 {
            return Err("oversized frame".into());
        }
        let mut bytes = vec![0; count];
        input.read_exact(&mut bytes)?;
        let result = (|| {
            let (id, quality, epoch, image) = protocol::decode(&bytes)?;
            if last_epoch != Some(epoch) {
                segmenter.reset()?;
                last_epoch = Some(epoch);
            }
            let start = std::time::Instant::now();
            let mask = segmenter.segment(image, quality)?;
            Ok::<_, studio_segmentation::person::Error>(protocol::encode(
                id,
                &mask,
                start.elapsed().as_secs_f32() * 1000.0,
            ))
        })();
        let result = result.unwrap_or_else(|error| format!("ERR:{error}").into_bytes());
        output.write_all(&(result.len() as u32).to_le_bytes())?;
        output.write_all(&result)?;
        output.flush()?;
    }
    Ok(())
}
