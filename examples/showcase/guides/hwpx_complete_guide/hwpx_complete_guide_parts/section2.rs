use super::shared::{
    csi, empty, p, runs_p, CS_BLUE, CS_GRAY, CS_GREEN_ITALIC, CS_HEADING, CS_NORMAL, CS_RED_BOLD,
    CS_SMALL, CS_TITLE, PS_BODY, PS_CENTER, PS_DISTRIBUTE, PS_LEFT, PS_RIGHT,
};
use hwpforge_core::control::{Control, DutmalAlign, DutmalPosition};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{LineNumberShape, Section, Visibility};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{FieldType, HwpUnit, RefContentType, RefType, ShowMode};

pub(crate) fn section2_text_formatting() -> Section {
    let mut paras: Vec<Paragraph> = vec![
        // ── 제목 ──
        p("텍스트 서식 시스템", CS_TITLE, PS_CENTER),
        empty(),
        // ── 정렬 데모 ──
        p("1. 문단 정렬 (Paragraph Alignment)", CS_HEADING, PS_LEFT),
        empty(),
        p(
            "양쪽 정렬(Justify): 본문에서 가장 일반적으로 사용되는 정렬입니다. 양쪽 여백에 맞춰 글자 간격이 자동 조절됩니다.",
            CS_NORMAL,
            PS_BODY,
        ),
        p(
            "가운데 정렬(Center): 제목이나 캡션에 주로 사용합니다.",
            CS_NORMAL,
            PS_CENTER,
        ),
        p(
            "왼쪽 정렬(Left): 코드나 목록에 적합합니다.",
            CS_NORMAL,
            PS_LEFT,
        ),
        p(
            "오른쪽 정렬(Right): 날짜, 서명 등에 사용합니다.",
            CS_NORMAL,
            PS_RIGHT,
        ),
        p(
            "배분 정렬(Distribute): 글자를 균등하게 분배합니다.",
            CS_NORMAL,
            PS_DISTRIBUTE,
        ),
        empty(),
        // ── 덧말(Dutmal) 데모 ──
        p("2. 덧말 (Dutmal / Ruby Text)", CS_HEADING, PS_LEFT),
        empty(),
        p(
            "덧말은 한자 위나 아래에 한글 읽기를 표시하는 기능입니다:",
            CS_NORMAL,
            PS_BODY,
        ),
        empty(),
    ];

    // 위쪽 덧말
    let dutmal_top = Control::dutmal("大韓民國", "대한민국");
    // 아래쪽 덧말
    let mut dutmal_bottom = Control::dutmal("漢字", "한자");
    if let Control::Dutmal { ref mut position, .. } = dutmal_bottom {
        *position = DutmalPosition::Bottom;
    }
    // 오른쪽 덧말 + 왼쪽정렬
    let mut dutmal_right = Control::dutmal("情報", "정보");
    if let Control::Dutmal { ref mut position, ref mut align, .. } = dutmal_right {
        *position = DutmalPosition::Right;
        *align = DutmalAlign::Left;
    }

    paras.push(runs_p(
        vec![
            Run::text("위쪽 덧말: ", csi(CS_NORMAL)),
            Run::control(dutmal_top, csi(CS_NORMAL)),
            Run::text("    아래쪽 덧말: ", csi(CS_NORMAL)),
            Run::control(dutmal_bottom, csi(CS_NORMAL)),
            Run::text("    오른쪽 덧말: ", csi(CS_NORMAL)),
            Run::control(dutmal_right, csi(CS_NORMAL)),
        ],
        PS_CENTER,
    ));
    paras.push(empty());

    // ── 글자겹침(Compose) ──
    paras.push(p("3. 글자겹침 (Compose)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("글자겹침 기능: ", csi(CS_NORMAL)),
            Run::control(Control::compose("12"), csi(CS_NORMAL)),
            Run::text("  (숫자 1과 2를 겹침)", csi(CS_SMALL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 필드(Field) 데모 ──
    paras.push(p("4. 필드 (Field)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    // ClickHere (누름틀)
    paras.push(runs_p(
        vec![
            Run::text("누름틀(ClickHere): ", csi(CS_NORMAL)),
            Run::control(Control::field("이름을 입력하세요"), csi(CS_BLUE)),
        ],
        PS_BODY,
    ));

    // Date 필드
    paras.push(runs_p(
        vec![
            Run::text("날짜 필드(Date): ", csi(CS_NORMAL)),
            Run::control(
                Control::Field {
                    field_type: FieldType::Date,
                    hint_text: Some("날짜".to_string()),
                    help_text: Some("문서 작성 날짜를 표시합니다.".to_string()),
                },
                csi(CS_BLUE),
            ),
        ],
        PS_BODY,
    ));

    // PageNum 필드
    paras.push(runs_p(
        vec![
            Run::text("쪽 번호 필드(autoNum): 현재 ", csi(CS_NORMAL)),
            Run::control(
                Control::Field { field_type: FieldType::PageNum, hint_text: None, help_text: None },
                csi(CS_BLUE),
            ),
            Run::text("쪽", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 미주(Endnote) 데모 ──
    paras.push(p("5. 미주 (Endnote)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text(
                "글자 모양(charShape)은 폰트, 크기, 색상, 굵기, 기울임, 밑줄, 취소선 등을 정의합니다",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::endnote(vec![p(
                    "charShape 속성 목록: height(크기), textColor(색상), bold(굵기), italic(기울임), underlineType(밑줄), strikeoutShape(취소선), emphasis(강조점), ratio(장평), spacing(자간), relSz(상대크기), offset(세로위치), useKerning(커닝), useFontSpace(폰트 자간).",
                    CS_SMALL,
                    PS_BODY,
                )]),
                csi(CS_NORMAL),
            ),
            Run::text(".", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 메모(Memo) ──
    paras.push(p("6. 메모 (Memo)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("이 문단에는 검토 메모가 첨부되어 있습니다.", csi(CS_NORMAL)),
            Run::control(
                Control::memo(
                    vec![
                        p("검토 의견:", CS_RED_BOLD, PS_LEFT),
                        p("charShape 설명을 표 형태로 정리하면 더 좋겠습니다.", CS_NORMAL, PS_LEFT),
                        p("다음 버전에 반영 부탁드립니다.", CS_NORMAL, PS_LEFT),
                    ],
                    "김검토",
                    "2026-03-06",
                ),
                csi(CS_NORMAL),
            ),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 상호참조(CrossRef) ──
    paras.push(p("7. 상호참조 (CrossRef)", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("HWPX 문서 정의는 섹션 1의 ", csi(CS_NORMAL)),
            Run::control(
                Control::cross_ref("HWPX정의", RefType::Bookmark, RefContentType::Page),
                csi(CS_BLUE),
            ),
            Run::text("쪽을 참조하세요. ZIP 파일 구조는 ", csi(CS_NORMAL)),
            Run::control(
                Control::CrossRef {
                    target_name: "헤더구조".to_string(),
                    ref_type: RefType::Bookmark,
                    content_type: RefContentType::Page,
                    as_hyperlink: true,
                },
                csi(CS_BLUE),
            ),
            Run::text("쪽에 설명되어 있습니다.", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 정렬별 글자 스타일 시연 ──
    paras.push(p("8. 글자 서식 변화 시연", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("기본 ", csi(CS_NORMAL)),
            Run::text("굵게 ", csi(CS_RED_BOLD)),
            Run::text("파랑 ", csi(CS_BLUE)),
            Run::text("기울임 녹색 ", csi(CS_GREEN_ITALIC)),
            Run::text("작은 글씨 ", csi(CS_SMALL)),
            Run::text("제목 크기 ", csi(CS_TITLE)),
            Run::text("회색 워터마크", csi(CS_GRAY)),
        ],
        PS_BODY,
    ));

    // ── 섹션 설정: Visibility + 줄번호 ──
    let vis = Visibility {
        hide_first_header: true,
        hide_first_footer: false,
        hide_first_master_page: false,
        hide_first_page_num: false,
        hide_first_empty_line: false,
        show_line_number: true,
        border: ShowMode::ShowAll,
        fill: ShowMode::ShowAll,
    };
    let lns = LineNumberShape {
        restart_type: 2, // per section
        count_by: 5,
        distance: HwpUnit::new(850).unwrap(),
        start_number: 1,
    };

    let mut section = Section::with_paragraphs(paras, PageSettings::a4());
    section.visibility = Some(vis);
    section.line_number_shape = Some(lns);

    section
}
