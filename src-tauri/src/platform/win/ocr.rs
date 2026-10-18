//! D12: `Windows.Media.Ocr` for each installed language among en / vi / zh-Hans.
use std::path::Path;

use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode, SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};

/// Recognition languages; those without an installed OCR pack are skipped.
const LANGUAGES: [&str; 3] = ["en-US", "vi-VN", "zh-Hans-CN"];

/// The recognised text of the language engine that read the most characters. Engines are
/// per-language on Windows (Vision on macOS reads all languages in one pass); a language whose
/// engine fails is skipped. Blocking; call off the async runtime.
pub fn recognize(path: &Path) -> Result<String, String> {
    let abs = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
    let run = || -> windows::core::Result<String> {
        let bitmap = decode(&abs)?;
        let mut best = String::new();
        for tag in LANGUAGES {
            let Ok(Some(text)) = read_with(tag, &bitmap) else {
                continue;
            };
            if visible_chars(&text) > visible_chars(&best) {
                best = text;
            }
        }
        Ok(best)
    };
    run().map_err(|e| e.to_string())
}

/// Decodes the image, scaled down to fit `OcrEngine::MaxImageDimension` on both axes (the engine
/// rejects larger bitmaps).
fn decode(abs: &str) -> windows::core::Result<SoftwareBitmap> {
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(abs))?.join()?;
    let stream = file.OpenAsync(FileAccessMode::Read)?.join()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let (w, h) = (decoder.PixelWidth()?, decoder.PixelHeight()?);
    let max = OcrEngine::MaxImageDimension()?;
    if w <= max && h <= max {
        return decoder.GetSoftwareBitmapAsync()?.join();
    }
    // Scaling happens before the EXIF rotation; a uniform factor on the longer side fits both.
    let k = f64::from(max) / f64::from(w.max(h));
    let t = BitmapTransform::new()?;
    t.SetScaledWidth(((f64::from(w) * k) as u32).clamp(1, max))?;
    t.SetScaledHeight(((f64::from(h) * k) as u32).clamp(1, max))?;
    t.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    decoder
        .GetSoftwareBitmapTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &t,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()
}

/// The text the `tag` engine reads; `None` when no OCR pack for `tag` is installed.
fn read_with(tag: &str, bitmap: &SoftwareBitmap) -> windows::core::Result<Option<String>> {
    let lang = Language::CreateLanguage(&HSTRING::from(tag))?;
    if !OcrEngine::IsLanguageSupported(&lang)? {
        return Ok(None);
    }
    let text = OcrEngine::TryCreateFromLanguage(&lang)?
        .RecognizeAsync(bitmap)?
        .join()?
        .Text()?;
    Ok(Some(text.to_string()))
}

fn visible_chars(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs an installed OCR language pack, which hosted runners don't guarantee.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn reads_fixture() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/platform/win/fixtures/ocr-hello.png");
        let text = recognize(&fixture).unwrap();
        assert!(text.contains("Hello"), "{text:?}");
    }
}
