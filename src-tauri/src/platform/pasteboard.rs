//! The single reader/writer for the drag pasteboard and the general pasteboard (the clipboard),
//! shared by drop, clipboard, Send selection, copy and paste. Call `read`/`write`/`change_count`
//! on the main thread; elsewhere use `write_on_main`.
use std::path::PathBuf;

pub use super::os::pasteboard::{change_count, read, read_image, write_concealed_text, write_on_main};

pub const TYPE_URL: &str = "public.url";
pub const TYPE_URL_NAME: &str = "public.url-name";
pub const TYPE_PNG: &str = "public.png";
pub const TYPE_TIFF: &str = "public.tiff";
pub const TYPE_HTML: &str = "public.html";
pub const TYPE_PLAIN: &str = "public.utf8-plain-text";
/// nspasteboard.org markers set by password managers and other privacy-aware apps.
pub const TYPE_CONCEALED: &str = "org.nspasteboard.ConcealedType";
pub const TYPE_TRANSIENT: &str = "org.nspasteboard.TransientType";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    General,
    Drag,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PbContent {
    Files(Vec<PathBuf>),
    Url { url: String, title: Option<String> },
    /// PNG bytes (TIFF is converted).
    Image(Vec<u8>),
    Text { plain: String, html: Option<String> },
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // fields are read by capture (Step 6)
pub struct PbRead {
    /// The highest-precedence content, or `None` when nothing usable is on the board.
    pub content: Option<PbContent>,
    /// Every type on the board (UTIs), including privacy markers.
    pub types: Vec<String>,
    pub change_count: isize,
}

impl PbRead {
    /// The writer marked the contents as concealed or transient (password managers).
    pub fn concealed(&self) -> bool {
        self.types.iter().any(|t| t == TYPE_CONCEALED || t == TYPE_TRANSIENT)
    }
}

/// A plain text that is exactly one http(s) URL is a link, not a note.
pub fn text_content(plain: String, html: Option<String>) -> PbContent {
    let trimmed = plain.trim();
    let single_url = !trimmed.contains(char::is_whitespace)
        && url::Url::parse(trimmed).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
    if single_url {
        PbContent::Url { url: trimmed.to_string(), title: None }
    } else {
        PbContent::Text { plain, html }
    }
}
