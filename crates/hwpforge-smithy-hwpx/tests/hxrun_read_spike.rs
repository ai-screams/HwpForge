//! W0b 스파이크 (이미지/글상자 에픽 게이트 2 재설계): quick-xml 0.41 의
//! `@attr + $value` 혼합 자식 **순서 보존** 역직렬화 지원을 잠근다 —
//! W1a `HxRunRead`(decode 전용 ordered DTO) 전환의 전제 조건.
//!
//! 검증 항목 (에픽 계획 §1b W0b):
//! 1. `<run charPrIDRef>` 속성과 `$value` enum 병용 (in-tree 선례 없음)
//! 2. `<t>a</t><pic/><t>b</t>` 의 문서 순서가 Vec 순서로 보존되는지
//! 3. 미지 자식 → `#[serde(other)]` 폴백 (파싱 실패 금지)
//!
//! 직렬화는 검증하지 않는다 — Codex 리뷰 #2 채택: 인코더는 1 Core Run →
//! 1 HxRun 정규형을 유지하므로 read/write DTO 분리로 mixed serializer 불요.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RunReadSpike {
    #[serde(rename = "@charPrIDRef", default)]
    char_pr_id_ref: u32,
    #[serde(rename = "$value", default)]
    children: Vec<ChildSpike>,
}

#[derive(Debug, Deserialize)]
enum ChildSpike {
    #[serde(rename = "t")]
    Text(String),
    #[serde(rename = "pic")]
    Pic(PicSpike),
    #[serde(rename = "ctrl")]
    Ctrl(CtrlSpike),
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct PicSpike {
    #[serde(rename = "@id", default)]
    id: String,
}

#[derive(Debug, Default, Deserialize)]
struct CtrlSpike {}

#[test]
fn attr_plus_value_enum_preserves_document_order() {
    // 핵심 케이스: 현행 HxRun(종류별 Vec)이 잃어버리는 인터리브 순서.
    let xml = r#"<run charPrIDRef="7"><t>a</t><pic id="p1"/><t>b</t></run>"#;
    let run: RunReadSpike = quick_xml::de::from_str(xml).expect("deserialize");
    assert_eq!(run.char_pr_id_ref, 7, "@attr 병용");
    assert_eq!(run.children.len(), 3);
    assert!(matches!(&run.children[0], ChildSpike::Text(t) if t == "a"));
    assert!(matches!(&run.children[1], ChildSpike::Pic(p) if p.id == "p1"));
    assert!(matches!(&run.children[2], ChildSpike::Text(t) if t == "b"));
}

#[test]
fn unknown_children_fall_back_without_error() {
    // 미지 요소(미래 스키마·미지원 종류)는 Other 로 흡수 — 순서 자리는 유지.
    let xml =
        r#"<run charPrIDRef="1"><t>x</t><futureElem attr="1"><inner/></futureElem><t>y</t></run>"#;
    let run: RunReadSpike = quick_xml::de::from_str(xml).expect("deserialize");
    assert_eq!(run.children.len(), 3);
    assert!(matches!(&run.children[1], ChildSpike::Other));
    assert!(matches!(&run.children[2], ChildSpike::Text(t) if t == "y"));
}

#[test]
fn nested_element_content_is_consumed_within_its_slot() {
    // ctrl 처럼 내부 자식이 있는 요소도 자기 슬롯에서 소비되고 이후 순서가
    // 이어져야 한다 (스텁 struct 가 내부를 무시해도 커서가 안 깨질 것).
    let xml =
        r#"<run><ctrl><newNum num="7" numType="PAGE"/></ctrl><t>본문</t><pic id="p2"/></run>"#;
    let run: RunReadSpike = quick_xml::de::from_str(xml).expect("deserialize");
    assert_eq!(run.children.len(), 3);
    assert!(matches!(&run.children[0], ChildSpike::Ctrl(_)));
    assert!(matches!(&run.children[1], ChildSpike::Text(t) if t == "본문"));
    assert!(matches!(&run.children[2], ChildSpike::Pic(p) if p.id == "p2"));
}
