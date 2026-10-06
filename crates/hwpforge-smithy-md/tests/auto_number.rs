//! Caption numbers in Markdown (§19.16 J, P19): the lossy and styled views
//! write the number Hancom cached between its prefix and suffix; the
//! lossless grammar has no place for it and refuses the document rather
//! than dropping the number.

use hwpforge_core::control::{AutoNumberFormat, AutoNumberKind, Control};
use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section, StyleLookup, Validated};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_md::{MdEncoder, MdError};

struct PlainStyles;

impl StyleLookup for PlainStyles {}

fn caption(format: AutoNumberFormat) -> Document<Validated> {
    let number = Control::AutoNumber { kind: AutoNumberKind::Picture, num: 3, format };
    let para = Paragraph::with_runs(
        vec![
            Run::text("그림 ", CharShapeIndex::new(0)),
            Run::control(number, CharShapeIndex::new(0)),
            Run::text(" 조직도", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    doc.validate().expect("validate")
}

/// "그림 3 조직도" in both readable views; "(3)" with a prefix and suffix;
/// the lossless encoder refuses.
///
/// 이것을 실패시키는 것: lossy·styled 의 `AutoNumber` 갈래를 지우는 것(번호가
/// 사라짐) / 접두·접미를 버리는 것 / lossless 가 번호를 조용히 버리는 것.
#[test]
fn caption_number_is_written_or_refused() {
    let plain = caption(AutoNumberFormat::default());
    assert!(MdEncoder::encode_lossy(&plain).unwrap().contains("그림 3 조직도"));
    assert!(MdEncoder::encode_styled(&plain, &PlainStyles).markdown.contains("그림 3 조직도"));
    let wrapped = caption(AutoNumberFormat {
        prefix: Some('('),
        suffix: Some(')'),
        ..AutoNumberFormat::default()
    });
    assert!(MdEncoder::encode_lossy(&wrapped).unwrap().contains("그림 (3) 조직도"));
    assert!(MdEncoder::encode_styled(&wrapped, &PlainStyles).markdown.contains("그림 (3) 조직도"));
    assert!(matches!(
        MdEncoder::encode_lossless(&plain),
        Err(MdError::UnsupportedStructure { .. })
    ));
}
