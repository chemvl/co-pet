//! Speech-to-text benchmark: transcribes audio files with the app's engines and, where a
//! reference `<file stem>.txt` sits next to a file, scores it.
//!
//! cargo run --release --example stt_bench -- [--models <dir>] [--lang auto|en|vi|zh|vi+en]
//!     [--greedy] [--diarize] <files…>
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use copet_lib::transcribe::diarize::{Diarizer, SherpaDiarizer};
use copet_lib::transcribe::stt::{SpeechToText, WhisperStt};
use copet_lib::transcribe::{assemble, audio, models, wer};

const USAGE: &str = "usage: stt_bench [--models <dir>] [--lang auto|en|vi|zh|vi+en] [--greedy] [--diarize] <files…>";

struct Args {
    models: PathBuf,
    lang: String,
    greedy: bool,
    diarize: bool,
    files: Vec<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let mut args = Args {
        models: Path::new(&home).join("Library/Application Support/com.copet.app/models"),
        lang: "auto".to_owned(),
        greedy: false,
        diarize: false,
        files: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--models" => args.models = it.next().ok_or("--models needs a directory")?.into(),
            "--lang" => {
                args.lang = it.next().ok_or("--lang needs a value")?;
                if !["auto", "en", "vi", "zh", "vi+en"].contains(&args.lang.as_str()) {
                    return Err(format!("unknown language {}", args.lang));
                }
            }
            "--greedy" => args.greedy = true,
            "--diarize" => args.diarize = true,
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            file => args.files.push(file.into()),
        }
    }
    if args.files.is_empty() {
        return Err("no input files".to_owned());
    }
    Ok(args)
}

fn mmss(ms: i64) -> String {
    format!("{:02}:{:02}", ms / 60_000, ms / 1000 % 60)
}

/// The scored result of one file.
struct Row {
    lang: String,
    score: f64,
}

fn bench(
    file: &Path,
    args: &Args,
    stt: &mut WhisperStt,
    diarizer: Option<&SherpaDiarizer>,
) -> Result<Option<Row>, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let raw = tmp.path().join("raw.wav");
    let wav = tmp.path().join("16k.wav");
    audio::decode_to_wav(file, &raw).map_err(|e| format!("decode: {e}"))?;
    audio::afconvert(&raw, &wav, audio::Target::Wav16k).map_err(|e| format!("afconvert: {e}"))?;
    let samples = audio::read_wav_f32(&wav).map_err(|e| format!("read: {e}"))?;
    let audio_secs = samples.len() as f64 / 16_000.0;

    let started = Instant::now();
    let (segs, detected) = stt
        .transcribe(&samples, &args.lang, &mut |_| {}, &AtomicBool::new(false))
        .map_err(|e| format!("transcribe: {e}"))?;
    let turns = match diarizer {
        Some(d) => Some(d.diarize(&samples).map_err(|e| format!("diarize: {e}"))?),
        None => None,
    };
    let wall = started.elapsed().as_secs_f64();

    let lang = match args.lang.as_str() {
        "auto" => detected.unwrap_or_else(|| "auto".to_owned()),
        "vi+en" => "vi".to_owned(),
        other => other.to_owned(),
    };
    let hyp_text = segs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
    let lines: Vec<String> = match &turns {
        Some(turns) => {
            let (segs, _) = assemble::assemble(Vec::new(), segs, Some(turns), "import", &lang);
            segs.iter().map(|s| format!("[{}] {}: {}", mmss(s.start_ms), s.speaker, s.text)).collect()
        }
        None => segs.iter().map(|s| format!("[{}] {}", mmss(s.start_ms), s.text)).collect(),
    };
    let mut hyp_path = file.as_os_str().to_owned();
    hyp_path.push(".hyp.txt");
    std::fs::write(&hyp_path, lines.join("\n") + "\n").map_err(|e| format!("write hypothesis: {e}"))?;

    let Ok(reference) = std::fs::read_to_string(file.with_extension("txt")) else {
        return Ok(None);
    };
    let (metric, score) = if lang == "zh" {
        ("CER", wer::cer(&reference, &hyp_text))
    } else {
        ("WER", wer::wer(&reference, &hyp_text))
    };
    println!(
        "{} | {lang} | {:.2} | {:.3} ({wall:.1}s / {audio_secs:.1}s) | {metric} {:.1}%",
        file.display(),
        audio_secs / 60.0,
        wall / audio_secs.max(f64::EPSILON),
        score * 100.0
    );
    Ok(Some(Row { lang, score }))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let [whisper, vad] = &models::SPEECH;
    let mut stt = match WhisperStt::load(&args.models.join(whisper.file), &args.models.join(vad.file)) {
        Ok(stt) => stt,
        Err(e) => {
            eprintln!("loading speech models from {}: {e}", args.models.display());
            return ExitCode::FAILURE;
        }
    };
    stt.greedy = args.greedy;
    let diarizer = if args.diarize {
        let [segmentation, embedding] = &models::SPEAKER;
        match SherpaDiarizer::load(&args.models.join(segmentation.file), &args.models.join(embedding.file)) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("loading speaker models from {}: {e}", args.models.display());
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    println!("file | lang | audio min | RTF (wall / audio) | WER or CER (zh)");
    let mut by_lang: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut failed = false;
    for file in &args.files {
        match bench(file, &args, &mut stt, diarizer.as_ref()) {
            Ok(Some(row)) => by_lang.entry(row.lang).or_default().push(row.score),
            Ok(None) => println!("{} | no reference, hypothesis written", file.display()),
            Err(e) => {
                eprintln!("{}: {e}", file.display());
                failed = true;
            }
        }
    }
    for (lang, scores) in &by_lang {
        let mean = scores.iter().sum::<f64>() / scores.len() as f64;
        let metric = if lang == "zh" { "CER" } else { "WER" };
        println!("mean {lang} ({} files): {metric} {:.1}%", scores.len(), mean * 100.0);
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
