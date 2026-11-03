//! The DraftNew mark (a gear block), shared by desktop, packaging and CLI.

/// The mark at 256 x 256, straight-alpha RGBA, the master the other sizes are made from.
const MASTER: &[u8] = include_bytes!("app_icon_256.rgba");
const MASTER_SIZE: usize = 256;

/// The mark at `size` x `size` pixels (straight-alpha RGBA): the master averaged down, or
/// sampled up for sizes above 256.
pub fn rgba(size: usize) -> Result<Vec<u8>, String> {
    if !(1..=2048).contains(&size) {
        return Err("icon size must be 1..=2048".into());
    }
    let px = |x: usize, y: usize| -> [u32; 4] {
        let at = (y.min(MASTER_SIZE - 1) * MASTER_SIZE + x.min(MASTER_SIZE - 1)) * 4;
        match MASTER.get(at..at + 4) {
            Some(p) => [u32::from(p[0]), u32::from(p[1]), u32::from(p[2]), u32::from(p[3])],
            None => [0; 4],
        }
    };
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            // The master pixels this one covers (at least one).
            let (x0, x1) = (x * MASTER_SIZE / size, ((x + 1) * MASTER_SIZE / size).max(x * MASTER_SIZE / size + 1));
            let (y0, y1) = (y * MASTER_SIZE / size, ((y + 1) * MASTER_SIZE / size).max(y * MASTER_SIZE / size + 1));
            let (mut sum, mut alpha, mut count) = ([0u32; 3], 0u32, 0u32);
            for j in y0..y1 {
                for i in x0..x1 {
                    let p = px(i, j);
                    count += 1;
                    alpha += p[3];
                    for k in 0..3 {
                        sum[k] += p[k] * p[3];
                    }
                }
            }
            if alpha == 0 || count == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[(sum[0] / alpha) as u8, (sum[1] / alpha) as u8, (sum[2] / alpha) as u8, (alpha / count) as u8]);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_mark_has_transparency_and_original_colors_at_window_and_package_sizes() {
        for size in [16, 24, 32, 48, 64, 128, 256] {
            let pixels = super::rgba(size).expect("valid size");
            assert_eq!(pixels.len(), size * size * 4);
            assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
            assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 255 && p[1] > p[0]));
        }
        assert!(super::rgba(0).is_err());
        assert!(super::rgba(2049).is_err());
    }
}
