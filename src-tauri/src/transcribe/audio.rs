//! Audio on disk: a 16-bit mono WAV writer for recording tracks that fills silence gaps by wall
//! clock and meters its level, linear resampling, WAV header repair after a crash, conversion
//! through the OS's native converter or symphonia, decoding imported files and mixing tracks.
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::TrackType;
use symphonia::core::io::MediaSourceStream;

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

/// `s` (clamped to −1…1) as a 16-bit sample; `NaN` is 0.
fn to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
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
        self.wav.write_sample(to_i16(s)).map_err(wav_err)?;
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

/// Audio conversion behind a trait so the worker can run with a fake in tests.
pub trait Convert {
    fn wav16k(&self, i: &Path, o: &Path) -> AppResult<()>;
    fn aac(&self, i: &Path, o: &Path) -> AppResult<()>;
}

/// [`Convert`] through the OS's native converter (`crate::platform::audio_convert`).
pub struct NativeConvert;

impl Convert for NativeConvert {
    fn wav16k(&self, i: &Path, o: &Path) -> AppResult<()> {
        crate::platform::audio_convert::wav16k(i, o)
    }

    fn aac(&self, i: &Path, o: &Path) -> AppResult<()> {
        crate::platform::audio_convert::aac(i, o)
    }
}

/// A 16-bit mono WAV as samples in −1…1.
pub fn read_wav_f32(path: &Path) -> AppResult<Vec<f32>> {
    let reader = WavReader::open(path).map_err(wav_err)?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.bits_per_sample != 16 || spec.sample_format != SampleFormat::Int {
        return Err(AppError::msg(format!("wav: {} is not 16-bit mono", path.display())));
    }
    reader
        .into_samples::<i16>()
        .map(|s| s.map(|s| f32::from(s) / f32::from(i16::MAX)).map_err(wav_err))
        .collect()
}

/// Decodes the first audio track of any file symphonia reads into a 16-bit mono WAV (channels
/// averaged) at the source rate. Returns its duration in ms. No audio track, an unsupported
/// codec or no decodable audio is `unsupportedAudio`; on any error no output file is left.
pub fn decode_to_wav(input: &Path, output: &Path) -> AppResult<u64> {
    let result = decode(input, output);
    if result.is_err() {
        discard(output);
    }
    result
}

/// Removes the half-written `path` of a failed conversion; a missing file is fine.
pub(crate) fn discard(path: &Path) {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            eprintln!("[transcribe] removing {}: {e}", path.display())
        }
        _ => {}
    }
}

fn decode(input: &Path, output: &Path) -> AppResult<u64> {
    let unsupported = |e: DecodeError| {
        eprintln!("[transcribe] decode {}: {e}", input.display());
        AppError::Code("unsupportedAudio")
    };
    let mss = MediaSourceStream::new(Box::new(File::open(input)?), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = input.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, Default::default(), Default::default())
        .map_err(unsupported)?;
    let track = format.first_track_known_codec(TrackType::Audio).ok_or(AppError::Code("unsupportedAudio"))?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or(AppError::Code("unsupportedAudio"))?;
    let mut decoder =
        symphonia::default::get_codecs().make_audio_decoder(params, &Default::default()).map_err(unsupported)?;

    // Created with the rate of the first decoded buffer.
    let mut wav: Option<(WavWriter<BufWriter<File>>, u32)> = None;
    let mut frames: u64 = 0;
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // A truncated file: keep what decoded.
            Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(unsupported(e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            // A corrupt packet: skip it.
            Err(DecodeError::DecodeError(_)) => continue,
            Err(e) => return Err(unsupported(e)),
        };
        let channels = buf.spec().channels().count().max(1);
        let rate = buf.spec().rate();
        buf.copy_to_vec_interleaved(&mut interleaved);
        if interleaved.is_empty() {
            continue;
        }
        if wav.is_none() {
            let spec = WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: SampleFormat::Int };
            wav = Some((WavWriter::create(output, spec).map_err(wav_err)?, rate));
        }
        if let Some((writer, _)) = wav.as_mut() {
            for frame in interleaved.chunks_exact(channels) {
                let mono = frame.iter().sum::<f32>() / channels as f32;
                writer.write_sample(to_i16(mono)).map_err(wav_err)?;
            }
        }
        frames += (interleaved.len() / channels) as u64;
    }
    let (writer, rate) = wav.ok_or(AppError::Code("unsupportedAudio"))?;
    writer.finalize().map_err(wav_err)?;
    Ok(frames * 1000 / u64::from(rate))
}

/// Any audio symphonia decodes → 16-bit mono WAV at 16 kHz (the speech engines' input), without
/// an OS converter: decode to mono at the source rate, then resample. Undecodable input is
/// `audioConvert`; on any error no output file is left.
pub fn wav16k_from_any(input: &Path, output: &Path) -> AppResult<()> {
    let mono = output.with_extension("mono.wav");
    decode_to_wav(input, &mono).map_err(|_| AppError::Code("audioConvert"))?;
    let read = WavReader::open(&mono).map_err(wav_err).and_then(|reader| {
        let rate = reader.spec().sample_rate;
        drop(reader);
        Ok((read_wav_f32(&mono)?, rate))
    });
    discard(&mono);
    let (samples, rate) = read?;
    let result = write_wav16k(output, linear_resample(&samples, rate, 16_000));
    if result.is_err() {
        discard(output);
    }
    result
}

fn write_wav16k(output: &Path, samples: Vec<f32>) -> AppResult<()> {
    let spec = WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: SampleFormat::Int };
    let mut w = WavWriter::create(output, spec).map_err(wav_err)?;
    for s in samples {
        w.write_sample(to_i16(s)).map_err(wav_err)?;
    }
    w.finalize().map_err(wav_err)
}

/// Sums two tracks sample by sample, clamped to −1…1; the shorter one is padded with silence.
pub fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    long.iter()
        .enumerate()
        .map(|(i, &x)| (x + short.get(i).copied().unwrap_or(0.0)).clamp(-1.0, 1.0))
        .collect()
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

    #[test]
    fn mix_pads_shorter_track() {
        let a = [0.5, -0.5, 0.25, 0.1];
        let b = [0.75, -0.75];
        let expected = vec![1.0, -1.0, 0.25, 0.1];
        assert_eq!(mix(&a, &b), expected, "sums clamp; the tail of the longer track stays");
        assert_eq!(mix(&b, &a), expected, "order does not matter");
        assert_eq!(mix(&[], &[0.2]), vec![0.2]);
    }

    /// A one-second 44.1 kHz stereo WAV: left 0.5, right −0.1.
    fn stereo_wav(path: &Path) {
        let spec = WavSpec { channels: 2, sample_rate: 44_100, bits_per_sample: 16, sample_format: SampleFormat::Int };
        let mut w = WavWriter::create(path, spec).unwrap();
        for _ in 0..44_100 {
            w.write_sample(to_i16(0.5)).unwrap();
            w.write_sample(to_i16(-0.1)).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn decode_averages_channels_at_source_rate() {
        let dir = tempfile::tempdir().unwrap();
        let (input, output) = (dir.path().join("in.wav"), dir.path().join("out.wav"));
        stereo_wav(&input);
        assert_eq!(decode_to_wav(&input, &output).unwrap(), 1000);
        let reader = WavReader::open(&output).unwrap();
        assert_eq!((reader.spec().channels, reader.spec().sample_rate), (1, 44_100));
        let s = read_wav_f32(&output).unwrap();
        assert_eq!(s.len(), 44_100);
        assert!(s.iter().all(|v| (v - 0.2).abs() < 1e-3), "mono is the channel average: {}", s[0]);
    }

    #[test]
    fn decode_rejects_non_audio() {
        let dir = tempfile::tempdir().unwrap();
        let (input, output) = (dir.path().join("notes.mp3"), dir.path().join("out.wav"));
        std::fs::write(&input, "not audio at all").unwrap();
        assert!(matches!(decode_to_wav(&input, &output), Err(AppError::Code("unsupportedAudio"))));
        assert!(!output.exists());
    }

    #[test]
    fn wav16k_from_any_downmixes_and_resamples() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.wav");
        stereo_wav(&input);
        let out = dir.path().join("out.wav");
        wav16k_from_any(&input, &out).unwrap();
        let r = WavReader::open(&out).unwrap();
        assert_eq!((r.spec().channels, r.spec().sample_rate, r.spec().bits_per_sample), (1, 16_000, 16));
        let n = read_wav_f32(&out).unwrap().len() as i64;
        assert!((n - 16_000).abs() <= 160, "{n} samples");
        assert!(!dir.path().join("out.mono.wav").exists(), "the intermediate file is removed");
    }

    #[test]
    fn native_convert_resamples_and_round_trips_through_aac() {
        let dir = tempfile::tempdir().unwrap();
        let (input, wav16k, m4a, back) = (
            dir.path().join("in.wav"),
            dir.path().join("16k.wav"),
            dir.path().join("out.m4a"),
            dir.path().join("back.wav"),
        );
        stereo_wav(&input);
        NativeConvert.wav16k(&input, &wav16k).unwrap();
        let reader = WavReader::open(&wav16k).unwrap();
        assert_eq!((reader.spec().channels, reader.spec().sample_rate), (1, 16_000));
        let n = read_wav_f32(&wav16k).unwrap().len() as i64;
        assert!((n - 16_000).abs() <= 160, "{n} samples");

        NativeConvert.aac(&wav16k, &m4a).unwrap();
        let ms = decode_to_wav(&m4a, &back).unwrap();
        // symphonia keeps AAC's encoder priming and last-frame padding (isomp4 is not gapless).
        assert!((1000..=1200).contains(&ms), "AAC decodes back to about a second: {ms} ms");

        let missing = dir.path().join("missing.wav");
        let out = dir.path().join("never.wav");
        assert!(matches!(NativeConvert.wav16k(&missing, &out), Err(AppError::Code("audioConvert"))));
        assert!(!out.exists());
    }
}
