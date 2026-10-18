//! Windows clipboard / OLE data formats as pure byte codecs, compiled on every OS so they are
//! unit-tested on macOS too. Only `platform::win` uses them at runtime.
#![cfg_attr(not(windows), allow(dead_code))]
use std::path::PathBuf;

const CF_HTML_HEADER: &str =
    "Version:0.9\r\nStartHTML:{SH}\r\nEndHTML:{EH}\r\nStartFragment:{SF}\r\nEndFragment:{EF}\r\n";

/// `HTML Format` (CF_HTML): UTF-8 with a header of 10-digit byte offsets.
pub fn cf_html_encode(html: &str) -> Vec<u8> {
    let pre = "<html><body>\r\n<!--StartFragment-->";
    let post = "<!--EndFragment-->\r\n</body></html>";
    let header_len = CF_HTML_HEADER
        .replace("{SH}", "0000000000")
        .replace("{EH}", "0000000000")
        .replace("{SF}", "0000000000")
        .replace("{EF}", "0000000000")
        .len();
    let sh = header_len;
    let sf = sh + pre.len();
    let ef = sf + html.len();
    let eh = ef + post.len();
    let header = CF_HTML_HEADER
        .replace("{SH}", &format!("{sh:010}"))
        .replace("{EH}", &format!("{eh:010}"))
        .replace("{SF}", &format!("{sf:010}"))
        .replace("{EF}", &format!("{ef:010}"));
    [header.as_bytes(), pre.as_bytes(), html.as_bytes(), post.as_bytes()].concat()
}

/// The fragment of a CF_HTML payload (else the whole HTML part).
pub fn cf_html_decode(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let num = |key: &str| -> Option<usize> {
        let rest = text.split(key).nth(1)?;
        rest.lines().next()?.trim().parse().ok()
    };
    let (start, end) = match (num("StartFragment:"), num("EndFragment:")) {
        (Some(s), Some(e)) => (s, e),
        _ => (num("StartHTML:")?, num("EndHTML:")?),
    };
    let slice = bytes.get(start..end.min(bytes.len()))?;
    Some(String::from_utf8_lossy(slice).into_owned())
}

/// `CF_HDROP` payload: DROPFILES header (20 bytes, wide) + NUL-separated UTF-16 paths + NUL.
pub fn dropfiles_encode(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::with_capacity(20);
    out.extend_from_slice(&20u32.to_le_bytes()); // pFiles
    out.extend_from_slice(&[0u8; 8]); // pt
    out.extend_from_slice(&0i32.to_le_bytes()); // fNC
    out.extend_from_slice(&1i32.to_le_bytes()); // fWide
    for p in paths {
        for u in p.to_string_lossy().encode_utf16().chain(std::iter::once(0)) {
            out.extend_from_slice(&u.to_le_bytes());
        }
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// The paths of a `CF_HDROP` payload (wide or ANSI), up to the double-NUL terminator: a global
/// block may be larger than its payload.
pub fn dropfiles_decode(bytes: &[u8]) -> Vec<PathBuf> {
    let Some(off) = bytes.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize) else {
        return Vec::new();
    };
    let wide = bytes.get(16..20).is_some_and(|b| i32::from_le_bytes(b.try_into().unwrap()) != 0);
    let Some(body) = bytes.get(off..) else { return Vec::new() };
    let text = if wide { utf16_decode_keep_nuls(body) } else { String::from_utf8_lossy(body).into_owned() };
    text.split('\0').take_while(|s| !s.is_empty()).map(PathBuf::from).collect()
}

fn utf16_decode_keep_nuls(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// UTF-16LE up to the first NUL.
pub fn utf16_decode(bytes: &[u8]) -> String {
    let s = utf16_decode_keep_nuls(bytes);
    s.split('\0').next().unwrap_or_default().to_owned()
}

/// `text/x-moz-url`: UTF-16 `url\ntitle`.
pub fn moz_url_decode(bytes: &[u8]) -> Option<(String, Option<String>)> {
    let s = utf16_decode(bytes);
    let mut lines = s.lines();
    let url = lines.next()?.trim().to_owned();
    if url.is_empty() {
        return None;
    }
    let title = lines.next().map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned);
    Some((url, title))
}

/// CF_DIB/CF_DIBV5 → PNG: prepend a BITMAPFILEHEADER and let `image` read the BMP.
pub fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    let header_size = u32::from_le_bytes(dib.get(0..4)?.try_into().ok()?) as usize;
    let bit_count = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
    let compression = u32::from_le_bytes(dib.get(16..20)?.try_into().ok()?);
    let colors_used = u32::from_le_bytes(dib.get(32..36)?.try_into().ok()?) as usize;
    let masks = if compression == 3 && header_size == 40 { 12 } else { 0 }; // BI_BITFIELDS
    let palette = if bit_count <= 8 { (if colors_used == 0 { 1 << bit_count } else { colors_used }) * 4 } else { 0 };
    let pixel_offset = 14 + header_size + masks + palette;
    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0u8; 4]);
    bmp.extend_from_slice(&(pixel_offset as u32).to_le_bytes());
    bmp.extend_from_slice(dib);
    let img = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp).ok()?;
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

/// PNG → CF_DIB: a 24-bpp BMP (40-byte `BITMAPINFOHEADER`) without its 14-byte file header. Alpha
/// is carried by the `PNG` format written alongside.
pub fn png_to_dib(png: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let mut bmp = Vec::new();
    image::DynamicImage::ImageRgb8(img.to_rgb8())
        .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
        .ok()?;
    bmp.get(14..).map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cf_html_round_trips_and_offsets_point_at_the_fragment() {
        let enc = cf_html_encode("<b>hé</b>");
        let s = std::str::from_utf8(&enc).unwrap();
        let off = |k: &str| s.split(k).nth(1).unwrap()[..10].parse::<usize>().unwrap();
        assert_eq!(&enc[off("StartFragment:")..off("EndFragment:")], "<b>hé</b>".as_bytes());
        assert_eq!(cf_html_decode(&enc).as_deref(), Some("<b>hé</b>"));
    }

    #[test]
    fn dropfiles_round_trip_wide_paths() {
        let paths = vec![PathBuf::from(r"C:\Users\Cường\a b.pdf"), PathBuf::from(r"D:\x.txt")];
        assert_eq!(dropfiles_decode(&dropfiles_encode(&paths)), paths);
    }

    #[test]
    fn dropfiles_stop_at_the_terminator() {
        let mut bytes = dropfiles_encode(&[PathBuf::from(r"D:\x.txt")]);
        // Slack of a rounded-up global block.
        bytes.extend("junk\0more".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(dropfiles_decode(&bytes), [PathBuf::from(r"D:\x.txt")]);
    }

    #[test]
    fn moz_url_has_url_then_title() {
        let bytes: Vec<u8> = "https://a.test/\nA page".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(moz_url_decode(&bytes), Some(("https://a.test/".into(), Some("A page".into()))));
    }

    #[test]
    fn dib_and_png_round_trip() {
        let img = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let dib = png_to_dib(&png).unwrap();
        // CF_DIB: BITMAPINFOHEADER (biSize 40), 24 bpp.
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 40);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 24);
        let back = dib_to_png(&dib).unwrap();
        let decoded = image::load_from_memory(&back).unwrap().to_rgba8();
        assert_eq!((decoded.width(), decoded.height()), (3, 2));
        assert_eq!(decoded.get_pixel(2, 1).0, [10, 20, 30, 255]);
    }
}
