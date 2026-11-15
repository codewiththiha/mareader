//! Shared measurements and file formats for the standalone voice parity tools.

use std::path::Path;

pub fn read_f32(path: &Path) -> std::io::Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    let (chunks, _) = bytes.as_chunks::<4>();
    Ok(chunks.iter().map(|&c| f32::from_le_bytes(c)).collect())
}

pub fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mean = |v: &[f32]| v[..n].iter().map(|&x| f64::from(x)).sum::<f64>() / n as f64;
    let (ma, mb) = (mean(a), mean(b));
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for (&x, &y) in a[..n].iter().zip(&b[..n]) {
        let (x, y) = (f64::from(x) - ma, f64::from(y) - mb);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    num / (da.sqrt() * db.sqrt()).max(1e-12)
}

/// Loudness per 10 ms, so timing near-misses still compare.
pub fn envelope(v: &[f32]) -> Vec<f32> {
    v.chunks(240)
        .map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt())
        .collect()
}

pub fn edits(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|&s| ((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes())
        .collect();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    std::fs::write(path, out)?;
    Ok(())
}
