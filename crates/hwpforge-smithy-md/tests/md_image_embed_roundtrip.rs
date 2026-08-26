//! md→hwpx 이미지 임베드 왕복 게이트 (W6 §12b).
//!
//! CLI/MCP convert 가 쓰는 것과 동일한 파이프라인(디코드 → embed 로더 →
//! 레지스트리 브리지 → 인코드)으로 HWPX 를 만들고, **우리 디코더로 다시
//! 열어** BinData 가 실제로 포장됐는지(바이트 일치·합성 키·manifest 정합)
//! 를 실증한다 — "BinData 미포장" 실갭(2026-08-13 실측)의 회귀 게이트.

use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxRegistryBridge};
use hwpforge_smithy_md::{load_referenced_images, MdDecoder};

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 9, 8, 7, 6];

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let p =
            std::env::temp_dir().join(format!("hwpforge-embed-e2e-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("mkdir");
        Self(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn md_image_embeds_into_bindata_and_roundtrips() {
    let dir = TempDir::new("roundtrip");
    std::fs::write(dir.0.join("photo.png"), PNG).expect("write png");
    let markdown = "# 제목\n\n본문 앞 ![사진](photo.png) 뒤.\n";

    // CLI/MCP convert 와 동일 파이프라인.
    let mut md_doc = MdDecoder::decode_with_default(markdown).expect("decode");
    let embedded = load_referenced_images(&mut md_doc.document, Some(&dir.0));
    assert!(embedded.warnings.is_empty(), "{:?}", embedded.warnings);

    let bridge = HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge");
    let rebound = bridge.rebind_draft_document(md_doc.document).expect("rebind");
    let validated = rebound.validate().expect("validate");
    let bytes =
        HwpxEncoder::encode(&validated, bridge.style_store(), &embedded.store).expect("encode");

    // 왕복: 우리 디코더로 BinData 실증.
    let decoded = HwpxDecoder::decode(&bytes).expect("hwpx decode");
    assert_eq!(
        decoded.image_store.get("image1.png"),
        Some(PNG),
        "BinData 바이트가 원본과 일치해야 한다"
    );
    let mut image_paths = Vec::new();
    for section in decoded.document.sections() {
        for para in &section.paragraphs {
            for run in &para.runs {
                if let RunContent::Image(img) = &run.content {
                    image_paths.push(img.path.clone());
                }
            }
        }
    }
    // 디코더는 참조를 binaryItemIDRef 유래 canonical 형태(`BinData/<stem>`)로
    // 복원한다 — 렌더 쪽 resolver 가 store 키(`image1.png`)와 결정적으로
    // 잇는 기존 계약 (W2a). 여기선 stem 이 합성 키에서 왔음을 잠근다.
    assert_eq!(image_paths, vec!["BinData/image1"], "합성 키 stem 이 참조로 왕복");

    // 패키지 실물: BinData/ 엔트리 + manifest 등재 (H2 — id 정합).
    let reader = std::io::Cursor::new(&bytes);
    let mut zip = zip::ZipArchive::new(reader).expect("zip");
    let names: Vec<String> =
        (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect();
    assert!(names.iter().any(|n| n == "BinData/image1.png"), "BinData 엔트리: {names:?}");
    let mut content_hpf = String::new();
    {
        use std::io::Read as _;
        zip.by_name("Contents/content.hpf")
            .expect("content.hpf")
            .read_to_string(&mut content_hpf)
            .expect("read hpf");
    }
    assert!(
        content_hpf.contains(r#"id="image1" href="BinData/image1.png""#),
        "manifest 등재: {content_hpf}"
    );
}

#[test]
fn failed_reference_produces_valid_document_without_dangling_ref() {
    // 부재 파일 참조 = run 드롭 + 경고 — 산출 HWPX 에 dangling
    // binaryItemIDRef 가 없어야 한다.
    let dir = TempDir::new("dangling");
    let markdown = "본문 ![없음](missing.png) 계속.\n";
    let mut md_doc = MdDecoder::decode_with_default(markdown).expect("decode");
    let embedded = load_referenced_images(&mut md_doc.document, Some(&dir.0));
    assert_eq!(embedded.warnings.len(), 1);

    let bridge = HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge");
    let rebound = bridge.rebind_draft_document(md_doc.document).expect("rebind");
    let validated = rebound.validate().expect("validate");
    let bytes =
        HwpxEncoder::encode(&validated, bridge.style_store(), &embedded.store).expect("encode");
    let decoded = HwpxDecoder::decode(&bytes).expect("hwpx decode");
    assert_eq!(decoded.image_store.iter().count(), 0);
    let section_xml_has_pic = {
        let reader = std::io::Cursor::new(&bytes);
        let mut zip = zip::ZipArchive::new(reader).expect("zip");
        use std::io::Read as _;
        let mut s = String::new();
        zip.by_name("Contents/section0.xml")
            .expect("section")
            .read_to_string(&mut s)
            .expect("read");
        s.contains("<hp:pic")
    };
    assert!(!section_xml_has_pic, "드롭된 이미지는 pic 요소를 남기지 않는다");
}
