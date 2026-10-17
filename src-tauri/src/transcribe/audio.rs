//! Recording tracks on disk: a 16-bit mono WAV writer that fills silence gaps by wall clock and
//! meters its level, linear resampling, and WAV header repair after a crash.
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};

use crate::error::{AppError, AppResult};

/// Seconds of audio between header rewrites, so a crash leaves a playable file.
const FLUSH_EVERY_SECS: u64 = 5;
/// hound's header for 16-bit mono PCM: `RIFF` size at 4, `data` tag at 36, data size at 40.
const HEADER_LEN: u64 = 44;

fn wav_err(e: hound::Error) -> AppError {
    match e {
        hound::Error::IoError(e) => AppError::Io(e),
        e => AppError::msg(format!("wav: {e}")),
    }
}

/// One track's `.wav` (mono, 16-bit, `rate`).
pub struct TrackWriter {
    wav: WavWriter<BufWriter<File>>,
    rate: u32,
    /// Samples written, silence padding included.
    written: u64,
    /// `written` at the last header rewrite.
    flushed: u64,
    /// The last 100 ms of samples, for `level`.
    level_window: VecDeque<f32>,
}

impl TrackWriter {
    pub fn create(path: &Path, rate: u32) -> AppResult<Self> {
        let spec = WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: SampleFormat::Int };
        Ok(TrackWriter {
            wav: WavWriter::create(path, spec).map_err(wav_err)?,
            rate,
            written: 0,
            flushed: 0,
            level_window: VecDeque::with_capacity(Self::window_len(rate)),
        })
    }

    fn window_len(rate: u32) -> usize {
        (rate / 10).max(1) as usize
    }

    /// Appends `samples` received after `active` recording time. When the file is more than
    /// 100 ms behind the wall clock (a device that sends nothing during silence), zeros fill the
    /// gap first so every track stays aligned to the same clock.
    pub fn push(&mut self, samples: &[f32], active: Duration) -> AppResult<()> {
        let expected = (active.as_nanos() * u128::from(self.rate) / 1_000_000_000) as u64;
        let have = self.written + samples.len() as u64;
        if expected > have + u64::from(self.rate / 10) {
            for _ in have..expected {
                self.write(0.0)?;
            }
        }
        for &s in samples {
            self.write(s)?;
        }
        if self.written - self.flushed >= u64::from(self.rate) * FLUSH_EVERY_SECS {
            self.wav.flush().map_err(wav_err)?;
            self.flushed = self.written;
        }
        Ok(())
    }

    fn write(&mut self, s: f32) -> AppResult<()> {
        let s = s.clamp(-1.0, 1.0);
        // `NaN as i16` is 0.
        self.wav.write_sample((s * f32::from(i16::MAX)) as i16).map_err(wav_err)?;
        self.written += 1;
        if self.level_window.len() == Self::window_len(self.rate) {
            self.level_window.pop_front();
        }
        self.level_window.push_back(s);
        Ok(())
    }

    /// The last 100 ms as 0–1: −60 dBFS (or silence) is 0, full scale is 1.
    pub fn level(&self) -> f32 {
        if self.level_window.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.level_window.iter().map(|s| s * s).sum();
        let rms = (sum / self.level_window.len() as f32).sqrt();
        ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    /// Writes the final header; returns the samples written.
    pub fn finish(self) -> AppResult<u64> {
        let written = self.written;
        self.wav.finalize().map_err(wav_err)?;
        Ok(written)
    }
}

/// Linear interpolation from `from` Hz to `to` Hz.
pub fn linear_resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let out_len = (samples.len() as f64 * f64::from(to) / f64::from(from)).round() as usize;
    let step = f64::from(from) / f64::from(to);
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let j = (pos as usize).min(samples.len() - 1);
            let frac = (pos - j as f64) as f32;
            let a = samples[j];
            let b = samples.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

/// Rewrites the RIFF and `data` sizes of a 16-bit mono WAV written by [`TrackWriter`] from the
/// file length (a half-written last sample is cut). Returns the samples in the file; a file
/// shorter than its header holds none and is left as is.
#[allow(dead_code)] // crash recovery in `transcribe::start` (Task 4.4)
pub fn fix_wav_header(path: &Path) -> AppResult<u64> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let len = f.metadata()?.len();
    if len < HEADER_LEN {
        return Ok(0);
    }
    let mut tag = [0u8; 4];
    f.seek(SeekFrom::Start(36))?;
    f.read_exact(&mut tag)?;
    if &tag != b"data" {
        return Err(AppError::msg(format!("wav: unexpected header in {}", path.display())));
    }
    let data = ((len - HEADER_LEN) & !1).min(u64::from(u32::MAX) - (HEADER_LEN - 8));
    f.set_len(HEADER_LEN + data)?;
    f.seek(SeekFrom::Start(4))?;
    f.write_all(&((data + HEADER_LEN - 8) as u32).to_le_bytes())?;
    f.seek(SeekFrom::Start(40))?;
    f.write_all(&(data as u32).to_le_bytes())?;
    f.sync_all()?;
    Ok(data / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;

    fn read_samples(path: &Path) -> Vec<i16> {
        hound::WavReader::open(path).unwrap().samples::<i16>().map(Result::unwrap).collect()
    }

    #[test]
    fn gap_fill_pads_silence_by_wall_clock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let mut w = TrackWriter::create(&path, RATE).unwrap();
        w.push(&[0.5; 1600], Duration::from_millis(100)).unwrap();
        // Nothing arrived for a second, then 100 ms of audio.
        w.push(&[0.5; 1600], Duration::from_millis(1100)).unwrap();
        assert_eq!(w.written(), 17_600);
        assert_eq!(w.finish().unwrap(), 17_600);

        let s = read_samples(&path);
        assert_eq!(s.len(), 17_600);
        assert!(s[..1600].iter().all(|&v| v == 16_383));
        assert!(s[1600..16_000].iter().all(|&v| v == 0), "the gap is silence");
        assert!(s[16_000..].iter().all(|&v| v == 16_383), "new audio lands at its wall-clock time");
    }

    #[test]
    fn no_pad_within_100ms() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let mut w = TrackWriter::create(&path, RATE).unwrap();
        w.push(&[0.25; 1600], Duration::from_millis(100)).unwrap();
        // 100 ms behind the clock exactly: jitter, not a gap.
        w.push(&[0.25; 1600], Duration::from_millis(300)).unwrap();
        assert_eq!(w.written(), 3200);
        // Ahead of the clock (buffers arriving early) never pads either.
        w.push(&[0.25; 1600], Duration::from_millis(200)).unwrap();
        assert_eq!(w.finish().unwrap(), 4800);
        assert!(read_samples(&path).iter().all(|&v| v != 0));
    }

    #[test]
    fn level_of_silence_is_zero_and_full_scale_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = TrackWriter::create(&dir.path().join("t.wav"), RATE).unwrap();
        assert_eq!(w.level(), 0.0, "nothing written yet");
        w.push(&[0.0; 3200], Duration::from_millis(200)).unwrap();
        assert_eq!(w.level(), 0.0);
        w.push(&[1.0; 1600], Duration::from_millis(300)).unwrap();
        assert!((w.level() - 1.0).abs() < 1e-6, "level {}", w.level());
        // Only the last 100 ms count: −20 dBFS after a full-scale burst reads 2/3.
        w.push(&[0.1; 1600], Duration::from_millis(400)).unwrap();
        assert!((w.level() - 2.0 / 3.0).abs() < 1e-3, "level {}", w.level());
    }

    #[test]
    fn fix_header_recovers_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let mut w = TrackWriter::create(&path, RATE).unwrap();
        w.push(&[0.5; 8000], Duration::from_millis(500)).unwrap();
        w.finish().unwrap();
        // A crash: 4000 more samples and half of one reached the disk, the header says zero.
        let mut f = OpenOptions::new().read(true).write(true).open(&path).unwrap();
        f.seek(SeekFrom::End(0)).unwrap();
        f.write_all(&[0x10; 8001]).unwrap();
        for offset in [4, 40] {
            f.seek(SeekFrom::Start(offset)).unwrap();
            f.write_all(&0u32.to_le_bytes()).unwrap();
        }
        drop(f);

        assert_eq!(fix_wav_header(&path).unwrap(), 12_000);
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.len(), 12_000);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), HEADER_LEN + 24_000);
        let s = read_samples(&path);
        assert_eq!((s[0], s[7999], s[8000], s[11_999]), (16_383, 16_383, 0x1010, 0x1010));
    }
}
