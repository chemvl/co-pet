//! Extraction, run by the triage worker before rules: OCR + thumbnail for images, link
//! previews for URLs, text from documents. Never panics; failures yield status `failed`.
mod files;
mod ocr;
mod web;

use crate::library;
use crate::model::ItemRow;
use crate::paths::Paths;

const TEXT_CAP: usize = 200_000;

#[derive(Debug, Clone, Default)]
pub struct ExtractOut {
    pub text: Option<String>,
    /// A better title (link previews only).
    pub title: Option<String>,
    /// Library-relative thumbnail.
    pub thumb: Option<String>,
    /// `done | failed | skipped`
    pub status: &'static str,
}

fn cap(text: String) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    Some(match text.char_indices().nth(TEXT_CAP) {
        Some((i, _)) => text[..i].to_string(),
        None => text,
    })
}

fn failed(id: &str, what: &str, e: impl std::fmt::Display) -> ExtractOut {
    eprintln!("[extract] {what} for {id} failed: {e}");
    ExtractOut { status: "failed", ..Default::default() }
}

/// Runs `f` on the blocking pool; a panic inside (e.g. a malformed PDF) becomes an error.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())
}

pub async fn extract(paths: &Paths, item: &ItemRow, local_only: bool) -> ExtractOut {
    match item.kind.as_str() {
        "image" | "screenshot" => {
            let Some(rel) = item.file_path.clone() else { return failed(&item.id, "ocr", "no file") };
            let paths = paths.clone();
            let res = blocking(move || {
                let thumb = library::make_thumb(&paths, &rel);
                (thumb, ocr::recognize(&paths.abs(&rel)))
            })
            .await;
            match res {
                Ok((thumb, Ok(text))) => ExtractOut { text: cap(text), title: None, thumb, status: "done" },
                Ok((thumb, Err(e))) => ExtractOut { thumb, ..failed(&item.id, "ocr", e) },
                Err(e) => failed(&item.id, "ocr", e),
            }
        }
        "url" => {
            if local_only {
                return ExtractOut { status: "skipped", ..Default::default() };
            }
            let Some(url) = item.url.as_deref() else { return failed(&item.id, "link preview", "no url") };
            match web::fetch(paths, url).await {
                Ok(page) => ExtractOut {
                    text: page.text.and_then(cap),
                    title: page.title,
                    thumb: page.thumb,
                    status: "done",
                },
                Err(e) => failed(&item.id, "link preview", e),
            }
        }
        "file" => {
            let Some(rel) = item.file_path.clone() else { return failed(&item.id, "text", "no file") };
            let ext = item
                .file_name
                .as_deref()
                .and_then(|n| n.rsplit_once('.'))
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            let abs = paths.abs(&rel);
            match blocking(move || files::extract_file(&abs, &ext)).await {
                Ok(Ok(Some(text))) => ExtractOut { text: cap(text), status: "done", ..Default::default() },
                Ok(Ok(None)) => ExtractOut { status: "skipped", ..Default::default() },
                Ok(Err(e)) | Err(e) => failed(&item.id, "text", e),
            }
        }
        // text / richtext / code: the payload already is the text.
        _ => ExtractOut { status: "skipped", ..Default::default() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_limits_chars_not_bytes() {
        let capped = cap("đ".repeat(TEXT_CAP + 10)).unwrap();
        assert_eq!(capped.chars().count(), TEXT_CAP);
        assert_eq!(cap("  \n".into()), None);
    }

    #[tokio::test]
    async fn local_only_skips_urls() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        let item = ItemRow { kind: "url".into(), url: Some("https://example.com".into()), ..Default::default() };
        assert_eq!(extract(&paths, &item, true).await.status, "skipped");
    }
}
