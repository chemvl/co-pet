//! Text extraction from stored files, chosen by extension. Blocking; run off the async runtime.
use std::io::{Read, Seek};
use std::path::Path;

use calamine::{open_workbook_auto, Reader};
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;

const TEXT_LIMIT_BYTES: u64 = 1024 * 1024;
const SHEETS: usize = 3;
const SHEET_ROWS: usize = 200;

/// UTF-8 (lossy) files read as-is, up to 1 MB.
const TEXT_EXTS: &[&str] = &[
    "md", "txt", "csv", "json", "yaml", "yml", "toml", "log", "ts", "tsx", "js", "jsx", "mjs", "rs", "py", "go",
    "java", "kt", "swift", "c", "h", "cpp", "cs", "rb", "php", "css", "scss", "sh", "sql",
];

/// `Ok(None)` means the extension has no extractor (`skipped`).
pub fn extract_file(path: &Path, ext: &str) -> Result<Option<String>, String> {
    let text = match ext {
        "pdf" => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            pdf_extract::extract_text_from_mem(&bytes).map_err(|e| e.to_string())?
        }
        "docx" => ooxml_text(&mut open_zip(path)?, &["word/document.xml".to_string()], b"w:t", b"w:p")?,
        "pptx" => {
            let mut zip = open_zip(path)?;
            let slides = slide_names(&zip);
            ooxml_text(&mut zip, &slides, b"a:t", b"a:p")?
        }
        "xlsx" | "xls" | "ods" => sheets_markdown(path)?,
        "html" | "htm" => {
            let html = String::from_utf8_lossy(&std::fs::read(path).map_err(|e| e.to_string())?).into_owned();
            htmd::convert(&html).map_err(|e| e.to_string())?
        }
        e if TEXT_EXTS.contains(&e) => {
            let mut buf = Vec::new();
            std::fs::File::open(path)
                .and_then(|f| f.take(TEXT_LIMIT_BYTES).read_to_end(&mut buf))
                .map_err(|e| e.to_string())?;
            String::from_utf8_lossy(&buf).into_owned()
        }
        _ => return Ok(None),
    };
    Ok(Some(text))
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(f).map_err(|e| e.to_string())
}

/// `ppt/slides/slideN.xml` in slide order (numeric, so slide10 follows slide9).
fn slide_names<R: Read + Seek>(zip: &zip::ZipArchive<R>) -> Vec<String> {
    let mut slides: Vec<(u32, String)> = zip
        .file_names()
        .filter_map(|n| {
            let num = n.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok()?;
            Some((num, n.to_string()))
        })
        .collect();
    slides.sort();
    slides.into_iter().map(|(_, n)| n).collect()
}

/// Concatenates the text of `text_tag` elements, one line per `para_tag`, across `parts`
/// (parts separated by a blank line).
fn ooxml_text<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    parts: &[String],
    text_tag: &[u8],
    para_tag: &[u8],
) -> Result<String, String> {
    let mut out = String::new();
    for part in parts {
        let mut xml = String::new();
        zip.by_name(part).map_err(|e| e.to_string())?.read_to_string(&mut xml).map_err(|e| e.to_string())?;
        if !out.is_empty() {
            out.push('\n');
        }
        let mut reader = XmlReader::from_str(&xml);
        let mut in_text = false;
        loop {
            match reader.read_event().map_err(|e| e.to_string())? {
                Event::Start(e) if e.name().as_ref() == text_tag => in_text = true,
                Event::End(e) if e.name().as_ref() == text_tag => in_text = false,
                Event::End(e) if e.name().as_ref() == para_tag => out.push('\n'),
                Event::Text(t) if in_text => out.push_str(&t.decode().map_err(|e| e.to_string())?),
                Event::GeneralRef(r) if in_text => {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        out.push(c);
                    } else if let Some(s) = r.decode().ok().and_then(|n| quick_xml::escape::resolve_xml_entity(&n)) {
                        out.push_str(s);
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    Ok(out)
}

fn md_cell(s: String) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

/// First 3 sheets, at most 200 rows each, as Markdown tables under `## <sheet>` headings.
fn sheets_markdown(path: &Path) -> Result<String, String> {
    let mut wb = open_workbook_auto(path).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for name in wb.sheet_names().into_iter().take(SHEETS) {
        let range = wb.worksheet_range(&name).map_err(|e| e.to_string())?;
        let rows: Vec<Vec<String>> = range
            .rows()
            .take(SHEET_ROWS)
            .map(|r| r.iter().map(|c| md_cell(c.to_string())).collect())
            .collect();
        let Some(header) = rows.first() else { continue };
        out.push_str(&format!("## {name}\n\n| {} |\n|{}\n", header.join(" | "), "---|".repeat(header.len())));
        for row in &rows[1..] {
            out.push_str(&format!("| {} |\n", row.join(" | ")));
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_zip(path: &Path, entries: &[(&str, &str)]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn docx_paragraphs_become_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.docx");
        write_zip(
            &p,
            &[(
                "word/document.xml",
                r#"<w:document xmlns:w="x"><w:body><w:p><w:r><w:t>Đường </w:t></w:r><w:r><w:t>đến A&amp;B</w:t></w:r></w:p><w:p><w:r><w:t>Line two</w:t></w:r></w:p></w:body></w:document>"#,
            )],
        );
        assert_eq!(extract_file(&p, "docx").unwrap().unwrap(), "Đường đến A&B\nLine two\n");
    }

    #[test]
    fn pptx_slides_in_numeric_order() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.pptx");
        let slide = |t: &str| format!(r#"<p:sld xmlns:a="x" xmlns:p="y"><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>"#);
        write_zip(
            &p,
            &[
                ("ppt/slides/slide10.xml", &slide("ten")),
                ("ppt/slides/slide2.xml", &slide("two")),
                ("ppt/slides/slide1.xml", &slide("one")),
            ],
        );
        assert_eq!(extract_file(&p, "pptx").unwrap().unwrap(), "one\n\ntwo\n\nten\n");
    }

    #[test]
    fn unknown_extension_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        std::fs::write(&p, b"\0\x01").unwrap();
        assert_eq!(extract_file(&p, "bin").unwrap(), None);
    }
}
