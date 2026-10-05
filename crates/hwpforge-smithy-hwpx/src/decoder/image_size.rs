//! Pixel size of an embedded image, read from its header.
//!
//! A group picture whose `<hp:imgDim>` is missing or zero is judged for
//! cropping against its image's own size (pixels × 75 HWPUNIT, 96 dpi);
//! this reads that size from PNG, JPEG, GIF and BMP headers. Any other
//! format, or a header that does not parse, gives `None`.

use hwpforge_core::image::ImageFormat;

/// Pixel sizes of the package's images, keyed by every reference that
/// resolves to one: each stored name (`logo.png`) and each name without its
/// extension (`image1`, Hancom's form), resolved by the crate's one image
/// rule ([`resolve_image_key`](crate::style_lookup_bridge::resolve_image_key):
/// an exact name first, else a stem only one image has). A reference is
/// given the size of the image it resolves to and no other; when that
/// image's header cannot be read, or the reference resolves to none, it
/// gets no size.
pub(crate) fn pixel_sizes_by_reference(
    store: &hwpforge_core::image::ImageStore,
) -> std::collections::HashMap<String, (u32, u32)> {
    let mut candidates: Vec<&str> = Vec::new();
    for (key, _) in store.iter() {
        candidates.push(key);
        if let Some((stem, _)) = key.rsplit_once('.') {
            candidates.push(stem);
        }
    }
    candidates
        .into_iter()
        .filter_map(|reference| {
            let key = crate::style_lookup_bridge::resolve_image_key(store, reference)?;
            let size = pixel_size(store.get(key)?)?;
            Some((reference.to_string(), size))
        })
        .collect()
}

/// `(width, height)` in pixels, or `None` when the header cannot be read.
pub(crate) fn pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let be32 =
        |at: usize| bytes.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let le16 =
        |at: usize| bytes.get(at..at + 2).map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])));
    let le32 =
        |at: usize| bytes.get(at..at + 4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let size = match ImageFormat::sniff(bytes)? {
        // IHDR is always the first chunk: width and height at 16 and 20.
        ImageFormat::Png => (be32(16)?, be32(20)?),
        ImageFormat::Gif => (le16(6)?, le16(8)?),
        ImageFormat::Bmp => match le32(14)? {
            // BITMAPCOREHEADER: 16-bit width and height.
            12 => (le16(18)?, le16(20)?),
            // BITMAPINFOHEADER and later: 32-bit, height negative = top-down.
            _ => (u32::try_from(le32(18)?).ok()?, le32(22)?.unsigned_abs()),
        },
        ImageFormat::Jpeg => jpeg_size(bytes)?,
        _ => return None,
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// The frame size from the first start-of-frame marker of a JPEG.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        // Markers may be padded with any number of 0xFF bytes.
        while *bytes.get(at)? == 0xFF && *bytes.get(at + 1)? == 0xFF {
            at += 1;
        }
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        let len = usize::from(u16::from_be_bytes([*bytes.get(at + 2)?, *bytes.get(at + 3)?]));
        // SOF0–SOF15 except DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes([*bytes.get(at + 5)?, *bytes.get(at + 6)?]);
            let w = u16::from_be_bytes([*bytes.get(at + 7)?, *bytes.get(at + 8)?]);
            return Some((u32::from(w), u32::from(h)));
        }
        if len < 2 {
            return None;
        }
        at += 2 + len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b
    }

    /// 이것을 실패시키는 것: 각 형식의 너비·높이 위치를 바꾸는 변이(형식마다 따로).
    #[test]
    fn reads_png_gif_bmp_and_jpeg_sizes() {
        assert_eq!(pixel_size(&png(94, 33)), Some((94, 33)));
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[0x5e, 0x00, 0x21, 0x00]);
        assert_eq!(pixel_size(&gif), Some((94, 33)));
        // BITMAPINFOHEADER, top-down (negative height).
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0u8; 8]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&94i32.to_le_bytes());
        bmp.extend_from_slice(&(-33i32).to_le_bytes());
        bmp.resize(54, 0);
        assert_eq!(pixel_size(&bmp), Some((94, 33)));
        // SOI, an APP0 segment, then SOF0 (padded marker).
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xFF, 0xC0];
        jpg.extend_from_slice(&[0x00, 0x11, 0x08, 0x00, 0x21, 0x00, 0x5e]);
        assert_eq!(pixel_size(&jpg), Some((94, 33)));
    }

    /// 이것을 실패시키는 것: 0 크기 거르기 삭제 / 잘린 헤더에서 패닉.
    #[test]
    fn unreadable_headers_give_none() {
        assert_eq!(pixel_size(&png(0, 33)), None);
        assert_eq!(pixel_size(&png(94, 33)[..20]), None);
        assert_eq!(pixel_size(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]), None);
        assert_eq!(pixel_size(&[0xD7, 0xCD, 0xC6, 0x9A, 0, 0]), None);
        assert_eq!(pixel_size(b"not an image"), None);
    }

    /// A reference with its extension (`logo.png`) and Hancom's form without
    /// it (`image1`) both find their image; a stem two images share finds
    /// neither.
    ///
    /// 이것을 실패시키는 것: 전체 이름 등록 삭제(`logo.png` 못 찾음) / 줄기 등록 삭제
    /// (`image1` 못 찾음) / 줄기 중복 검사 삭제(`dup` 이 한쪽 크기를 얻음).
    #[test]
    fn sizes_are_found_by_either_reference_form() {
        let mut store = hwpforge_core::image::ImageStore::new();
        store.insert("logo.png", png(94, 33));
        store.insert("image1.png", png(300, 200));
        store.insert("dup.png", png(1, 1));
        store.insert("dup.gif", {
            let mut g = b"GIF89a".to_vec();
            g.extend_from_slice(&[2, 0, 2, 0]);
            g
        });
        let map = pixel_sizes_by_reference(&store);
        assert_eq!(map.get("logo.png"), Some(&(94, 33)));
        assert_eq!(map.get("image1"), Some(&(300, 200)));
        assert_eq!(map.get("dup"), None);
        assert_eq!(map.get("dup.gif"), Some(&(2, 2)));
    }

    /// A reference gets the size of the image the crate's resolver picks and
    /// no other: a stem shared with an image whose header cannot be read
    /// (`dup.png` + `dup.wmf`) is ambiguous as the resolver says, and an
    /// exact name whose own header cannot be read (`logo.wmf`) does not take
    /// the size of another image whose stem it is (`logo.wmf.png`).
    ///
    /// 이것을 실패시키는 것: 크기를 못 읽는 자산을 후보에서 미리 빼는 것(`dup` 이 PNG
    /// 크기를 얻는다) / 해석된 자산 대신 후보 이름이 가리키는 아무 자산의 크기를 쓰는 것
    /// (`logo.wmf` 가 `logo.wmf.png` 크기를 얻는다).
    #[test]
    fn sizes_follow_the_crate_image_resolver() {
        let wmf = vec![0xD7, 0xCD, 0xC6, 0x9A, 0, 0];
        let mut store = hwpforge_core::image::ImageStore::new();
        store.insert("dup.png", png(10, 10));
        store.insert("dup.wmf", wmf.clone());
        store.insert("logo.wmf", wmf);
        store.insert("logo.wmf.png", png(20, 20));
        let map = pixel_sizes_by_reference(&store);
        assert_eq!(map.get("dup"), None);
        assert_eq!(map.get("logo.wmf"), None);
        assert_eq!(map.get("dup.png"), Some(&(10, 10)));
        assert_eq!(map.get("logo.wmf.png"), Some(&(20, 20)));
    }
}
