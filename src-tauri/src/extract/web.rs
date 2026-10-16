//! Link previews: fetch the page, take its title / og:title / og:image, and turn the readable
//! article into Markdown.
use std::sync::LazyLock;
use std::time::Duration;

use reqwest::header::CONTENT_TYPE;
use scraper::{Html, Selector};

use crate::library;
use crate::paths::Paths;

const PAGE_LIMIT: usize = 5 * 1024 * 1024;
const IMAGE_LIMIT: usize = 2 * 1024 * 1024;

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent("Co-pet/0.1")
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .expect("static reqwest client")
});

pub struct Page {
    pub title: Option<String>,
    pub text: Option<String>,
    /// Library-relative thumbnail made from og:image.
    pub thumb: Option<String>,
}

/// GETs `url`, keeping at most `limit` bytes. The flag tells whether the body was cut.
async fn get_limited(url: &str, limit: usize) -> Result<(Option<String>, Vec<u8>, bool), String> {
    let mut resp = CLIENT.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let mime = resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_ascii_lowercase);
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > limit {
            body.extend_from_slice(&chunk[..limit - body.len()]);
            return Ok((mime, body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((mime, body, false))
}

struct Parsed {
    title: Option<String>,
    og_image: Option<String>,
    markdown: Option<String>,
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

fn parse(html: &str, url: &str) -> Parsed {
    let doc = Html::parse_document(html);
    let meta = |prop: &str| {
        let sel = Selector::parse(&format!("meta[property='{prop}']")).ok()?;
        doc.select(&sel).find_map(|e| e.value().attr("content")).and_then(non_empty)
    };
    let og_title = meta("og:title");
    let og_image = meta("og:image");
    let title = og_title.or_else(|| {
        let sel = Selector::parse("title").ok()?;
        doc.select(&sel).next().and_then(|t| non_empty(&t.text().collect::<String>()))
    });
    let markdown = dom_smoothie::Readability::new(html, Some(url), None)
        .and_then(|mut r| r.parse())
        .ok()
        .and_then(|article| htmd::convert(&article.content).ok())
        .filter(|md| !md.trim().is_empty());
    Parsed { title, og_image, markdown }
}

/// Downloads og:image (≤ 2 MB, image MIME) into the library and returns its thumbnail.
async fn og_thumb(paths: &Paths, page_url: &str, image: &str) -> Option<String> {
    let abs = url::Url::parse(page_url).ok()?.join(image).ok()?;
    let (mime, bytes, cut) = get_limited(abs.as_str(), IMAGE_LIMIT).await.ok()?;
    if cut || !mime.is_some_and(|m| m.starts_with("image/")) {
        return None;
    }
    let ext = infer::get(&bytes).filter(|t| t.mime_type().starts_with("image/"))?.extension();
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || {
        let stored = library::store_bytes(&paths, &bytes, ext).ok()?;
        library::make_thumb(&paths, &stored.rel_path)
    })
    .await
    .ok()
    .flatten()
}

pub async fn fetch(paths: &Paths, url: &str) -> Result<Page, String> {
    // A page over 5 MB is parsed from its first 5 MB.
    let (mime, body, _) = get_limited(url, PAGE_LIMIT).await?;
    if !mime.as_deref().is_none_or(|m| m.contains("html")) {
        return Ok(Page { title: None, text: None, thumb: None });
    }
    let html = String::from_utf8_lossy(&body).into_owned();
    let page_url = url.to_string();
    let parsed = tokio::task::spawn_blocking(move || parse(&html, &page_url)).await.map_err(|e| e.to_string())?;
    let thumb = match &parsed.og_image {
        Some(img) => og_thumb(paths, url, img).await,
        None => None,
    };
    Ok(Page { title: parsed.title, text: parsed.markdown, thumb })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn og_title_wins_over_title() {
        let html = r#"<html><head><title>Plain</title><meta property="og:title" content=" Rich  title "><meta property="og:image" content="/i.png"></head>
            <body><article><h1>Heading</h1><p>First paragraph with enough words to count as readable content for the parser to keep it around.</p>
            <p>Second paragraph, also long enough to be considered part of the main article body by readability.</p></article></body></html>"#;
        let p = parse(html, "https://example.com/post");
        assert_eq!(p.title.as_deref(), Some("Rich title"));
        assert_eq!(p.og_image.as_deref(), Some("/i.png"));
        assert!(p.markdown.unwrap().contains("First paragraph"));
    }

    #[test]
    fn title_fallback() {
        let p = parse("<html><head><title> Only title </title></head><body></body></html>", "https://e.com/");
        assert_eq!(p.title.as_deref(), Some("Only title"));
    }
}
