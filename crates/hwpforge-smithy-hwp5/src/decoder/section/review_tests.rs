// The same wire controls must mean the same thing in every host.
const R1_GROUP: u32 = u32::MAX;
const R1_NESTED_GROUP: u32 = u32::MAX - 1;
const R1_CAPTION: u32 = u32::MAX - 2;
const R1_ELLIPSE: u32 = u32::MAX - 3;
const R1_POLYGON: u32 = u32::MAX - 4;

fn r1_shift(stream: &mut Vec<u8>, bytes: &[u8], delta: u16) {
    for record in Record::parse_stream(&mut Cursor::new(bytes)).unwrap() {
        stream.extend(make_record(
            TagId::from(record.header.tag_id),
            record.header.level + delta,
            &record.data,
        ));
    }
}

fn r1_paragraph(text: &str) -> Vec<u8> {
    let mut bytes = make_record(TagId::ParaHeader, 0, &para_header_data(0, 0));
    bytes.extend(make_record(TagId::ParaText, 1, &para_text_data(text)));
    bytes
}

fn r1_wrap(kind: u32, inner: &[u8]) -> Vec<u8> {
    let paragraph_count = Record::parse_stream(&mut Cursor::new(inner))
        .unwrap()
        .iter()
        .filter(|r| r.header.level == 0 && TagId::from(r.header.tag_id) == TagId::ParaHeader)
        .count() as u32;
    let mut bytes = r1_paragraph("outer");
    let base = match kind {
        R1_CAPTION => {
            bytes.extend(make_record(TagId::CtrlHeader, 1, &CTRL_ID_TABLE.to_le_bytes()));
            bytes.extend(make_record(
                TagId::ListHeader,
                2,
                &caption_list_header(paragraph_count as u16, 3, 0, 0, 0),
            ));
            r1_shift(&mut bytes, inner, 2);
            bytes.extend(make_record(TagId::Table, 2, &basic_table_data(1, 1)));
            return bytes;
        }
        CTRL_ID_TABLE => {
            let mut table = Vec::new();
            one_cell_table(&mut table, paragraph_count);
            r1_shift(&mut bytes, &table, 1);
            2
        }
        CTRL_ID_GSO | R1_GROUP | R1_NESTED_GROUP | R1_ELLIPSE | R1_POLYGON => {
            bytes.extend(make_record(
                TagId::CtrlHeader,
                1,
                &gso_ctrl_header_data(0, 0, 9000, 3000),
            ));
            let group = matches!(kind, R1_GROUP | R1_NESTED_GROUP);
            let shape_tag = match kind {
                R1_ELLIPSE => b"lle$",
                R1_POLYGON => b"lop$",
                _ => b"cer$",
            };
            bytes.extend(make_record(
                TagId::ShapeComponent,
                2,
                &shape_component_render_data(
                    if group { &SHAPE_COMPONENT_TYPE_GROUP } else { shape_tag },
                    true,
                    0,
                    (9000, 3000),
                    0.0,
                ),
            ));
            let mut component = 2;
            if kind == R1_NESTED_GROUP {
                component += 1;
                bytes.extend(make_record(
                    TagId::ShapeComponent,
                    component,
                    &shape_component_render_data(
                        &SHAPE_COMPONENT_TYPE_GROUP,
                        false,
                        1,
                        (9000, 3000),
                        0.0,
                    ),
                ));
            }
            if group {
                component += 1;
                bytes.extend(make_record(
                    TagId::ShapeComponent,
                    component,
                    &shape_component_render_data(b"cer$", false, 1, (9000, 3000), 0.0),
                ));
            }
            match kind {
                R1_ELLIPSE => {
                    bytes.extend(make_record(TagId::ShapeComponentEllipse, component + 1, &[0; 60]))
                }
                R1_POLYGON => bytes.extend(make_record(
                    TagId::ShapeComponentPolygon,
                    component + 1,
                    &shape_component_polygon_data(&[(0, 0), (100, 0), (50, 80)]),
                )),
                _ => bytes.extend(make_record(TagId::ShapeComponentRect, component + 1, &[0; 33])),
            }
            bytes.extend(make_record(
                TagId::ListHeader,
                component + 1,
                &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ));
            component + 1
        }
        _ => {
            bytes.extend(make_record(TagId::CtrlHeader, 1, &kind.to_le_bytes()));
            bytes.extend(make_record(TagId::ListHeader, 2, &1u32.to_le_bytes()));
            2
        }
    };
    r1_shift(&mut bytes, inner, base);
    bytes
}

fn r1_paths() -> Vec<Vec<u32>> {
    vec![
        vec![],
        vec![CTRL_ID_TABLE],
        vec![CTRL_ID_HEADER],
        vec![CTRL_ID_FOOTER],
        vec![CTRL_ID_FOOTNOTE],
        vec![CTRL_ID_ENDNOTE],
        vec![CTRL_ID_GSO],
        vec![R1_GROUP],
        vec![R1_NESTED_GROUP],
        vec![R1_CAPTION],
        vec![R1_ELLIPSE],
        vec![R1_POLYGON],
        vec![CTRL_ID_TABLE, CTRL_ID_FOOTER],
        vec![R1_GROUP, CTRL_ID_GSO, CTRL_ID_TABLE],
        vec![CTRL_ID_GSO, CTRL_ID_TABLE, CTRL_ID_FOOTNOTE],
    ]
}

fn r1_at_path<'a>(mut paragraphs: &'a [Hwp5Paragraph], path: &[u32]) -> &'a [Hwp5Paragraph] {
    for kind in path {
        assert_eq!(paragraphs.len(), 1, "host paragraphs must not absorb nested bodies");
        paragraphs = match &paragraphs[0].controls[0] {
            Hwp5Control::Table(t) if *kind == R1_CAPTION => &t.caption.as_ref().unwrap().paragraphs,
            Hwp5Control::Table(t) => &t.cells[0].paragraphs,
            Hwp5Control::Ellipse(t) => &t.paragraphs,
            Hwp5Control::Polygon(t) => &t.paragraphs,
            Hwp5Control::Header(t)
            | Hwp5Control::Footer(t)
            | Hwp5Control::Footnote(t)
            | Hwp5Control::Endnote(t) => &t.paragraphs,
            Hwp5Control::TextBox(t) => &t.paragraphs,
            Hwp5Control::Memo(t) => &t.paragraphs,
            Hwp5Control::Group(g) if *kind == R1_NESTED_GROUP => {
                let Hwp5Control::Group(inner) = &g.children[0].control else {
                    panic!("nested group")
                };
                &inner.children[0].paragraphs
            }
            Hwp5Control::Group(g) => &g.children[0].paragraphs,
            other => panic!("unexpected host {kind:x}: {other:?}"),
        };
    }
    paragraphs
}

fn r1_in_path(payload: &[u8], path: &[u32]) -> Vec<u8> {
    path.iter().rev().fold(payload.to_vec(), |inner, &kind| r1_wrap(kind, &inner))
}

fn memo_control() -> Vec<u8> {
    r4_command(CTRL_ID_MEMO, "MEMO/65535/7/1/2/test/;")
}

fn equation_control() -> Vec<u8> {
    let mut wire = make_record(TagId::CtrlHeader, 1, &CTRL_ID_EQED.to_le_bytes());
    let script = "x + y";
    let mut data = 0u32.to_le_bytes().to_vec();
    data.extend((script.encode_utf16().count() as u16).to_le_bytes());
    data.extend(para_text_data(script));
    data.extend([0; 10]);
    wire.extend(make_record(TagId::EqEdit, 2, &data));
    wire
}

/// 이것을 실패시키는 것: 중첩 해석기에서 eqed/tbl/fn/en/head/foot/gso 중 하나를
/// Unknown으로 돌리거나, 자식 레코드를 부모 문단으로 보내기(각 경로별 변이).
#[test]
// 이것을 실패시키는 것: 수식 제어를 Unknown으로 교체 (실제 음성 대조 실행).
fn r1_subtree_controls_use_the_body_interpreter_in_every_host() {
    let mut payloads = Vec::new();
    let mut equation = r1_paragraph("equation host");
    equation.extend(equation_control());
    payloads.push(equation);
    for kind in [
        CTRL_ID_TABLE,
        CTRL_ID_HEADER,
        CTRL_ID_FOOTER,
        CTRL_ID_FOOTNOTE,
        CTRL_ID_ENDNOTE,
        CTRL_ID_GSO,
        R1_GROUP,
    ] {
        payloads.push(r1_wrap(kind, &r1_paragraph("inner body")));
    }
    for payload in payloads {
        let body = parse_body_text(&payload, &version()).unwrap();
        assert_eq!(body.paragraphs[0].controls.len(), 1);
        assert!(!matches!(body.paragraphs[0].controls[0], Hwp5Control::Unknown { .. }));
        let expected = r3_semantic_snapshot(&body.paragraphs);
        for path in r1_paths() {
            let nested = parse_body_text(&r1_in_path(&payload, &path), &version()).unwrap();
            assert_eq!(
                r3_semantic_snapshot(&r1_at_path(&nested.paragraphs, &path)),
                expected,
                "path {path:x?}"
            );
        }
    }
}

fn r1_clickhere() -> Vec<u8> {
    r1_clickhere_with_direction("힌")
}

fn r1_clickhere_with_direction(direction: &str) -> Vec<u8> {
    let command = format!(
        "Clickhere:set:0:Direction:wstring:{}:{direction} HelpState:wstring:0:  ",
        direction.encode_utf16().count()
    );
    let mut data = CTRL_ID_CLICK_HERE.to_le_bytes().to_vec();
    data.extend([0; 4]);
    data.push(9);
    let units: Vec<_> = command.encode_utf16().collect();
    data.extend_from_slice(&(units.len() as u16).to_le_bytes());
    data.extend(units.into_iter().flat_map(|u| u.to_le_bytes()));
    data.extend([0; 8]);
    data
}

/// 이것을 실패시키는 것: %clk 이름 깊이를 고정 2로 / 현재 문단 깊이로 판정 /
/// 열린 제어가 아닌 다음 문단 또는 잘못된 깊이의 CtrlData를 이름으로 사용하기.
#[test]
fn r1_clickhere_name_tracks_the_control_depth_and_owner() {
    let mut name = vec![0x1B, 0x02, 0x01, 0, 0, 0, 0, 0x40, 0x01, 0, 1, 0];
    name.extend_from_slice(&('명' as u16).to_le_bytes());
    for path in r1_paths() {
        for name_level in [0, 2, 3] {
            let mut payload = r1_paragraph("owner");
            payload.extend(make_record(TagId::CtrlHeader, 1, &r1_clickhere()));
            if name_level > 0 {
                payload.extend(make_record(TagId::CtrlData, name_level, &name));
            }
            payload.extend(r1_paragraph("next"));
            payload.extend(make_record(TagId::CtrlData, 2, &name));
            let nested = parse_body_text(&r1_in_path(&payload, &path), &version()).unwrap();
            let paragraphs = r1_at_path(&nested.paragraphs, &path);
            assert_eq!(paragraphs.len(), 2);
            let Hwp5Control::ClickHere(control) = &paragraphs[0].controls[0] else {
                panic!("clickhere")
            };
            assert_eq!(
                control.name.as_deref(),
                (name_level == 2).then_some("명"),
                "path {path:x?}"
            );
            assert!(paragraphs[1].controls.is_empty());
        }
    }
}

/// 이것을 실패시키는 것: 자식 finish에서 MEMO를 조기 확정 / Group 자식 문단이나
/// 중첩 Group control 순회를 생략 / 실제 본문 대신 빈 메모끼리 비교하기.
#[test]
// 이것을 실패시키는 것: 메모의 중첩 각주 글자 누락 (실제 음성 대조 실행).
fn r1_group_memo_bodies_join_only_after_the_section_is_complete() {
    let memo_wire = memo_control();
    let mut payload = r1_paragraph("anchor");
    payload.extend(memo_wire);
    let parsed = parse_body_text(&payload, &version()).unwrap();
    let Hwp5Control::Memo(memo) = &parsed.paragraphs[0].controls[0] else { panic!("memo") };
    let id = memo.command.memo_id;
    for path in r1_paths() {
        let mut bytes = r1_in_path(&payload, &path);
        bytes.extend(make_record(TagId::MemoList, 1, &id.to_le_bytes()));
        bytes.extend(make_record(TagId::ListHeader, 1, &1u32.to_le_bytes()));
        r1_shift(&mut bytes, &r1_wrap(CTRL_ID_FOOTNOTE, &r1_paragraph("memo note body")), 1);
        let result = parse_body_text(&bytes, &version()).unwrap();
        let Hwp5Control::Memo(memo) = &r1_at_path(&result.paragraphs, &path)[0].controls[0] else {
            panic!("memo")
        };
        assert_eq!(memo.paragraphs.len(), 1);
        assert_eq!(memo.paragraphs[0].text, "outer");
        let Hwp5Control::Footnote(note) = &memo.paragraphs[0].controls[0] else {
            panic!("memo footnote")
        };
        assert_eq!(note.paragraphs[0].text, "memo note body");
        assert!(
            !result.warnings.iter().any(|w| matches!(
                w,
                Hwp5Warning::DroppedControl { control: "memo" | "memo_content_cluster", .. }
            )),
            "{path:x?}: {:?}",
            result.warnings
        );
    }
}

/// 이것을 실패시키는 것: 묶음 텍스트 진입 때 예산 초기화 / 전체 깊이 조건 삭제 /
/// 같은 종류만 세기 / 레코드 복제 / record마다 이전 prefix를 재처리하기.
#[test]
fn r1_cross_container_depth_and_record_work_are_bounded() {
    let run = |depth: usize, count: usize, payload_size: usize| {
        let kinds = [R1_GROUP, CTRL_ID_GSO, CTRL_ID_TABLE, CTRL_ID_FOOTNOTE, CTRL_ID_HEADER];
        let path: Vec<_> = (0..depth).map(|i| kinds[i % kinds.len()]).collect();
        let bytes = r1_in_path(&r1_paragraph("deep leaf"), &path);
        let mut records = Record::parse_stream(&mut Cursor::new(&bytes)).unwrap();
        let deep_level = records.last().unwrap().header.level;
        records.push(Record {
            header: crate::schema::record::RecordHeader {
                tag_id: 0x3ff,
                level: deep_level,
                size: payload_size as u32,
            },
            data: vec![0x41; payload_size],
        });
        let mut parser = BodyTextParserState::default();
        let work = parser.work.clone();
        let identity = parser.record_identity.clone();
        for _ in 0..count {
            for record in &records {
                identity.set(std::ptr::from_ref(record));
                parser.handle_record(record);
            }
        }
        let result = parser.finish();
        assert_eq!(result.paragraphs.len(), count);
        let capped = result.warnings.iter().any(|w| matches!(w, Hwp5Warning::DroppedControl { reason, .. } if reason.contains("depth cap")));
        let cost = depth + path.iter().filter(|&&k| k == R1_GROUP).count();
        assert_eq!(capped, cost > MAX_CONTROL_NESTING);
        if cost <= MAX_CONTROL_NESTING {
            assert_eq!(r1_at_path(&result.paragraphs, &path)[0].text, "deep leaf");
        } else {
            assert!(!format!("{:?}", result.paragraphs).contains("deep leaf"));
        }
        assert!(work.get() <= records.len() * count * (2 * MAX_CONTROL_NESTING + 2));
        (work.get(), records.len() * count)
    };
    run(26, 1, 1); // 26 controls + 6 group children = 32
    run(27, 1, 1); // 33 cumulative containers
    let (one, n) = run(200, 1, 64 * 1024 * 1024);
    let (many, m) = run(200, 32, 1);
    assert_eq!(m, n * 32);
    assert_eq!(many, one * 32);
    eprintln!("r1 work: {n} records/{one} visits (64 MiB borrowed); {m} records/{many} visits; max container depth {MAX_CONTROL_NESTING}");
}

/// 이것을 실패시키는 것: gso CtrlHeader와 $con 자식 사이에서 그룹 예산 초기화 /
/// 그룹 경계의 >, >= 혼동 / 그룹 자식의 전역 예산 차감을 생략하기.
#[test]
fn r1_group_components_and_text_share_the_cumulative_budget() {
    for groups in [16, 17] {
        let mut path = vec![R1_NESTED_GROUP; groups / 2];
        if groups % 2 == 1 {
            path.insert(0, R1_GROUP);
        }
        let result =
            parse_body_text(&r1_in_path(&r1_paragraph("group leaf"), &path), &version()).unwrap();
        let capped = result
            .warnings
            .iter()
            .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "gso_group", .. }));
        assert_eq!(capped, groups > 16);
        if groups == 16 {
            assert_eq!(r1_at_path(&result.paragraphs, &path)[0].text, "group leaf");
        }
    }
    let mut path = vec![R1_NESTED_GROUP; 8];
    path.push(R1_GROUP);
    let result =
        parse_body_text(&r1_in_path(&r1_paragraph("over group cap"), &path), &version()).unwrap();
    assert!(result.warnings.iter().any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "gso_group", reason } if reason == "shared group depth cap exceeded")));
    for total in [32, 33] {
        let mut path = vec![CTRL_ID_FOOTNOTE; total - 2];
        path.push(R1_GROUP); // group + its child text consume the last two slots
        let result =
            parse_body_text(&r1_in_path(&r1_paragraph("last child"), &path), &version()).unwrap();
        let capped = result
            .warnings
            .iter()
            .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "group_child_text", .. }));
        assert_eq!(capped, total > 32);
        if total == 32 {
            assert_eq!(r1_at_path(&result.paragraphs, &path)[0].text, "last child");
        }
    }
}

/// 이것을 실패시키는 것: 구역 끝 MEMO 연결에서 누적 깊이 예산을 초기화하거나
/// 제한을 지워 얕은 MEMO_LIST 200개가 무제한 재귀 트리를 만들게 하기.
#[test]
// 이것을 실패시키는 것: 메모 연결 사슬 축소 (실제 음성 대조 실행).
fn r1_memo_list_join_cannot_bypass_the_container_budget() {
    let mut bytes = r1_paragraph("anchor");
    bytes.extend(memo_control());
    let prototype = parse_body_text(&bytes, &version()).unwrap().paragraphs.remove(0);
    let memo_para = |id: u32| {
        let mut p = prototype.clone();
        let Hwp5Control::Memo(memo) = &mut p.controls[0] else { panic!("memo") };
        memo.command.memo_id = id;
        p
    };
    for path in [vec![], vec![R1_GROUP, CTRL_ID_TABLE]] {
        let mut parser = BodyTextParserState::default();
        let mut root = r1_paragraph("anchor");
        root.extend(memo_control());
        for record in Record::parse_stream(&mut Cursor::new(r1_in_path(&root, &path))).unwrap() {
            parser.handle_record(&record);
        }
        // Close the owning scopes before section-wide joining.
        parser.close_nested_control();
        parser.prepare_for_record(0);
        let id = match &prototype.controls[0] {
            Hwp5Control::Memo(m) => m.command.memo_id,
            _ => unreachable!(),
        };
        parser.memo_contents.insert(id, vec![memo_para(10_000)]);
        for i in 10_000..10_200 {
            parser.memo_contents.insert(i, vec![memo_para(i + 1)]);
        }
        let result = parser.finish();
        let mut paragraphs = r1_at_path(&result.paragraphs, &path);
        let mut filled = 0;
        while let Some(Hwp5Control::Memo(memo)) =
            paragraphs.first().and_then(|p| p.controls.first())
        {
            if memo.paragraphs.is_empty() {
                break;
            }
            filled += 1;
            paragraphs = &memo.paragraphs;
        }
        let host_depth = if path.is_empty() { 0 } else { 3 };
        assert_eq!(filled, MAX_CONTROL_NESTING - host_depth);
        assert!(result
            .warnings
            .iter()
            .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "memo_depth", .. })));
    }
}

fn r2_paths() -> Vec<Vec<u32>> {
    let mut paths = r1_paths();
    paths.push(vec![CTRL_ID_MEMO]);
    paths
}

fn r2_in_path(payload: &[u8], path: &[u32]) -> Vec<u8> {
    path.iter().rev().fold(payload.to_vec(), |inner, &kind| {
        if kind == CTRL_ID_MEMO {
            let mut wire = r1_paragraph("memo host");
            wire.extend(memo_control());
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let Hwp5Control::Memo(m) = &parsed.paragraphs[0].controls[0] else { panic!("memo") };
            wire.extend(make_record(TagId::MemoList, 1, &m.command.memo_id.to_le_bytes()));
            wire.extend(make_record(TagId::ListHeader, 1, &1u32.to_le_bytes()));
            r1_shift(&mut wire, &inner, 1);
            wire
        } else {
            let mut wire = r1_wrap(kind, &inner);
            if kind == R1_CAPTION {
                // A supported caption needs a real cell; the earlier parser-only
                // fixture deliberately ended before the table's cell list.
                let mut table = Vec::new();
                one_cell_table(&mut table, 1);
                let records = Record::parse_stream(&mut Cursor::new(table)).unwrap();
                let list = records.last().unwrap();
                wire.extend(make_record(TagId::ListHeader, 2, &list.data));
                r1_shift(&mut wire, &r1_paragraph("caption host cell"), 2);
            }
            wire
        }
    })
}

fn r2_xml(doc: hwpforge_core::Document<hwpforge_core::Draft>) -> String {
    let outcome = r3_encode(doc, &hwpforge_core::ImageStore::new());
    assert!(
        outcome.warnings.iter().all(|w| !w.is_semantic_loss()),
        "encode warnings: {:?}",
        outcome.warnings
    );
    hwpforge_smithy_hwpx::PackageReader::new(&outcome.bytes).unwrap().read_section_xml(0).unwrap()
}

/// 이것을 실패시키는 것: 메모 연결 한도에서 반환만 하고 이미 붙인 비메모 subtree 유지,
/// 연결 위치 예산 초기화, retain 결과 무시, 마지막 유효 깊이까지 잘라 버리기.
#[test]
// 이것을 실패시키는 것: 허용된 메모 leaf 글자 누락 (실제 음성 대조 실행).
fn r2_memo_join_trims_preparsed_container_bodies() {
    let mut anchor = r1_paragraph("memo anchor");
    anchor.extend(memo_control());
    let prototype = parse_body_text(&anchor, &version()).unwrap();
    let Hwp5Control::Memo(memo) = &prototype.paragraphs[0].controls[0] else { panic!("memo") };
    let id = memo.command.memo_id;
    for host_depth in [0, 1, 30, 31] {
        for body_depth in [0, 1, 2, 31] {
            let path = vec![CTRL_ID_FOOTNOTE; host_depth];
            let mut wire = r1_in_path(&anchor, &path);
            wire.extend(make_record(TagId::MemoList, 1, &id.to_le_bytes()));
            wire.extend(make_record(TagId::ListHeader, 1, &1u32.to_le_bytes()));
            let body = r1_in_path(&r1_paragraph("R2깊은본문"), &vec![CTRL_ID_FOOTNOTE; body_depth]);
            r1_shift(&mut wire, &body, 1);
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let Hwp5Control::Memo(memo) = &r1_at_path(&parsed.paragraphs, &path)[0].controls[0]
            else {
                panic!("memo")
            };
            let mut paragraphs = &memo.paragraphs;
            let mut actual = 0;
            while let Some(Hwp5Control::Footnote(note)) =
                paragraphs.first().and_then(|p| p.controls.first())
            {
                actual += 1;
                paragraphs = &note.paragraphs;
            }
            assert_eq!(actual, body_depth.min(32 - host_depth - 1));
            let capped = host_depth + 1 + body_depth > 32;
            assert_eq!(format!("{:?}", parsed.paragraphs).contains("R2깊은본문"), !capped);
            assert_eq!(
                parsed
                    .warnings
                    .iter()
                    .filter(|w| matches!(
                        w,
                        Hwp5Warning::DroppedControl { control: "memo_depth", .. }
                    ))
                    .count(),
                usize::from(capped)
            );
        }
    }
}

fn r2_package(section: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    let mut header = vec![0; 256];
    header[..18].copy_from_slice(b"HWP Document File\0");
    header[32..36].copy_from_slice(&0x05000205u32.to_le_bytes());
    cfb.create_stream("/FileHeader").unwrap().write_all(&header).unwrap();
    // 이것을 실패시키는 것: 캡션 경고 합성 입력의 원본 스타일 정의 제거.
    let mut info = make_record(TagId::CharShape, 0, &[0; 68]);
    info.extend(make_record(TagId::ParaShape, 0, &[0; 42]));
    info.extend(make_record(TagId::Style, 0, &[0; 14]));
    cfb.create_stream("/DocInfo").unwrap().write_all(&info).unwrap();
    cfb.create_storage("/BodyText").unwrap();
    cfb.create_stream("/BodyText/Section0").unwrap().write_all(section).unwrap();
    cfb.into_inner().into_inner()
}

/// 이것을 실패시키는 것: 구역 경고를 복사해 두 번 합류, 최종 문자열 dedup으로
/// 서로 다른 동일 캡션 손실을 하나로 합치기, caption 경고 자체를 없애기.
#[test]
fn r2_caption_events_merge_once_without_deduplicating_equal_losses() {
    let shape = r1_wrap(CTRL_ID_GSO, &r1_paragraph("shape body"));
    let mut captioned = Vec::new();
    for record in Record::parse_stream(&mut Cursor::new(shape)).unwrap() {
        captioned.extend(make_record(
            TagId::from(record.header.tag_id),
            record.header.level,
            &record.data,
        ));
        if TagId::from(record.header.tag_id) == TagId::CtrlHeader {
            captioned.extend(make_record(
                TagId::ListHeader,
                2,
                &caption_list_header(1, 3, 0, 0, 0),
            ));
            r1_shift(&mut captioned, &r1_paragraph("same lost caption"), 2);
        }
    }
    let captions = |warnings: &[Hwp5Warning]| {
        warnings
            .iter()
            .filter(|w| matches!(w, Hwp5Warning::DroppedControl { control: "caption", .. }))
            .count()
    };
    for count in [1, 2, 9] {
        let wire = captioned.repeat(count);
        assert_eq!(captions(&parse_body_text(&wire, &version()).unwrap().warnings), count);
        let bytes = r2_package(&wire);
        let intermediate = crate::decoder::decode_intermediate(&bytes).unwrap();
        assert_eq!(captions(&intermediate.warnings), count);
        assert_eq!(captions(&intermediate.sections[0].warnings), 0);
        assert_eq!(captions(&crate::Hwp5Decoder::decode(&bytes).unwrap().warnings), count);
        assert_eq!(captions(&crate::decode_hwp5_with_images(&bytes).unwrap().warnings), count);
        assert_eq!(captions(&crate::decode_hwp5_to_core(&bytes).unwrap().warnings), count);
    }
}

pub(crate) fn r3_asset_wire(id: u16) -> Vec<u8> {
    let mut wire = make_record(TagId::ParaHeader, 0, &para_header_data(0, 0));
    // Real inline anchors, with two distinct assets in the same paragraph.
    let mut text = para_text_with_control_ref("", "");
    text.extend(para_text_with_control_ref("", ""));
    wire.extend(make_record(TagId::ParaText, 1, &text));
    for (tag, data) in [
        (TagId::ShapeComponentOle, shape_component_ole_data(id, 7200, 7200)),
        (TagId::ShapePicture, shape_picture_data(id)),
    ] {
        wire.extend(make_record(
            TagId::CtrlHeader,
            1,
            &gso_ctrl_header_data(0, 0, 7200 + u32::from(id), 7200),
        ));
        // A complete unturned render block keeps this asset-routing test valid.
        wire.extend(make_record(
            TagId::ShapeComponent,
            2,
            &shape_component_render_data(b"cip$", true, 0, (7200 + u32::from(id), 7200), 0.0),
        ));
        wire.extend(make_record(tag, 2, &data));
    }
    wire
}

pub(crate) fn r3_chart_asset(id: u16) -> (String, Vec<u8>, Vec<u8>) {
    use std::io::Write;
    let xml = format!(
        r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:title>R3chart{id}</c:title></c:chart></c:chartSpace>"#
    );
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    cfb.create_stream("/OOXMLChartContents").unwrap().write_all(xml.as_bytes()).unwrap();
    let ole = cfb.into_inner().into_inner();
    let mut inflated = (ole.len() as u32).to_le_bytes().to_vec();
    inflated.extend(&ole);
    (xml, ole, inflated)
}

fn r3_encode(
    doc: hwpforge_core::Document<hwpforge_core::Draft>,
    images: &hwpforge_core::ImageStore,
) -> hwpforge_smithy_hwpx::EncodeOutcome {
    use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    // The synthetic Compose fixture refers to wire charPr 7.
    for _ in 0..=7 {
        styles.push_char_shape(HwpxCharShape::default());
    }
    styles.push_para_shape(HwpxParaShape::default());
    // 이것을 실패시키는 것: 합성 셀의 명시적 borderFill 1 정의 누락.
    // This helper authors its store directly; output defaults cannot resolve
    // the cell's original reference on its behalf.
    styles
        .push_border_fill(hwpforge_smithy_hwpx::style_store::HwpxBorderFill::default_page_border());
    hwpforge_smithy_hwpx::HwpxEncoder::encode_with_diagnostics(
        &doc.validate().unwrap(),
        &styles,
        images,
        Default::default(),
    )
    .unwrap()
}

fn r3_refs(xml: &str, attribute: &str) -> Vec<String> {
    xml.split(&format!("{attribute}=\""))
        .skip(1)
        .map(|s| s.split('"').next().unwrap().to_string())
        .collect()
}

/// 이것을 실패시키는 것: 중첩 자산 버퍼 폐기, 차트/OLE 번호 리셋·본문 자산 대체,
/// 이미지 저장소 누락, 경고 폐기. 각 XML 참조를 manifest→실제 ZIP 바이트로 역추적한다.
#[test]
// 이것을 실패시키는 것: 중첩 자산 reference를 본문 자산으로 치환 (실제 음성 대조 실행).
fn r3_asset_matrix_nested_only_and_distinct_body_pairs() {
    use crate::{Hwp5JoinedImageAsset, Hwp5JoinedImageAssetPlan, Hwp5OleAssetPlan};
    use std::collections::BTreeMap;
    let charts = [r3_chart_asset(1), r3_chart_asset(2)];
    let assets: Vec<_> = (1..=2)
        .map(|id| Hwp5JoinedImageAsset {
            payload: crate::Hwp5SemanticImagePayload {
                binary_data_id: id,
                storage_name: format!("R3image{id}.png"),
                package_path: format!("BinData/R3image{id}.png"),
                format: crate::Hwp5SemanticImageFormat::Png,
                width_hwp: None,
                height_hwp: None,
            },
            bytes: vec![0x89, b'P', b'N', b'G', id as u8],
        })
        .collect();
    let image_plan = Hwp5JoinedImageAssetPlan {
        assets_by_reference: assets
            .iter()
            .cloned()
            .map(|a| (a.payload.binary_data_id, a))
            .collect(),
        ordered_assets: assets.clone(),
        unavailable_reasons: BTreeMap::new(),
    };
    let ole_plan = Hwp5OleAssetPlan {
        assets_by_reference: charts
            .iter()
            .enumerate()
            .map(|(i, c)| ((i + 1) as u16, Ok(c.2.clone())))
            .collect(),
        reference_ambiguities: BTreeMap::new(),
    };
    for path in r2_paths() {
        for paired in [false, true] {
            let mut wire = if paired { r3_asset_wire(1) } else { Vec::new() };
            wire.extend(r2_in_path(&r3_asset_wire(2), &path));
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let (doc, images, projection_warnings) =
                crate::projection::project_to_core_with_images_and_ole(
                    vec![parsed],
                    &image_plan,
                    &ole_plan,
                    crate::decoder::package::DecompressBudget::new(
                        crate::decoder::package::MAX_TOTAL_DECOMPRESSED,
                    ),
                )
                .unwrap();
            r4_assert_projection_warnings(&projection_warnings, &[]);
            let outcome = r3_encode(doc, &images);
            assert!(
                outcome.warnings.iter().all(|w| !w.is_semantic_loss()),
                "{path:x?} paired={paired}: {:?}",
                outcome.warnings
            );
            let mut zip = hwpforge_smithy_hwpx::PackageReader::new(&outcome.bytes).unwrap();
            let xml = zip.read_section_xml(0).unwrap();
            let manifest = zip.read_text_entry("Contents/content.hpf").unwrap();
            let bindata = zip.read_all_bindata().unwrap();
            let chart_refs = r3_refs(&xml, "chartIDRef");
            let binary_refs = r3_refs(&xml, "binaryItemIDRef");
            let expected_ids = if paired { vec![1usize, 2] } else { vec![2] };
            assert_eq!(chart_refs.len(), expected_ids.len(), "{path:x?}");
            let mut seen_charts = Vec::new();
            for reference in &chart_refs {
                let actual = zip.read_text_entry(reference).unwrap();
                let id = expected_ids
                    .iter()
                    .find(|&&id| actual == charts[id - 1].0)
                    .copied()
                    .expect("own chart bytes");
                seen_charts.push(id);
            }
            seen_charts.sort();
            assert_eq!(seen_charts, expected_ids, "{path:x?}: replaced/duplicated chart");
            let mut seen_oles = Vec::new();
            let mut seen_images = Vec::new();
            for reference in binary_refs {
                let item = manifest
                    .split("<opf:item ")
                    .find(|item| item.starts_with(&format!("id=\"{reference}\"")))
                    .expect("manifest id");
                let href = r3_refs(item.split('>').next().unwrap(), "href").remove(0);
                let actual = bindata
                    .get(&href)
                    .or_else(|| bindata.get(href.strip_prefix("BinData/").unwrap()))
                    .expect("ZIP binary");
                if href.ends_with(".ole") {
                    let id = expected_ids
                        .iter()
                        .find(|&&id| actual == charts[id - 1].1)
                        .copied()
                        .expect("own OLE bytes");
                    seen_oles.push(id);
                } else {
                    let id = expected_ids
                        .iter()
                        .find(|&&id| actual == assets[id - 1].bytes)
                        .copied()
                        .expect("own image bytes");
                    seen_images.push(id);
                }
            }
            seen_oles.sort();
            seen_images.sort();
            assert_eq!(seen_oles, expected_ids, "{path:x?}: OLE");
            assert_eq!(seen_images, expected_ids, "{path:x?}: image");
            for picture in xml.split("<hp:pic ").skip(1) {
                let fragment = picture.split("</hp:pic>").next().unwrap();
                let image_ref = r3_refs(fragment, "binaryItemIDRef").remove(0);
                let id =
                    expected_ids.iter().find(|&&id| image_ref == format!("R3image{id}")).unwrap();
                assert!(
                    fragment.contains(&format!("width=\"{}\"", 7200 + id)),
                    "image bound to the wrong source object"
                );
            }
            // Pair each switch's chart and OLE, not just their unordered sets.
            for switch in xml.split("<hp:switch>").skip(1) {
                let fragment = switch.split("</hp:switch>").next().unwrap();
                let refs = r3_refs(fragment, "chartIDRef");
                if refs.is_empty() {
                    continue;
                }
                let chart = zip.read_text_entry(&refs[0]).unwrap();
                let id = charts.iter().position(|c| c.0 == chart).unwrap();
                assert!(
                    fragment.contains(&format!("width=\"{}\"", 7201 + id)),
                    "chart bound to the wrong source object"
                );
                let ole_ref = r3_refs(fragment, "binaryItemIDRef").remove(0);
                assert_eq!(bindata.get(&format!("{ole_ref}.ole")).unwrap(), charts[id].1);
            }
        }
    }
}

/// 이것을 실패시키는 것: 표 면적 사전 검사 제거·문서 누적 예산 초기화·오류 누락.
#[test]
fn r3_table_cell_budget_rejects_amplification_before_allocation() {
    fn table(rows: u16, cols: u16) -> Vec<u8> {
        let mut wire = r1_paragraph("budget");
        wire.extend(make_record(TagId::CtrlHeader, 1, &CTRL_ID_TABLE.to_le_bytes()));
        let mut payload = vec![0; 8];
        payload[4..6].copy_from_slice(&rows.to_le_bytes());
        payload[6..8].copy_from_slice(&cols.to_le_bytes());
        wire.extend(make_record(TagId::Table, 2, &payload));
        wire
    }
    let parsed = parse_body_text(&table(65535, 65535), &version()).unwrap();
    let err = crate::projection::project_to_core(vec![parsed]).unwrap_err();
    assert!(matches!(err, crate::Hwp5Error::RecordParse { .. }));
    assert!(err.to_string().contains("65535x65535"));
    // Exactly the per-table boundary succeeds; next row fails before generation.
    for (rows, succeeds) in [(256, true), (257, false)] {
        let parsed = parse_body_text(&table(rows, 256), &version()).unwrap();
        assert_eq!(crate::projection::project_to_core(vec![parsed]).is_ok(), succeeds);
    }
    // The document budget crosses section boundaries and nested hosts.
    for (count, succeeds) in [(4, true), (5, false)] {
        let sections = (0..count)
            .map(|i| {
                let path = if i % 2 == 0 { vec![] } else { vec![CTRL_ID_GSO] };
                parse_body_text(&r2_in_path(&table(256, 256), &path), &version()).unwrap()
            })
            .collect();
        assert_eq!(crate::projection::project_to_core(sections).is_ok(), succeeds);
    }
}

// Source provenance is tested separately by r3_header_footer_sibling_hosts_keep_wire_order.
fn r3_semantic_snapshot(value: &impl std::fmt::Debug) -> String {
    let debug = format!("{value:?}");
    let mut output = String::new();
    let mut rest = debug.as_str();
    while let Some((before, after)) = rest.split_once("wire_order: ") {
        output.push_str(before);
        rest = after.split_once(", ").unwrap().1;
    }
    output.push_str(rest);
    output
}

/// 이것을 실패시키는 것: 실제 셀이 있으면 면적 예산 우회, declared 크기만 검사,
/// 중복 셀의 생성 개수 무시. 거부된 표는 Core 셀을 만들지 않아야 한다.
#[test]
fn r3_explicit_cells_and_inferred_extents_obey_the_budget() {
    let wire = r1_wrap(CTRL_ID_TABLE, &r1_paragraph("cell"));
    let original = parse_body_text(&wire, &version()).unwrap();
    for case in 0..4 {
        let mut parsed = original.clone();
        let Hwp5Control::Table(table) = &mut parsed.paragraphs[0].controls[0] else {
            panic!("table")
        };
        match case {
            0 => {
                table.rows = 65535;
                table.cols = 65535;
            }
            1 => {
                table.cells[0].row = 65535;
                table.cells[0].row_span = 65535;
            }
            2 => {
                table.cells[0].column = 65535;
                table.cells[0].col_span = 65535;
            }
            _ => {
                table.cells = vec![table.cells[0].clone(); 65_536];
            }
        }
        let error = crate::projection::project_to_core(vec![parsed]).unwrap_err();
        assert!(error.to_string().contains("table cell budget exceeded"), "{case}: {error}");
    }
}

fn r4_marker(code: u16, id: u32) -> Vec<u8> {
    let mut b = code.to_le_bytes().to_vec();
    b.extend(id.to_le_bytes());
    b.extend([0; 8]);
    b.extend(code.to_le_bytes());
    b
}

fn r4_para(text: &[u8], controls: &[u8], cache: bool) -> Vec<u8> {
    let mut b = make_record(TagId::ParaHeader, 0, &para_header_data(0, 0));
    b.extend(make_record(TagId::ParaText, 1, text));
    if cache {
        b.extend(make_record(TagId::ParaLineSeg, 1, &para_line_seg_data(&[(0, 0, 1000)])));
    }
    b.extend(controls);
    b
}

fn r4_assert_projection_warnings(warnings: &[Hwp5Warning], expected: &[&str]) {
    for w in warnings {
        match w {
            Hwp5Warning::ProjectionFallback { subject, reason }
                if expected.contains(subject)
                    || *subject == "textbox.list_header_short"
                    || (*subject == "shape.text_margin" && reason == "text margin 0 0 0 0 of an ellipse or polygon is not carried; 283 is written")
                    || (*subject == "field.memo" && reason.contains("no matching FieldBegin")) => {}
            Hwp5Warning::LayoutCacheDropped { .. } => {}
            _ => panic!("unaccounted projection warning: {w:?}; expected {expected:?}"),
        }
    }
    for subject in expected {
        assert!(
            warnings.iter().any(
                |w| matches!(w, Hwp5Warning::ProjectionFallback { subject: s, .. } if s == subject)
            ),
            "missing {subject}: {warnings:?}"
        );
    }
}

fn r4_bookmark(name: &str, span: bool) -> (u32, Vec<u8>) {
    let id = if span {
        crate::ctrl_ids::CTRL_ID_BOOKMARK_SPAN
    } else {
        crate::ctrl_ids::CTRL_ID_BOOKMARK_POINT
    };
    let mut b = make_record(TagId::CtrlHeader, 1, &id.to_le_bytes());
    let mut data = vec![0x1b, 2, 1, 0, 0, 0, 0, 0x40, 1, 0];
    data.extend((name.encode_utf16().count() as u16).to_le_bytes());
    data.extend(para_text_data(name));
    b.extend(make_record(TagId::CtrlData, 2, &data));
    (id, b)
}

fn r4_command(id: u32, command: &str) -> Vec<u8> {
    let mut data = id.to_le_bytes().to_vec();
    data.extend([0; 4]);
    if id == CTRL_ID_FIELD_CROSSREF {
        data.push(0);
        data.extend((command.encode_utf16().count() as u16).to_le_bytes());
        data.extend(para_text_data(command));
        data.extend([0; 8]);
    } else {
        data.extend((command.encode_utf16().count() as u16).to_be_bytes());
        for u in command.encode_utf16() {
            data.extend(u.to_be_bytes());
        }
    }
    make_record(TagId::CtrlHeader, 1, &data)
}

/// 이것을 실패시키는 것: 채워진 누름틀의 display_text를 비우거나 빈 필드 힌트를 본문으로 미리 복사.
#[test]
fn r5_clickhere_preserves_body_and_only_empty_uses_hint() {
    for body in ["", " ", "가상시 가상로 2 가상청사", "a😀\t나\n끝"] {
        let mut text = r4_marker(3, CTRL_ID_CLICK_HERE);
        for ch in body.chars() {
            if ch == '\t' {
                text.extend(r4_marker(9, 0));
            } else {
                text.extend(para_text_data(&ch.to_string()));
            }
        }
        text.extend(r4_marker(4, 0));
        let control = make_record(TagId::CtrlHeader, 1, &r1_clickhere());
        let parsed = parse_body_text(&r4_para(&text, &control, false), &version()).unwrap();
        let (doc, _) = crate::projection::project_to_core(vec![parsed]).unwrap();
        let field = doc.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|run| {
                if let hwpforge_core::RunContent::Control(c) = &run.content {
                    if let hwpforge_core::Control::Field { display_text, .. } = c.as_ref() {
                        return Some(display_text.as_str());
                    }
                }
                None
            })
            .unwrap_or_else(|| panic!("body={body:?} doc={doc:?}"));
        assert_eq!(field, body);
        let xml = r2_xml(doc);
        if body.is_empty() {
            assert!(xml.contains("<hp:t>힌</hp:t>"));
        } else {
            assert!(!xml.contains("<hp:t>힌</hp:t>"));
        }
    }
}

/// 이것을 실패시키는 것: 채워진 누름틀의 display_text 삭제 또는 두 번째 형제 필드 누락.
#[test]
fn synthetic_contact_fields_preserve_filled_clickhere() {
    for expected in [vec!["가상시 별빛로 7 탐험대"], vec!["user@example.com", "가상 연락처"]]
    {
        let mut payload = Vec::new();
        for body in &expected {
            payload.extend(r4_marker(3, CTRL_ID_CLICK_HERE));
            payload.extend(para_text_data(body));
            payload.extend(r4_marker(4, 0));
        }
        let controls = make_record(TagId::CtrlHeader, 1, &r1_clickhere()).repeat(expected.len());
        let parsed = parse_body_text(&r4_para(&payload, &controls, false), &version()).unwrap();
        let (doc, _) = crate::projection::project_to_core(vec![parsed]).unwrap();
        let actual: Vec<_> = doc.sections()[0].paragraphs[0]
            .runs
            .iter()
            .filter_map(|r| match &r.content {
                hwpforge_core::RunContent::Control(c) => match c.as_ref() {
                    hwpforge_core::Control::Field { display_text, .. } => {
                        Some(display_text.as_str())
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(actual, expected);
        let xml = r2_xml(doc);
        for body in expected {
            assert_eq!(xml.matches(body).count(), 1);
        }
    }
}

fn ck_encode(doc: hwpforge_core::Document<hwpforge_core::Draft>) -> Vec<u8> {
    ck_encode_with_images(doc, &hwpforge_core::ImageStore::new())
}

fn ck_encode_with_images(
    doc: hwpforge_core::Document<hwpforge_core::Draft>,
    images: &hwpforge_core::ImageStore,
) -> Vec<u8> {
    use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    for _ in 0..72 {
        styles.push_char_shape(HwpxCharShape::default());
    }
    styles.push_para_shape(HwpxParaShape::default());
    hwpforge_smithy_hwpx::HwpxEncoder::encode(&doc.validate().unwrap(), &styles, images).unwrap()
}

fn ck_mixed_wire() -> Vec<u8> {
    let mut text = para_text_data("앞");
    text.extend(r4_marker(3, CTRL_ID_CLICK_HERE));
    text.extend(para_text_data("가😀나끝"));
    text.extend(r4_marker(4, 0));
    text.extend(para_text_data("뒤"));
    let shapes: Vec<u8> = [(0u32, 9u32), (9, 1), (12, 2), (13, 1), (22, 9)]
        .into_iter()
        .flat_map(|(p, s)| p.to_le_bytes().into_iter().chain(s.to_le_bytes()))
        .collect();
    let mut controls = make_record(TagId::ParaCharShape, 1, &shapes);
    controls.extend(make_record(TagId::CtrlHeader, 1, &r1_clickhere()));
    r4_para(&text, &controls, true)
}

fn ck_body_shapes(runs: &[hwpforge_core::Run]) -> Vec<(usize, String)> {
    use hwpforge_core::control::{Control, FieldBoundary};
    let mut inside = false;
    let mut body = Vec::new();
    for run in runs {
        match run.content.as_control() {
            Some(Control::FieldBoundary { boundary: FieldBoundary::Start(_) }) => {
                assert!(!inside);
                inside = true;
            }
            Some(Control::FieldBoundary { boundary: FieldBoundary::End { .. } }) => {
                assert!(inside);
                inside = false;
            }
            _ if inside => {
                body.push((run.char_shape_id.get(), run.content.plain_text().unwrap().into_owned()))
            }
            _ => {}
        }
    }
    assert!(!inside);
    body
}

/// 이것을 실패시키는 것: 경계 id를 그대로 출력 / 끝 참조 재배정 누락 /
/// 전체 문서의 개체·경계 예약 전에 새 id 발급 / 재배정 표를 인코드 간 공유.
#[test]
fn ck_mixed_clickhere_ids_avoid_preserved_picture_id() {
    use hwpforge_core::image::{Image, ImageFormat};
    use hwpforge_core::{ImageStore, ObjectId, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge_smithy_hwpx::{HwpxDecoder, PackageReader};

    let mut wire = ck_mixed_wire();
    wire.extend(ck_mixed_wire());
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (mut doc, _) = crate::projection::project_to_core(vec![parsed]).unwrap();
    let mut picture = Image::new(
        "picture.png",
        HwpUnit::new(1000).unwrap(),
        HwpUnit::new(1000).unwrap(),
        ImageFormat::Png,
    );
    // Core preserves the HWP5 picture's instance_id in inst_id.
    picture.inst_id = Some(ObjectId::new(1));
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::image(picture, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let mut images = ImageStore::new();
    images.insert("picture.png", vec![89, 1]);

    for picture_first in [false, true] {
        if picture_first {
            doc.sections_mut().swap(0, 1);
        }
        let bytes = ck_encode_with_images(doc.clone(), &images);
        assert_eq!(bytes, ck_encode_with_images(doc.clone(), &images));
        let mut package = PackageReader::new(&bytes).unwrap();
        let field_section = usize::from(picture_first);
        let fields = package.read_section_xml(field_section).unwrap();
        let picture = package.read_section_xml(1 - field_section).unwrap();
        assert!(picture.contains("<hp:pic id=\"1\""), "{picture}");
        let starts: Vec<_> = fields
            .split("<hp:fieldBegin ")
            .skip(1)
            .map(|head| r3_refs(head.split('>').next().unwrap(), "id")[0].clone())
            .collect();
        let ends = r3_refs(&fields, "beginIDRef");
        assert_eq!(starts, ends);
        assert_eq!(starts, ["3", "2"]); // 2 was reserved before relocating 1.
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        for (before, after) in doc.sections()[field_section]
            .paragraphs
            .iter()
            .zip(&decoded.document.sections()[field_section].paragraphs)
        {
            assert_eq!(ck_body_shapes(&before.runs), ck_body_shapes(&after.runs));
        }
    }
}

const SYNTHETIC_FIELD_SPANS: [(u32, &str); 6] = [
    (1, "출발"),
    (2, "별빛을 모은다 "),
    (3, "😀"),
    (4, "상자에 담는다 "),
    (5, "새로운 길을 찾는다 "),
    (6, "끝으로 돌아온다."),
];

fn synthetic_unclosed_clickhere() -> Vec<u8> {
    let mut text = r4_marker(3, CTRL_ID_CLICK_HERE);
    let mut shapes = Vec::new();
    for (i, (shape, body)) in SYNTHETIC_FIELD_SPANS.iter().enumerate() {
        let position = if i == 0 { 0 } else { (text.len() / 2) as u32 };
        shapes.extend(position.to_le_bytes());
        shapes.extend(shape.to_le_bytes());
        text.extend(para_text_data(body));
    }
    shapes.extend(((text.len() / 2) as u32).to_le_bytes());
    shapes.extend(9u32.to_le_bytes());
    let page_id = u32::from_be_bytes(*b"pgnp");
    text.extend(r4_marker(0x15, page_id)); // deliberately no field-end marker
    let mut header = r1_clickhere_with_direction("가상 안내문을 입력");
    header[4..8].copy_from_slice(&0x8001u32.to_le_bytes());
    let mut controls = make_record(TagId::ParaCharShape, 1, &shapes);
    controls.extend(make_record(TagId::CtrlHeader, 1, &header));
    let name = Record::parse_stream(&mut Cursor::new(r4_bookmark("가상필드", false).1)).unwrap();
    controls.extend(make_record(TagId::CtrlData, 2, &name[1].data));
    let mut page = page_id.to_le_bytes().to_vec();
    page.extend(0x500u32.to_le_bytes());
    page.extend([0; 6]);
    page.extend(('-' as u16).to_le_bytes());
    controls.extend(make_record(TagId::CtrlHeader, 1, &page));
    r4_para(&text, &controls, true)
}

/// 합성 레코드의 바이트와 비교하여 투영 결과를 정답으로 재사용하지 않는다.
/// 이것을 실패시키는 것: editable을 0으로 또는 본문 run의 서식·텍스트·순서 변경.
/// 실제 변이 실행: Direction을 첫 글자로 자르면 "가" != "가상 안내문을 입력"으로 실패한다.
#[test]
fn synthetic_clickhere_metadata_and_body_match_wire() {
    use hwpforge_core::control::{Control, FieldBoundary, FieldParameter};
    let wire = synthetic_unclosed_clickhere();
    let records = Record::parse_stream(&mut Cursor::new(&wire)).unwrap();
    let data =
        |tag| records.iter().find(|r| TagId::from(r.header.tag_id) == tag).unwrap().data.as_slice();
    let units = |bytes: &[u8]| {
        bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>()
    };
    let header = data(TagId::CtrlHeader);
    assert_eq!(&header[..4], &CTRL_ID_CLICK_HERE.to_le_bytes());
    let properties = u32::from_le_bytes(header[4..8].try_into().unwrap());
    assert_eq!(properties, 0x8001);
    let command_len = u16::from_le_bytes(header[9..11].try_into().unwrap()) as usize;
    let command = String::from_utf16(&units(&header[11..11 + command_len * 2])).unwrap();
    let direction = "가상 안내문을 입력".to_string();
    assert_eq!(
        command,
        "Clickhere:set:0:Direction:wstring:10:가상 안내문을 입력 HelpState:wstring:0:  "
    );
    let name_data = data(TagId::CtrlData);
    let name_len = u16::from_le_bytes(name_data[10..12].try_into().unwrap()) as usize;
    let name = String::from_utf16(&units(&name_data[12..12 + name_len * 2])).unwrap();
    assert_eq!(name, "가상필드");

    let text = units(data(TagId::ParaText));
    assert_eq!(text[0], 3); // 8-unit field-begin marker
    assert_eq!(text[text.len() - 8], 0x15); // following page-number control, outside the body
    let spans: Vec<_> = data(TagId::ParaCharShape)
        .chunks_exact(8)
        .map(|b| {
            (
                u32::from_le_bytes(b[..4].try_into().unwrap()) as usize,
                u32::from_le_bytes(b[4..].try_into().unwrap()) as usize,
            )
        })
        .collect();
    let expected: Vec<_> = spans
        .windows(2)
        .map(|pair| (pair[0].1, String::from_utf16(&text[pair[0].0.max(8)..pair[1].0]).unwrap()))
        .collect();

    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, _) = crate::projection::project_to_core(vec![parsed]).unwrap();
    let runs = &doc.sections()[0].paragraphs[0].runs;
    let start = runs
        .iter()
        .find_map(|run| match run.content.as_control() {
            Some(Control::FieldBoundary { boundary: FieldBoundary::Start(start) }) => Some(start),
            _ => None,
        })
        .unwrap();
    assert_eq!(start.name, name);
    // HWP5 field properties bit 0: editable in read-only mode.
    assert_eq!(start.editable, (properties & 1).to_string());
    assert_eq!(
        start.parameters,
        vec![
            FieldParameter::Integer { name: "Prop".into(), value: header[8].to_string() },
            FieldParameter::String {
                name: "Command".into(),
                value: command,
                xml_space: "preserve".into()
            },
            FieldParameter::String {
                name: "Direction".into(),
                value: direction,
                xml_space: String::new()
            },
        ]
    );
    assert_eq!(ck_body_shapes(runs), expected);
}

/// 이것을 실패시키는 것: 본문을 첫 서식으로 평탄화 / 필드 밖 방출 / 중복 방출 /
/// UTF-16 경계 오산 / 범위 본문을 Folded ledger로 계산 / 필드 id를 문단마다 재사용.
#[test]
fn ck_mixed_clickhere_matches_hwpx_ranges_and_fill_policy() {
    use hwpforge_core::control::{Control, FieldBoundary, FieldParameter};
    use hwpforge_core::field_inventory::FieldInventory;
    use hwpforge_smithy_hwpx::{FillError, HwpxDecoder, HwpxFiller};
    let mut wire = ck_mixed_wire();
    wire.extend(ck_mixed_wire());
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (mut doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
    assert!(
        !warnings.iter().any(|w| matches!(w, Hwp5Warning::LayoutCacheDropped { .. })),
        "{warnings:?}"
    );
    let expected = vec![(1, "가😀".into()), (2, "나".into()), (1, "끝".into())];
    let mut ids = Vec::new();
    for (i, p) in doc.sections_mut()[0].paragraphs.iter_mut().enumerate() {
        assert_eq!(ck_body_shapes(&p.runs), expected);
        assert_eq!(p.runs.first().unwrap().content.plain_text().unwrap(), "앞");
        assert_eq!(p.runs.last().unwrap().content.plain_text().unwrap(), "뒤");
        let Control::FieldBoundary { boundary: FieldBoundary::Start(start) } =
            p.runs[1].content.as_control().unwrap()
        else {
            panic!("start")
        };
        assert_eq!(start.kind, "CLICK_HERE");
        assert!(start.parameters.iter().any(|p| matches!(p,
            FieldParameter::String { name, value, .. } if name == "Direction" && value == "힌")));
        assert!(start.parameters.iter().any(|p| matches!(p,
            FieldParameter::String { name, value, .. } if name == "Command" && value.contains("HelpState"))));
        ids.push(start.id.clone());
        // Name only the synthetic fields so the existing public fill path can address them.
        let hwpforge_core::RunContent::Control(c) = &mut p.runs[1].content else { unreachable!() };
        let Control::FieldBoundary { boundary: FieldBoundary::Start(start) } = c.as_mut() else {
            unreachable!()
        };
        start.name = format!("mixed{i}");
    }
    assert_ne!(ids[0], ids[1]);
    let inventory = FieldInventory::from_sections(doc.sections());
    assert_eq!(inventory.fields().len(), 2);
    assert!(inventory.fields().iter().all(|f| f.current() == "가😀나끝"));
    let json = serde_json::to_string(&doc).unwrap();
    let restored = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, restored);
    let bytes = ck_encode(restored);
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    for (before, after) in
        doc.sections()[0].paragraphs.iter().zip(&decoded.document.sections()[0].paragraphs)
    {
        // HWPX decoder produces the identical boundary metadata and styled body runs.
        assert_eq!(before.runs, after.runs);
    }
    let fields = HwpxFiller::list_fields(&bytes).unwrap();
    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|f| !f.fillable && f.current == "가😀나끝"));
    // Existing fill policy admits exactly one body run. Preserve that policy;
    // projecting a range must not silently turn a multi-style value into one run.
    assert!(matches!(
        HwpxFiller::fill(&bytes, &[("mixed0".into(), "새값😀".into())].into()),
        Err(FillError::UnfillableField { .. })
    ));
}

/// 이것을 실패시키는 것: 여러 본문 서식을 평탄화하거나 unclosed/object 경고 subject 변조.
#[test]
fn synthetic_clickhere_preserves_six_spans_and_projection_warnings() {
    use hwpforge_core::field_inventory::FieldInventory;
    use hwpforge_smithy_hwpx::{FillError, HwpxDecoder, HwpxFiller, PackageReader};
    let wire = synthetic_unclosed_clickhere();
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
    r4_assert_projection_warnings(&warnings, &["field.unclosed", "field.object"]);
    let runs = &doc.sections()[0].paragraphs[0].runs;
    assert!(matches!(
        runs.last().unwrap().content.as_control(),
        Some(hwpforge_core::Control::PageNumber { .. })
    ));
    let body = ck_body_shapes(runs);
    assert_eq!(
        body,
        SYNTHETIC_FIELD_SPANS
            .iter()
            .map(|(shape, text)| (*shape as usize, text.to_string()))
            .collect::<Vec<_>>()
    );
    let text: String = body.iter().map(|(_, t)| t.as_str()).collect();
    let bytes = ck_encode(doc);
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    assert_eq!(ck_body_shapes(&decoded.document.sections()[0].paragraphs[0].runs), body);
    let fields = FieldInventory::from_sections(decoded.document.sections());
    assert_eq!(fields.fields().len(), 1);
    assert_eq!(fields.fields()[0].current(), text);
    let xml = PackageReader::new(&bytes).unwrap().read_section_xml(0).unwrap();
    assert_eq!(xml.matches("type=\"CLICK_HERE\"").count(), 1);
    assert_eq!(xml.matches("<hp:fieldEnd ").count(), 1);
    assert_eq!(xml.matches("<hp:pageNum ").count(), 1);
    // Every non-space piece occurs exactly once in the emitted field body.
    for (_, piece) in body.iter().filter(|(_, t)| t.trim().chars().count() > 1) {
        assert_eq!(xml.matches(piece.as_str()).count(), 1);
    }
    let fields = HwpxFiller::list_fields(&bytes).unwrap();
    assert!(!fields[0].fillable);
    let name = fields[0].name.clone().unwrap();
    assert!(matches!(
        HwpxFiller::fill(&bytes, &[(name, "검증한 새 본문".into())].into()),
        Err(FillError::UnfillableField { .. })
    ));
}

/// 이것을 실패시키는 것: 빈/단일 서식도 범위로 바꾸기, 앞/뒤 서식을 본문 서식으로 오인.
#[test]
fn ck_single_shape_keeps_legacy_core_and_bytes() {
    use hwpforge_core::{Control, Run};
    for body in ["", "가😀나끝"] {
        let mut text = para_text_data("앞");
        text.extend(r4_marker(3, CTRL_ID_CLICK_HERE));
        text.extend(para_text_data(body));
        text.extend(r4_marker(4, 0));
        text.extend(para_text_data("뒤"));
        let end = 9 + body.encode_utf16().count() as u32;
        let shapes: Vec<u8> = [(0u32, 9u32), (9, 1), (end + 8, 9)]
            .into_iter()
            .flat_map(|(p, s)| p.to_le_bytes().into_iter().chain(s.to_le_bytes()))
            .collect();
        let mut controls = make_record(TagId::ParaCharShape, 1, &shapes);
        controls.extend(make_record(TagId::CtrlHeader, 1, &r1_clickhere()));
        let parsed = parse_body_text(&r4_para(&text, &controls, false), &version()).unwrap();
        let (doc, _) = crate::projection::project_to_core(vec![parsed]).unwrap();
        let expected = Run::control(
            Control::Field {
                field_type: hwpforge_foundation::FieldType::ClickHere,
                hint_text: Some("힌".into()),
                help_text: None,
                name: None,
                display_text: body.into(),
            },
            hwpforge_foundation::CharShapeIndex::new(if body.is_empty() { 9 } else { 1 }),
        );
        assert_eq!(doc.sections()[0].paragraphs[0].runs[1], expected);
        let mut legacy = doc.clone();
        legacy.sections_mut()[0].paragraphs[0].runs[1] = expected;
        assert_eq!(ck_encode(doc.clone()), ck_encode(legacy));
        let mut named = doc;
        let hwpforge_core::RunContent::Control(c) =
            &mut named.sections_mut()[0].paragraphs[0].runs[1].content
        else {
            unreachable!()
        };
        let Control::Field { name, .. } = c.as_mut() else { unreachable!() };
        *name = Some("single".into());
        let bytes = ck_encode(named);
        let filled = hwpforge_smithy_hwpx::HwpxFiller::fill(
            &bytes,
            &[("single".into(), "새값".into())].into(),
        )
        .unwrap();
        assert_eq!(
            hwpforge_smithy_hwpx::HwpxFiller::list_fields(&filled.bytes).unwrap()[0].current,
            "새값"
        );
    }
}

// 이것을 실패시키는 것: charPr/paraPr/style lookup 각각 생략, 또는 level 0만 검사.
#[test]
fn body_style_references_check_original_definitions_in_every_container() {
    let mut info = make_record(TagId::CharShape, 0, &[0; 68]);
    info.extend(make_record(TagId::ParaShape, 0, &[0; 42]));
    info.extend(make_record(TagId::Style, 0, &[0; 14]));
    let definitions = crate::decoder::header::parse_doc_info(&info, &version()).unwrap();
    assert_eq!(definitions.char_shapes.len(), 1);
    assert_eq!(definitions.para_shapes.len(), 1);
    assert_eq!(definitions.styles.len(), 1);
    // 이것을 실패시키는 것: 기존 잘린 레코드의 경고/fallback을 새 hard error로 바꾸기.
    for tag in [TagId::ParaHeader, TagId::ParaCharShape] {
        validate_body_style_references(&make_record(tag, 3, &[1]), &definitions).unwrap();
    }
    let hosts = [
        CTRL_ID_TABLE,
        CTRL_ID_HEADER,
        CTRL_ID_FOOTER,
        CTRL_ID_FOOTNOTE,
        CTRL_ID_ENDNOTE,
        CTRL_ID_GSO,
        CTRL_ID_MEMO,
        R1_CAPTION,
        R1_ELLIPSE,
        R1_POLYGON,
        R1_GROUP,
        R1_NESTED_GROUP,
    ];
    for host in hosts {
        for kind in ["charPr", "paraPr", "style"] {
            for id in [0, 1, 2, 3] {
                let mut header = para_header_data(
                    if kind == "paraPr" { id } else { 0 },
                    if kind == "style" { id as u8 } else { 0 },
                );
                header[12..14].copy_from_slice(&1u16.to_le_bytes());
                let mut leaf = make_record(TagId::ParaHeader, 0, &header);
                leaf.extend(make_record(TagId::ParaText, 1, &para_text_data("leaf")));
                let mut run = 0u32.to_le_bytes().to_vec();
                run.extend_from_slice(
                    &(if kind == "charPr" { u32::from(id) } else { 0 }).to_le_bytes(),
                );
                leaf.extend(make_record(TagId::ParaCharShape, 1, &run));
                let bytes = r1_in_path(&leaf, &[CTRL_ID_TABLE, host]);
                let result = validate_body_style_references(&bytes, &definitions);
                if id == 0 {
                    result.unwrap();
                } else {
                    let error = result.unwrap_err().to_string();
                    assert!(
                        error.contains(&format!("broken {kind} reference ID {id}")),
                        "host={host:x}: {error}"
                    );
                }
                let mut empty = crate::decoder::header::parse_doc_info(&info, &version()).unwrap();
                match kind {
                    "charPr" => empty.char_shapes.clear(),
                    "paraPr" => empty.para_shapes.clear(),
                    _ => empty.styles.clear(),
                }
                let error = validate_body_style_references(&bytes, &empty).unwrap_err().to_string();
                assert!(error.contains(kind), "{error}");
            }
        }
    }
}

/// 이것을 실패시키는 것: native Arc의 render 파싱 실패를 종류 판정에서 걸러 객체와 구체 경고를 버림.
#[test]
fn standalone_native_arc_keeps_its_kind_when_the_matrix_is_truncated() {
    let mut render = shape_component_render_data(b"cra$", true, 0, (1000, 1000), 0.0);
    render.truncate(100);
    let mut arc = vec![0u8];
    for v in [0i32, 0, 1000, 0, 0, 1000] {
        arc.extend(v.to_le_bytes());
    }
    let mut wire = make_record(TagId::ParaHeader, 0, &para_header_data(0, 0));
    wire.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
    wire.extend(make_record(TagId::CtrlHeader, 1, &gso_ctrl_header_data(0, 0, 1000, 1000)));
    wire.extend(make_record(TagId::ShapeComponent, 2, &render));
    wire.extend(make_record(TagId::ShapeComponentArc, 2, &arc));
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
    let mut arcs = 0;
    doc.for_each_paragraph(|p| {
        for run in &p.runs {
            if let hwpforge_core::RunContent::Control(c) = &run.content {
                if let hwpforge_core::Control::Arc { width, height, style, .. } = c.as_ref() {
                    arcs += 1;
                    assert_eq!((width.as_i32(), height.as_i32()), (1000, 1000));
                    assert!(style
                        .as_ref()
                        .is_none_or(|s| s.rotation.is_none() && s.flip.is_none()));
                }
            }
        }
    });
    assert_eq!(arcs, 1);
    assert!(warnings.iter().any(|w|matches!(w,crate::Hwp5Warning::ContentLost { kind,reason,.. } if kind=="shape_transform" && reason.contains("matrix chain:"))));
}

/// 이것을 실패시키는 것: native 호 읽기를 회전 또는 화살표 존재에 종속시키기.
#[test]
fn unturned_native_arc_is_read_without_arrows() {
    for properties in [0u32, (4 << 22) | (4 << 26) | (3 << 30), 1 << 10, 1 << 16, 7 << 16] {
        let mut render = shape_component_render_data(b"cra$", true, 0, (1000, 1000), 0.0);
        let props_at = render.len() - 17 + 8;
        render[props_at..props_at + 4].copy_from_slice(&properties.to_le_bytes());
        let mut arc = vec![0u8];
        for v in [0i32, 0, 1000, 0, 0, 1000] {
            arc.extend(v.to_le_bytes());
        }
        let mut wire = make_record(TagId::ParaHeader, 0, &para_header_data(0, 0));
        wire.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
        wire.extend(make_record(TagId::CtrlHeader, 1, &gso_ctrl_header_data(0, 0, 1000, 1000)));
        wire.extend(make_record(TagId::ShapeComponent, 2, &render));
        wire.extend(make_record(TagId::ShapeComponentArc, 2, &arc));
        let parsed = parse_body_text(&wire, &version()).unwrap();
        let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
        let mut arcs = 0;
        doc.for_each_paragraph(|p| {
            for run in &p.runs {
                if let hwpforge_core::RunContent::Control(c) = &run.content {
                    if matches!(c.as_ref(), hwpforge_core::Control::Arc { .. }) {
                        arcs += 1;
                    }
                }
            }
        });
        assert_eq!(arcs, 1);
        if properties == 7 << 16 {
            assert!(warnings.iter().any(|w| format!("{w:?}").contains("tail arrow shape 7")));
        }
    }
}

/// 이것을 실패시키는 것: 묶음 자식 rect 레코드의 곡률 byte를 수집하지 않기.
#[test]
fn group_rectangle_collects_its_corner_byte() {
    let source = r1_wrap(R1_GROUP, &r1_paragraph("corner"));
    let mut wire = Vec::new();
    for mut record in Record::parse_stream(&mut Cursor::new(source)).unwrap() {
        if TagId::from(record.header.tag_id) == TagId::ShapeComponentRect {
            record.data[0] = 25;
        }
        wire.extend(make_record(
            TagId::from(record.header.tag_id),
            record.header.level,
            &record.data,
        ));
    }
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let Hwp5Control::Group(g) = &parsed.paragraphs[0].controls[0] else { panic!("group") };
    let Hwp5Control::Rect(r) = &g.children[0].control else { panic!("rect") };
    assert_eq!(r.look.as_ref().unwrap().corner_ratio, 25);
}

/// 이것을 실패시키는 것: 잘린 look의 비영 곡률 경고 제거 / 곡률 0도 경고 /
/// 정상 look에도 손실 경고 / 중첩 묶음에서 곡률 진단 누락.
#[test]
fn group_corner_ratio_without_a_look_is_reported() {
    use crate::schema::section::Hwp5ShapeRender;

    for host in [R1_GROUP, R1_NESTED_GROUP] {
        for truncated in [false, true] {
            for ratio in [0, 25] {
                let source = r1_wrap(host, &r1_paragraph("corner"));
                let mut wire = Vec::new();
                for mut record in Record::parse_stream(&mut Cursor::new(source)).unwrap() {
                    let tag = TagId::from(record.header.tag_id);
                    if tag == TagId::ShapeComponent && record.data.starts_with(b"cer$") {
                        if truncated {
                            // Keep valid dimensions/matrices; cut only the 13-byte
                            // outline and 4-byte fill-kind block from this child.
                            record.data.truncate(record.data.len() - 17);
                            assert!(Hwp5ShapeRender::parse(&record.data, false).is_ok());
                            assert!(Hwp5ShapeRender::parse_look(&record.data, false).is_err());
                        }
                    } else if tag == TagId::ShapeComponentRect {
                        record.data[0] = ratio;
                    }
                    wire.extend(make_record(tag, record.header.level, &record.data));
                }
                let parsed = parse_body_text(&wire, &version()).unwrap();
                let warnings: Vec<_> = parsed
                    .warnings
                    .iter()
                    .filter_map(|w| match w {
                        Hwp5Warning::ProjectionFallback {
                            subject: "shape.corner_ratio",
                            reason,
                        } => Some(reason.as_str()),
                        _ => None,
                    })
                    .collect();
                let expected = if truncated && ratio > 0 {
                    vec!["corner ratio 25 not carried: the shape's outline and fill block is unreadable"]
                } else {
                    Vec::new()
                };
                assert_eq!(warnings, expected, "host={host}, truncated={truncated}, ratio={ratio}");
            }
        }
    }
}

/// 이것을 실패시키는 것: native 3 이상을 Normal로 대체하거나 잘림을 일반 UnsupportedTag로만 보고하기.
#[test]
fn native_arc_bad_type_and_truncation_are_specific_in_both_hosts() {
    for grouped in [false, true] {
        for (kind, len, ellipse) in
            [(3, 25, false), (255, 25, false), (0, 24, false), (0, 0, false), (2, 59, true)]
        {
            let mut wire = make_record(TagId::ParaHeader, 0, &para_header_data(0, 0));
            wire.extend(make_record(TagId::ParaText, 1, &para_text_with_control_ref("", "")));
            wire.extend(make_record(TagId::CtrlHeader, 1, &gso_ctrl_header_data(0, 0, 1000, 1000)));
            if grouped {
                wire.extend(make_record(
                    TagId::ShapeComponent,
                    2,
                    &shape_component_render_data(
                        &SHAPE_COMPONENT_TYPE_GROUP,
                        true,
                        0,
                        (1000, 1000),
                        0.0,
                    ),
                ));
            }
            let level = if grouped { 3 } else { 2 };
            wire.extend(make_record(
                TagId::ShapeComponent,
                level,
                &shape_component_render_data(
                    b"cra$",
                    !grouped,
                    u16::from(grouped),
                    (1000, 1000),
                    0.0,
                ),
            ));
            let mut data = vec![kind];
            for v in [0i32, 0, 1000, 0, 0, 1000] {
                data.extend(v.to_le_bytes());
            }
            data.truncate(len);
            if ellipse {
                data.resize(len, 0);
            }
            wire.extend(make_record(
                if ellipse { TagId::ShapeComponentEllipse } else { TagId::ShapeComponentArc },
                level + 1,
                &data,
            ));
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let parse_warnings = format!("{:?}", parsed.warnings);
            let (_, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
            let warnings = format!("{parse_warnings} {warnings:?}");
            assert!(
                warnings.contains(if ellipse {
                    "shape.ellipse_record"
                } else if len < 25 {
                    "shape.arc_record"
                } else {
                    "shape.arc_type"
                }),
                "{grouped} {kind} {len}: {warnings}"
            );
            assert!(
                warnings.contains(&if ellipse || len < 25 {
                    format!("{len} bytes")
                } else {
                    format!("native arc type {kind}")
                }),
                "{warnings}"
            );
        }
    }
}

/// 이것을 실패시키는 것: 채우기 헤더가 짧다는 이유로 정상 선 13바이트 폐기,
/// fill_kind=0 대체값을 정상 없음으로 오인, 선 손실 경고 누락, 묶음만 복구.
#[test]
fn partial_look_preserves_lines_and_reports_each_loss_in_every_host() {
    use hwpforge_core::{Control, RunContent};
    use hwpforge_foundation::{ArrowType, Color};
    fn shape(control: &Control) -> &hwpforge_core::control::ShapeStyle {
        match control {
            Control::Group { children, .. } => match &children[0] {
                hwpforge_core::control::GroupChild::Shape(c) => shape(c),
                other => panic!("{other:?}"),
            },
            Control::Rect { style, .. } | Control::TextBox { style, .. } => style.as_ref().unwrap(),
            other => panic!("unexpected {other:?}"),
        }
    }
    for host in [CTRL_ID_GSO, R1_GROUP, R1_NESTED_GROUP] {
        // Every truncation boundary, complete no-fill, and a broken fill body.
        for len in 0..=18 {
            let source = r1_wrap(host, &r1_paragraph("look"));
            let mut wire = Vec::new();
            for mut record in Record::parse_stream(&mut Cursor::new(source)).unwrap() {
                let tag = TagId::from(record.header.tag_id);
                if tag == TagId::ShapeComponent && record.data.starts_with(b"cer$") {
                    let end = record.data.len() - 17;
                    record.data.truncate(end);
                    let mut tail = 0x00332211u32.to_le_bytes().to_vec();
                    tail.extend(75i32.to_le_bytes());
                    tail.extend(
                        (1u32 | (1 << 10) | (4 << 22) | (4 << 26) | (3 << 30)).to_le_bytes(),
                    );
                    tail.push(0);
                    tail.extend(u32::from(len == 18).to_le_bytes());
                    tail.push(0);
                    record.data.extend(&tail[..len]);
                }
                wire.extend(make_record(tag, record.header.level, &record.data));
            }
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let line_lost = parsed.warnings.iter().any(|w| {
                matches!(w,
                Hwp5Warning::ProjectionFallback { subject: "shape.line", reason }
                if reason.contains("13 needed") && reason.contains("arrows not carried"))
            });
            assert_eq!(line_lost, len < 13, "host={host}, len={len}");
            let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
            let fill_lost = warnings.iter().any(|w| {
                matches!(w, Hwp5Warning::ProjectionFallback { subject: "shape.fill", .. })
            });
            assert_eq!(fill_lost, len != 17, "host={host}, len={len}: {warnings:?}");
            if len >= 13 {
                let control = doc.sections()[0]
                    .paragraphs
                    .iter()
                    .flat_map(|p| &p.runs)
                    .find_map(|r| {
                        if let RunContent::Control(c) = &r.content {
                            Some(c.as_ref())
                        } else {
                            None
                        }
                    })
                    .unwrap();
                let style = shape(control);
                assert_eq!(style.line_color, Some(Color::from_raw(0x00332211)));
                assert_eq!(style.line_width.unwrap(), 75);
                assert_eq!(style.head_arrow.as_ref().unwrap().arrow_type, ArrowType::Normal);
                if len != 17 {
                    assert!(style.fill.is_none());
                }
            }
        }
    }
}

/// 이것을 실패시키는 것: 내부 끝에서 외부 범위를 닫기 / 필드 ID 재사용 /
/// 한 문단 필드까지 경계 표현으로 바꾸기 / 미종료 fallback 누락 / 고아 끝 panic.
/// 실제 변이 M06–M09·M13이 이 시험에서 실패한다.
#[test]
fn fspan_pairing_keeps_siblings_nested_markers_and_unclosed_fallback() {
    use hwpforge_core::field_inventory::FieldInventory;
    let marker = |code| r4_marker(code, CTRL_ID_CLICK_HERE);
    let control = make_record(TagId::CtrlHeader, 1, &r1_clickhere());
    let mut first = marker(4); // unmatched end must not consume a later start
    first.extend(marker(3));
    first.extend(para_text_data("외부시작"));
    first.extend(marker(3));
    first.extend(para_text_data("내부값"));
    first.extend(marker(4));
    let mut middle = para_text_data("외부끝");
    middle.extend(marker(4));
    middle.extend(marker(3));
    middle.extend(para_text_data("짧은값"));
    middle.extend(marker(4));
    middle.extend(para_text_data("범위밖"));
    middle.extend(marker(3));
    middle.extend(para_text_data("다음시작"));
    let mut last = para_text_data("다음끝");
    last.extend(marker(4));
    last.extend(marker(3));
    last.extend(para_text_data("미종료값"));
    let mut wire = r4_para(&first, &control.repeat(2), false);
    wire.extend(r4_para(&middle, &control.repeat(2), false));
    wire.extend(r4_para(&last, &control, false));
    wire.extend(r1_paragraph("미종료뒤문단"));
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, _) = crate::projection::project_to_core(vec![parsed]).unwrap();
    let fields = FieldInventory::from_sections(doc.sections());
    assert_eq!(
        fields.fields().iter().map(|f| f.current()).collect::<Vec<_>>(),
        ["외부시작내부값외부끝", "내부값", "짧은값", "다음시작다음끝", "미종료값"]
    );
    // A same-paragraph inner field and the existing unclosed fallback stay folded.
    assert_eq!(fields.fields().iter().filter(|f| f.id().is_none()).count(), 3);
    let bytes = r3_encode(doc, &hwpforge_core::ImageStore::new()).bytes;
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&bytes).unwrap();
    let fields = FieldInventory::from_sections(decoded.document.sections());
    assert_eq!(
        fields.fields().iter().map(|f| f.current()).collect::<Vec<_>>(),
        ["외부시작내부값외부끝", "내부값", "짧은값", "다음시작다음끝", "미종료값"]
    );
}

/// 이것을 실패시키는 것: M10·M15 — 접힌 외부 필드 안의 경계만 승격하거나 다른 종류를 누름틀로 오인.
#[test]
fn fspan_folded_outer_keeps_nested_cross_paragraph_fallback() {
    use crate::ctrl_ids::CTRL_ID_HYPERLINK;
    let mut first = r4_marker(3, CTRL_ID_HYPERLINK);
    first.extend(r4_marker(3, CTRL_ID_CLICK_HERE));
    first.extend(para_text_data("가상앞"));
    let mut last = para_text_data("가상뒤");
    last.extend(r4_marker(4, 0));
    last.extend(r4_marker(4, 0));
    let mut controls = r4_command(CTRL_ID_HYPERLINK, "https\\://example.org/");
    controls.extend(make_record(TagId::CtrlHeader, 1, &r1_clickhere()));
    let mut wire = r4_para(&first, &controls, false);
    wire.extend(r4_para(&last, &[], false));
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, _) = crate::projection::project_to_core(vec![parsed]).unwrap();
    let mut boundaries = 0;
    doc.for_each_paragraph(|p| {
        boundaries += p
            .runs
            .iter()
            .filter(|r| {
                matches!(r.content.as_control(), Some(hwpforge_core::Control::FieldBoundary { .. }))
            })
            .count();
    });
    assert_eq!(boundaries, 0);
}

/// 이것을 실패시키는 것: M14 — 빈 시작 문단에 접힌 필드처럼 안내문을 본문으로 합성.
#[test]
fn fspan_empty_start_does_not_invent_hint_text() {
    use hwpforge_core::field_inventory::FieldInventory;
    let control = make_record(TagId::CtrlHeader, 1, &r1_clickhere_with_direction("가상 빈칸 안내"));
    let mut wire = r4_para(&r4_marker(3, CTRL_ID_CLICK_HERE), &control, false);
    let mut last = para_text_data("가상 입력값");
    last.extend(r4_marker(4, 0));
    wire.extend(r4_para(&last, &[], false));
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, _) = crate::projection::project_to_core(vec![parsed]).unwrap();
    for doc in [
        doc.clone(),
        hwpforge_smithy_hwpx::HwpxDecoder::decode(
            &r3_encode(doc, &hwpforge_core::ImageStore::new()).bytes,
        )
        .unwrap()
        .document,
    ] {
        let inventory = FieldInventory::from_sections(doc.sections());
        assert_eq!(inventory.fields()[0].current(), "가상 입력값");
    }
}

/// 이것을 실패시키는 것: 부모가 경계여도 중첩 여러 문단 누름틀을 승격에서 제외,
/// FIFO 짝짓기, ID 중복, 끝 경계 방출 누락, 끝 문단 flat 처리.
#[test]
fn fspan_nested_cross_paragraph_clicks_keep_both_ranges() {
    use hwpforge_core::field_inventory::FieldInventory;
    let mut first = r4_marker(3, CTRL_ID_CLICK_HERE);
    first.extend(r4_marker(3, CTRL_ID_CLICK_HERE));
    first.extend(para_text_data("가"));
    let mut last = para_text_data("나");
    last.extend(r4_marker(4, 0));
    last.extend(para_text_data("다"));
    last.extend(r4_marker(4, 0));
    let control = make_record(TagId::CtrlHeader, 1, &r1_clickhere());
    let mut wire = r4_para(&first, &control.repeat(2), false);
    wire.extend(r4_para(&last, &[], false));
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
    for doc in [
        doc.clone(),
        hwpforge_smithy_hwpx::HwpxDecoder::decode(
            &r3_encode(doc, &hwpforge_core::ImageStore::new()).bytes,
        )
        .unwrap()
        .document,
    ] {
        let inventory = FieldInventory::from_sections(doc.sections());
        assert_eq!(
            inventory.fields().iter().map(|f| f.current()).collect::<Vec<_>>(),
            ["가나다", "가나"]
        );
        assert_eq!(inventory.fields()[1].end().as_deref(), Some("paragraphs[1].runs[1]"));
        assert!(inventory.diagnostics().is_empty());
    }
    assert!(!warnings.iter().any(|w| matches!(
        w,
        Hwp5Warning::ProjectionFallback { subject: "field.unclosed" | "field.nested", .. }
    )));
}

/// 이것을 실패시키는 것: 미종료 경고 누락, 미소비 메타데이터 경고 누락,
/// 고아 끝에서 뒤의 시작을 잘못 짝짓기, 메타데이터 없는 시작을 승격.
#[test]
fn fspan_unmatched_markers_and_metadata_keep_fallbacks() {
    use hwpforge_core::field_inventory::FieldInventory;
    let click = make_record(TagId::CtrlHeader, 1, &r1_clickhere());
    let mut first = r4_marker(4, 0);
    first.extend(para_text_data("앞"));
    first.extend(r4_marker(3, CTRL_ID_CLICK_HERE));
    first.extend(para_text_data("미종료"));
    let wire = r4_para(&first, &click.repeat(2), true);
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
    assert_eq!(FieldInventory::from_sections(doc.sections()).fields()[0].current(), "미종료");
    assert_eq!(
        warnings
            .iter()
            .filter(|w| matches!(
                w,
                Hwp5Warning::ProjectionFallback { subject: "field.unclosed", .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|w| matches!(w, Hwp5Warning::DroppedControl { control: "clickhere", .. }))
            .count(),
        1
    );
    assert!(warnings.iter().any(|w| matches!(w, Hwp5Warning::LayoutCacheDropped { .. })));

    let mut first = r4_marker(3, CTRL_ID_CLICK_HERE);
    first.extend(para_text_data("무메타"));
    let mut last = para_text_data("후속");
    last.extend(r4_marker(4, 0));
    let mut wire = r4_para(&first, &[], false);
    wire.extend(r4_para(&last, &[], false));
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
    assert!(FieldInventory::from_sections(doc.sections()).fields().is_empty());
    assert!(warnings
        .iter()
        .any(|w| matches!(w, Hwp5Warning::ProjectionFallback { subject: "field.clickhere", .. })));
}

/// 이것을 실패시키는 것: 이전 문단의 접힌 부모를 현재 문단의 accumulator로 취급,
/// 이전 문단에서 이미 닫은 fallback을 실제 끝에서 다시 종료.
#[test]
fn fspan_folded_parent_from_previous_paragraph_has_no_local_accumulator() {
    use crate::ctrl_ids::CTRL_ID_HYPERLINK;
    use hwpforge_core::field_inventory::FieldInventory;
    let mut first = r4_marker(3, CTRL_ID_HYPERLINK);
    first.extend(para_text_data("앞"));
    let mut wire = r4_para(&first, &r4_command(CTRL_ID_HYPERLINK, "https\\://example.org/"), false);
    let mut middle = r4_marker(3, CTRL_ID_CLICK_HERE);
    middle.extend(para_text_data("가"));
    let click = make_record(TagId::CtrlHeader, 1, &r1_clickhere());
    wire.extend(r4_para(&middle, &click, false));
    let mut last = para_text_data("나");
    last.extend(r4_marker(4, 0));
    last.extend(para_text_data("뒤"));
    last.extend(r4_marker(4, 0));
    wire.extend(r4_para(&last, &[], false));
    let parsed = parse_body_text(&wire, &version()).unwrap();
    let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
    assert_eq!(
        warnings
            .iter()
            .filter(|w| matches!(
                w,
                Hwp5Warning::ProjectionFallback { subject: "field.unclosed", .. }
            ))
            .count(),
        2
    );
    assert!(!warnings
        .iter()
        .any(|w| matches!(w, Hwp5Warning::ProjectionFallback { subject: "field.nested", .. })));
    for doc in [
        doc.clone(),
        hwpforge_smithy_hwpx::HwpxDecoder::decode(
            &r3_encode(doc, &hwpforge_core::ImageStore::new()).bytes,
        )
        .unwrap()
        .document,
    ] {
        let inv = FieldInventory::from_sections(doc.sections());
        assert_eq!(inv.fields().iter().map(|f| f.current()).collect::<Vec<_>>(), ["앞", "가"]);
        assert!(inv.diagnostics().is_empty());
    }
}
