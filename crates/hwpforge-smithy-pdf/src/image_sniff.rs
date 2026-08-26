//! 이미지 바이트 magic 스니핑 (W2a — §3 D2).
//!
//! krilla 생성자는 포맷별로 분리돼 있고 auto-detect 가 없다 — 그리고
//! 한컴 문서의 파일명/확장자는 신뢰할 수 없다 (`ImageFormat` 은 확장자
//! 유래라 진단 힌트일 뿐). 렌더 경로의 포맷 판별은 **오직 이 스니퍼**가
//! 소유한다 (확장자 fallback 금지).

/// magic 스니핑 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sniffed {
    /// PNG (`\x89PNG\r\n\x1a\n`).
    Png,
    /// JPEG (`FF D8 FF`).
    Jpeg,
    /// GIF (`GIF87a`/`GIF89a`).
    Gif,
    /// WebP (`RIFF….WEBP`).
    Webp,
    /// magic 은 알지만 렌더 불가 (BMP/WMF/EMF).
    ///
    /// **W6 확정 정책 (§12a)**: strict = typed
    /// `PdfError::UnsupportedImageFormat` / `--degraded` = 경고+스킵.
    /// 포맷 변환(래스터화) 지원은 후속 슬라이스 — 그때까지 이 분기가
    /// "무음 손실 없는 미지원" 을 소유한다.
    KnownUnsupported(&'static str),
    /// 빈/절단/미지 magic — 확장자로 추측하지 않는다.
    Unknown,
}

/// 바이트 선두 magic 으로 이미지 포맷을 판별한다.
pub(crate) fn sniff_image_format(bytes: &[u8]) -> Sniffed {
    if bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Sniffed::Png;
    }
    if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        return Sniffed::Jpeg;
    }
    if bytes.len() >= 6 && (&bytes[..6] == b"GIF87a" || &bytes[..6] == b"GIF89a") {
        return Sniffed::Gif;
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Sniffed::Webp;
    }
    // BMP: "BM" + 파일 크기 필드가 붙는 14바이트 헤더.
    if bytes.len() >= 14 && &bytes[..2] == b"BM" {
        return Sniffed::KnownUnsupported("bmp");
    }
    // EMF: 헤더 레코드 타입 0x00000001 + 오프셋 40 시그니처 " EMF".
    if bytes.len() >= 44 && bytes[..4] == [0x01, 0, 0, 0] && &bytes[40..44] == b" EMF" {
        return Sniffed::KnownUnsupported("emf");
    }
    // WMF placeable: 0x9AC6CDD7 (LE). standard WMF(01 00 09 00)는 magic 이
    // 약해 오탐 위험이 커서 placeable 만 판별한다 — 나머지는 Unknown.
    if bytes.len() >= 4 && bytes[..4] == [0xD7, 0xCD, 0xC6, 0x9A] {
        return Sniffed::KnownUnsupported("wmf");
    }
    Sniffed::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0];

    #[test]
    fn supported_magics() {
        assert_eq!(sniff_image_format(PNG), Sniffed::Png);
        assert_eq!(sniff_image_format(JPEG), Sniffed::Jpeg);
        assert_eq!(sniff_image_format(b"GIF87a\x00"), Sniffed::Gif);
        assert_eq!(sniff_image_format(b"GIF89a\x00"), Sniffed::Gif);
        let mut webp = Vec::from(&b"RIFF"[..]);
        webp.extend_from_slice(&[0x10, 0, 0, 0]);
        webp.extend_from_slice(b"WEBPVP8 ");
        assert_eq!(sniff_image_format(&webp), Sniffed::Webp);
    }

    #[test]
    fn known_unsupported_magics() {
        let mut bmp = Vec::from(&b"BM"[..]);
        bmp.extend_from_slice(&[0u8; 12]);
        assert_eq!(sniff_image_format(&bmp), Sniffed::KnownUnsupported("bmp"));

        let mut emf = vec![0x01, 0, 0, 0];
        emf.extend_from_slice(&[0u8; 36]);
        emf.extend_from_slice(b" EMF");
        assert_eq!(sniff_image_format(&emf), Sniffed::KnownUnsupported("emf"));

        let wmf = [0xD7, 0xCD, 0xC6, 0x9A, 0, 0];
        assert_eq!(sniff_image_format(&wmf), Sniffed::KnownUnsupported("wmf"));
    }

    #[test]
    fn empty_and_truncated_prefixes_are_unknown() {
        assert_eq!(sniff_image_format(&[]), Sniffed::Unknown);
        // 각 지원 포맷의 모든 절단 prefix = Unknown (판별 최소 길이 미달).
        for full in [PNG, JPEG, &b"GIF89a"[..], &[0xD7, 0xCD, 0xC6, 0x9A][..]] {
            let min = full.len().min(3);
            for cut in 1..min {
                assert_eq!(
                    sniff_image_format(&full[..cut]),
                    Sniffed::Unknown,
                    "truncated {cut} of {full:?}"
                );
            }
        }
        // BMP 는 2바이트 magic 이지만 14바이트 헤더 미달이면 Unknown.
        assert_eq!(sniff_image_format(b"BM"), Sniffed::Unknown);
    }

    #[test]
    fn unknown_magic_never_guesses_from_content() {
        assert_eq!(sniff_image_format(b"<svg xmlns="), Sniffed::Unknown);
        assert_eq!(sniff_image_format(&[0u8; 64]), Sniffed::Unknown);
        // RIFF 인데 WEBP 아님 (WAV 등) = Unknown.
        let mut wav = Vec::from(&b"RIFF"[..]);
        wav.extend_from_slice(&[0x10, 0, 0, 0]);
        wav.extend_from_slice(b"WAVEfmt ");
        assert_eq!(sniff_image_format(&wav), Sniffed::Unknown);
    }
}
