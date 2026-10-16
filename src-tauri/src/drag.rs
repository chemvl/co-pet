//! Drag-out, Copy and Paste into field (Step 7). One format decision (`plan`) feeds the native
//! drag (`prepare_drag`) and the general pasteboard (`copy_items`).
use std::collections::BTreeMap;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::db::{items, Db};
use crate::error::{AppError, AppResult};
use crate::library;
use crate::model::{DragPlan, ItemRow};
use crate::paths::Paths;
use crate::platform::macos::pasteboard::{self, TYPE_HTML, TYPE_PLAIN, TYPE_URL, TYPE_URL_NAME};
use crate::platform::macos::{keys, on_main, permissions};

/// Lets the user's app take key status back before Cmd+V.
const PASTE_DELAY: Duration = Duration::from_millis(120);
/// Larger images are put on the pasteboard as a file only.
const COPY_PNG_MAX_SOURCE: u64 = 40 * 1024 * 1024;

pub async fn prepare_drag(app: &AppHandle, ids: Vec<String>, as_files: bool) -> AppResult<DragPlan> {
    crate::platform::macos::drop_target::mark_own_drag();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let rows = items::get_rows(&handle.state::<Db>().conn(), &ids)?;
        plan(&handle.state::<Paths>(), &rows, as_files)
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

/// Writes the drag formats to the general pasteboard. Image files also carry `public.png`.
pub async fn copy_items(app: &AppHandle, ids: Vec<String>) -> AppResult<()> {
    let handle = app.clone();
    let (plan, png) = tauri::async_runtime::spawn_blocking(move || -> AppResult<_> {
        let rows = items::get_rows(&handle.state::<Db>().conn(), &ids)?;
        let paths = handle.state::<Paths>();
        let plan = plan(&paths, &rows, false)?;
        let png = match (&plan, rows.first()) {
            (DragPlan::Files(_), Some(first)) if is_image(first) => image_png(&paths, first),
            _ => None,
        };
        Ok((plan, png))
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;
    match plan {
        DragPlan::Files(files) => {
            pasteboard::write_on_main(app, files.into_iter().map(Into::into).collect(), BTreeMap::new(), png).await
        }
        DragPlan::Data { data, .. } => pasteboard::write_on_main(app, Vec::new(), data, None).await,
    }
}

/// Copy, hand focus back to the user's app, then Cmd+V. `"needsAccessibility"` when Co-pet may not
/// send keys (the items stay on the clipboard).
pub async fn paste_items(app: &AppHandle, ids: Vec<String>) -> AppResult<&'static str> {
    copy_items(app, ids).await?;
    let handle = app.clone();
    if let Err(e) = on_main(app, move |_| crate::windows::resign_pet_panel(&handle)).await? {
        eprintln!("[drag] resign pet panel: {e}");
    }
    tokio::time::sleep(PASTE_DELAY).await;
    if !permissions::accessibility_trusted() {
        return Ok("needsAccessibility");
    }
    keys::command_key(app, 'v').await?;
    Ok("ok")
}

/// - only file/image/screenshot items, no Alt → their files
/// - one url or text item, no Alt → typed data
/// - Alt, or any other selection → files, with text as `.md` and links as `.webloc`
fn plan(paths: &Paths, rows: &[ItemRow], as_files: bool) -> AppResult<DragPlan> {
    if rows.is_empty() {
        return Err(AppError::Code("invalid"));
    }
    if !as_files {
        if let [row] = rows {
            if let Some((data, types)) = typed_data(row) {
                return Ok(DragPlan::Data { data, types });
            }
        }
    }
    let files = rows
        .iter()
        .map(|row| {
            let path = match row.kind.as_str() {
                "file" | "image" | "screenshot" => library::dragout_path(paths, row)?,
                "url" => library::temp_text_file(paths, &slug(&row.title), "webloc", &webloc(url_of(row)))?,
                _ => library::temp_text_file(paths, &slug(&row.title), "md", &markdown(row))?,
            };
            path.into_os_string().into_string().map_err(|p| AppError::msg(format!("non-UTF-8 path {p:?}")))
        })
        .collect::<AppResult<_>>()?;
    Ok(DragPlan::Files(files))
}

/// UTI → string pairs and their order for a single url or text item; `None` for file kinds.
fn typed_data(row: &ItemRow) -> Option<(BTreeMap<String, String>, Vec<String>)> {
    let pairs: Vec<(&str, String)> = match row.kind.as_str() {
        "url" => {
            let url = url_of(row);
            vec![
                (TYPE_URL, url.to_string()),
                (TYPE_URL_NAME, row.title.clone()),
                (TYPE_PLAIN, url.to_string()),
                (TYPE_HTML, format!("<a href=\"{}\">{}</a>", escape_html(url), escape_html(&row.title))),
            ]
        }
        "text" | "richtext" | "code" => {
            let plain = row.payload_text.clone().unwrap_or_default();
            let html = row.payload_html.clone().unwrap_or_else(|| format!("<pre>{}</pre>", escape_html(&plain)));
            vec![(TYPE_PLAIN, plain), (TYPE_HTML, html)]
        }
        _ => return None,
    };
    let types = pairs.iter().map(|(t, _)| t.to_string()).collect();
    Some((pairs.into_iter().map(|(t, v)| (t.to_string(), v)).collect(), types))
}

fn url_of(row: &ItemRow) -> &str {
    row.url.as_deref().unwrap_or(&row.title)
}

fn is_image(row: &ItemRow) -> bool {
    matches!(row.kind.as_str(), "image" | "screenshot")
}

/// The blob as PNG bytes (re-encoded when stored in another format); `None` when it can't be
/// decoded or is too large.
fn image_png(paths: &Paths, row: &ItemRow) -> Option<Vec<u8>> {
    let abs = paths.abs(row.file_path.as_deref()?);
    if std::fs::metadata(&abs).ok()?.len() > COPY_PNG_MAX_SOURCE {
        return None;
    }
    let bytes = std::fs::read(&abs).ok()?;
    if image::guess_format(&bytes).ok()? == image::ImageFormat::Png {
        return Some(bytes);
    }
    let img = image::load_from_memory(&bytes).ok()?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// A text item as a Markdown file: rich text converted from its HTML, everything else as is.
fn markdown(row: &ItemRow) -> String {
    let plain = row.payload_text.clone().unwrap_or_default();
    match (&row.kind[..], &row.payload_html) {
        ("richtext", Some(html)) => htmd::convert(html).ok().filter(|m| !m.trim().is_empty()).unwrap_or(plain),
        _ => plain,
    }
}

fn webloc(url: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\t<key>URL</key>\n\t<string>{}</string>\n</dict>\n</plist>\n",
        escape_html(url)
    )
}

/// A file-name slug: lowercase letters and digits (any script) joined by `-`, at most 60 chars.
fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let cut: String = out.chars().take(60).collect();
    let cut = cut.trim_end_matches('-');
    if cut.is_empty() { "item".into() } else { cut.to_string() }
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure().unwrap();
        (dir, paths)
    }

    fn url_row() -> ItemRow {
        ItemRow {
            kind: "url".into(),
            title: "Tom & \"Jerry\"".into(),
            url: Some("https://example.com/a?b=1&c=2".into()),
            ..ItemRow::default()
        }
    }

    fn text_row(kind: &str, plain: &str, html: Option<&str>) -> ItemRow {
        ItemRow {
            kind: kind.into(),
            title: "Ghi chú: họp 14:00".into(),
            payload_text: Some(plain.into()),
            payload_html: html.map(Into::into),
            ..ItemRow::default()
        }
    }

    fn file_row(paths: &Paths, name: &str, kind: &str) -> ItemRow {
        let stored = library::store_bytes(paths, name.as_bytes(), "pdf").unwrap();
        ItemRow {
            kind: kind.into(),
            title: name.into(),
            file_name: Some(name.into()),
            file_path: Some(stored.rel_path),
            ..ItemRow::default()
        }
    }

    fn files(plan: DragPlan) -> Vec<std::path::PathBuf> {
        match plan {
            DragPlan::Files(f) => f.into_iter().map(Into::into).collect(),
            other => panic!("expected files, got {other:?}"),
        }
    }

    #[test]
    fn single_url_is_typed_link_data() {
        let (_d, paths) = paths();
        let DragPlan::Data { data, types } = plan(&paths, &[url_row()], false).unwrap() else { panic!() };
        assert_eq!(types, [TYPE_URL, TYPE_URL_NAME, TYPE_PLAIN, TYPE_HTML]);
        assert_eq!(data[TYPE_URL], "https://example.com/a?b=1&c=2");
        assert_eq!(data[TYPE_PLAIN], "https://example.com/a?b=1&c=2");
        assert_eq!(data[TYPE_URL_NAME], "Tom & \"Jerry\"");
        assert_eq!(data[TYPE_HTML], "<a href=\"https://example.com/a?b=1&amp;c=2\">Tom &amp; &quot;Jerry&quot;</a>");
    }

    #[test]
    fn single_text_is_plain_plus_html() {
        let (_d, paths) = paths();
        let DragPlan::Data { data, types } = plan(&paths, &[text_row("code", "if a < b {}", None)], false).unwrap()
        else {
            panic!()
        };
        assert_eq!(types, [TYPE_PLAIN, TYPE_HTML]);
        assert_eq!(data[TYPE_PLAIN], "if a < b {}");
        assert_eq!(data[TYPE_HTML], "<pre>if a &lt; b {}</pre>");
        let rich = text_row("richtext", "Bold", Some("<b>Bold</b>"));
        let DragPlan::Data { data, .. } = plan(&paths, &[rich], false).unwrap() else { panic!() };
        assert_eq!(data[TYPE_HTML], "<b>Bold</b>");
    }

    #[test]
    fn file_items_drag_their_files_under_the_real_name() {
        let (_d, paths) = paths();
        let rows = [file_row(&paths, "report.pdf", "file"), file_row(&paths, "shot.pdf", "screenshot")];
        let f = files(plan(&paths, &rows, false).unwrap());
        assert_eq!(f.len(), 2);
        assert!(f[0].ends_with("report.pdf") && f[0].starts_with(&paths.tmp));
        assert_eq!(std::fs::read(&f[0]).unwrap(), b"report.pdf");
        assert!(f[1].ends_with("shot.pdf"));
    }

    #[test]
    fn alt_turns_text_and_links_into_files() {
        let (_d, paths) = paths();
        let f = files(plan(&paths, &[text_row("richtext", "Bold", Some("<p><b>Bold</b></p>"))], true).unwrap());
        assert_eq!(f[0].file_name().unwrap(), "ghi-chú-họp-14-00.md");
        assert_eq!(std::fs::read_to_string(&f[0]).unwrap().trim(), "**Bold**");
        let f = files(plan(&paths, &[url_row()], true).unwrap());
        assert_eq!(f[0].file_name().unwrap(), "tom-jerry.webloc");
        let plist = std::fs::read_to_string(&f[0]).unwrap();
        assert!(plist.contains("<key>URL</key>\n\t<string>https://example.com/a?b=1&amp;c=2</string>"));
    }

    #[test]
    fn mixed_or_multi_selection_becomes_files() {
        let (_d, paths) = paths();
        let rows = [url_row(), file_row(&paths, "a.pdf", "file"), text_row("text", "hello", None)];
        let f = files(plan(&paths, &rows, false).unwrap());
        let names: Vec<_> = f.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["tom-jerry.webloc", "a.pdf", "ghi-chú-họp-14-00.md"]);
        assert_eq!(std::fs::read_to_string(&f[2]).unwrap(), "hello");
        // Two links can't share one typed-data item either.
        assert_eq!(files(plan(&paths, &[url_row(), url_row()], false).unwrap()).len(), 2);
    }
}
