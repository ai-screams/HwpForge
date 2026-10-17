// Review r1: the same wire controls must mean the same thing in every host.
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

fn r1_fixture_control(file: &str, id: u32) -> Vec<u8> {
    let bytes = std::fs::read(fixture_path(file)).unwrap();
    let package = PackageReader::open(&bytes).unwrap();
    let records = Record::parse_stream(&mut Cursor::new(&package.sections_data()[0])).unwrap();
    let start = records
        .iter()
        .position(|r| {
            TagId::from(r.header.tag_id) == TagId::CtrlHeader && parse_ctrl_id(&r.data) == id
        })
        .unwrap();
    let base = records[start].header.level;
    let mut bytes = Vec::new();
    for (i, record) in records[start..].iter().enumerate() {
        if i > 0 && record.header.level <= base {
            break;
        }
        bytes.extend(make_record(
            TagId::from(record.header.tag_id),
            record.header.level - base + 1,
            &record.data,
        ));
    }
    bytes
}

/// 이것을 실패시키는 것: 중첩 해석기에서 eqed/tbl/fn/en/head/foot/gso 중 하나를
/// Unknown으로 돌리거나, 자식 레코드를 부모 문단으로 보내기(각 경로별 변이).
#[test]
fn r1_subtree_controls_use_the_body_interpreter_in_every_host() {
    let mut payloads = Vec::new();
    let mut equation = r1_paragraph("equation host");
    equation.extend(r1_fixture_control("sample-equation-native.hwp", CTRL_ID_EQED));
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
    let command = "Clickhere:set:0:Direction:wstring:1:힌 HelpState:wstring:0:  ";
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
fn r1_group_memo_bodies_join_only_after_the_section_is_complete() {
    let memo_wire = r1_fixture_control("sample-memo-basic.hwp", CTRL_ID_MEMO);
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
fn r1_memo_list_join_cannot_bypass_the_container_budget() {
    let mut bytes = r1_paragraph("anchor");
    bytes.extend(r1_fixture_control("sample-memo-basic.hwp", CTRL_ID_MEMO));
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
        root.extend(r1_fixture_control("sample-memo-basic.hwp", CTRL_ID_MEMO));
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

/// 이것을 실패시키는 것: 단발성 제어를 공통 해석기 안에서 중첩일 때만 Unknown으로
/// 바꾸기. 묶음·글상자·도형·캡션을 포함한 모든 호스트에서 본문 결과와 대조한다.
#[test]
fn r1_one_shot_controls_also_share_all_hosts() {
    for (file, id) in [
        ("sample-compose-basic.hwp", CTRL_ID_COMPOSE),
        ("sample-dutmal-basic.hwp", CTRL_ID_DUTMAL),
        ("sample-indexmark-basic.hwp", CTRL_ID_INDEXMARK),
        ("sample-field-docsummary.hwp", CTRL_ID_FIELD_SUMMERY),
        ("sample-bookmark-page.hwp", CTRL_ID_FIELD_CROSSREF),
    ] {
        let mut payload = r1_paragraph("host");
        payload.extend(r1_fixture_control(file, id));
        let body = parse_body_text(&payload, &version()).unwrap();
        assert!(!matches!(body.paragraphs[0].controls[0], Hwp5Control::Unknown { .. }));
        for path in r1_paths() {
            let nested = parse_body_text(&r1_in_path(&payload, &path), &version()).unwrap();
            assert_eq!(
                r3_semantic_snapshot(&r1_at_path(&nested.paragraphs, &path)),
                r3_semantic_snapshot(&body.paragraphs),
                "{file} {path:x?}"
            );
        }
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
            wire.extend(r1_fixture_control("sample-memo-basic.hwp", CTRL_ID_MEMO));
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let Hwp5Control::Memo(m) = &parsed.paragraphs[0].controls[0] else { panic!("memo") };
            wire.extend(make_record(TagId::MemoList, 1, &m.command.memo_id.to_le_bytes()));
            wire.extend(make_record(TagId::ListHeader, 1, &1u32.to_le_bytes()));
            r1_shift(&mut wire, &inner, 1);
            wire
        } else {
            let mut wire = r1_wrap(kind, &inner);
            if kind == R1_CAPTION {
                // A supported caption needs a real cell; r1's parser-only
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

/// 이것을 실패시키는 것: head/foot 공통 투영 제거, 특정 호스트의 문단 투영 생략,
/// 머리말 anchor 초기화, 재귀 전에 슬롯을 예약하지 않아 중첩 머리말 순서 뒤집기.
/// 파서 비교뿐 아니라 실제 Core 본문과 HWPX 출력의 개수/본문/앵커를 검사한다.
#[test]
fn r2_representation_by_path_reaches_core_and_hwpx() {
    let mut payloads = Vec::new();
    for (kind, tag) in [
        (CTRL_ID_TABLE, "tbl"),
        (CTRL_ID_HEADER, "header"),
        (CTRL_ID_FOOTER, "footer"),
        (CTRL_ID_FOOTNOTE, "footNote"),
        (CTRL_ID_ENDNOTE, "endNote"),
        (CTRL_ID_GSO, "rect"),
        (R1_GROUP, "container"),
    ] {
        payloads.push((tag, r1_wrap(kind, &r1_paragraph("R2보존본문")), true));
    }
    for (file, id, tag) in [
        ("sample-equation-native.hwp", CTRL_ID_EQED, "equation"),
        ("sample-compose-basic.hwp", CTRL_ID_COMPOSE, "compose"),
        ("sample-dutmal-basic.hwp", CTRL_ID_DUTMAL, "dutmal"),
        ("sample-indexmark-basic.hwp", CTRL_ID_INDEXMARK, "indexmark"),
    ] {
        let mut payload = r1_paragraph("R2표현앵커");
        payload.extend(r1_fixture_control(file, id));
        payloads.push((tag, payload, false));
    }
    for (tag, payload, has_body) in payloads {
        for path in r2_paths() {
            let mut wire = r1_paragraph("앞 문단");
            wire.extend(r2_in_path(&payload, &path));
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let expected = parse_body_text(&payload, &version()).unwrap();
            assert_eq!(
                r3_semantic_snapshot(&r1_at_path(&parsed.paragraphs[1..], &path)),
                r3_semantic_snapshot(&expected.paragraphs)
            );
            let next = parse_body_text(&r1_paragraph("next section"), &version()).unwrap();
            let (doc, warnings) = crate::projection::project_to_core(vec![parsed, next]).unwrap();
            assert!(doc.sections()[1].headers.is_empty());
            assert!(doc.sections()[1].footers.is_empty());
            let core = serde_json::to_string(&doc).unwrap();
            let marker = if has_body { "R2보존본문" } else { "R2표현앵커" };
            assert_eq!(core.matches(marker).count(), 1, "Core {tag} {path:x?}: {warnings:?}");
            for unit in doc.sections()[0].headers.iter().chain(&doc.sections()[0].footers) {
                assert_eq!(unit.anchor_paragraph, Some(1), "{tag} {path:x?}");
            }
            for (kind, id, units) in [
                ("header", CTRL_ID_HEADER, &doc.sections()[0].headers),
                ("footer", CTRL_ID_FOOTER, &doc.sections()[0].footers),
            ] {
                if tag == kind && path == [id] {
                    assert!(serde_json::to_string(&units[0]).unwrap().contains("outer"));
                    assert!(serde_json::to_string(&units[1]).unwrap().contains(marker));
                }
            }
            let output = r2_xml(doc);
            assert_eq!(output.matches(marker).count(), 1, "XML {tag} {path:x?}");
            let host_tag = |kind: &u32| match *kind {
                CTRL_ID_TABLE | R1_CAPTION => "tbl",
                CTRL_ID_HEADER => "header",
                CTRL_ID_FOOTER => "footer",
                CTRL_ID_FOOTNOTE => "footNote",
                CTRL_ID_ENDNOTE => "endNote",
                CTRL_ID_GSO => "rect",
                R1_GROUP | R1_NESTED_GROUP => "container",
                _ => "",
            };
            let extra = path
                .iter()
                .filter(|k| host_tag(k) == tag)
                .map(|k| if *k == R1_NESTED_GROUP { 2 } else { 1 })
                .sum::<usize>();
            // A group's rectangle child also contributes a rect element.
            let extra_rect = if tag == "rect" {
                path.iter().filter(|k| matches!(**k, R1_GROUP | R1_NESTED_GROUP)).count()
            } else {
                0
            };
            assert_eq!(
                output.matches(&format!("<hp:{tag} ")).count()
                    + output.matches(&format!("<hp:{tag}>")).count(),
                1 + extra + extra_rect,
                "{tag} {path:x?}: {output}"
            );
        }
    }
}

fn r2_field_payloads() -> Vec<(&'static str, u32, Vec<u8>)> {
    let mut fields = vec![
        (
            "summary",
            CTRL_ID_FIELD_SUMMERY,
            r1_fixture_control("sample-field-docsummary.hwp", CTRL_ID_FIELD_SUMMERY),
        ),
        (
            "crossref",
            CTRL_ID_FIELD_CROSSREF,
            r1_fixture_control("sample-bookmark-page.hwp", CTRL_ID_FIELD_CROSSREF),
        ),
        ("clickhere", CTRL_ID_CLICK_HERE, make_record(TagId::CtrlHeader, 1, &r1_clickhere())),
    ];
    for (kind, id, command) in
        [("datecode", CTRL_ID_FIELD_DATE_CODE, "T\\:;0;"), ("path", CTRL_ID_FIELD_PATH, "$P$F")]
    {
        let mut data = id.to_le_bytes().to_vec();
        data.extend([0; 4]);
        data.push(8);
        data.extend((command.encode_utf16().count() as u16).to_le_bytes());
        data.extend(para_text_data(command));
        data.extend([0; 8]);
        fields.push((kind, id, make_record(TagId::CtrlHeader, 1, &data)));
    }
    fields
}

/// 이것을 실패시키는 것: 타입화 필드의 공통 구조 투영 또는 중첩 HWPX 방출 제거.
#[test]
fn r2_paired_fields_reach_core_and_hwpx_in_every_host() {
    for (kind, id, control) in r2_field_payloads() {
        let mut text = 3u16.to_le_bytes().to_vec();
        text.extend(id.to_le_bytes());
        text.extend([0; 10]);
        text.extend(para_text_data("R2필드본문"));
        text.extend(4u16.to_le_bytes());
        text.extend([0; 14]);
        let mut payload = make_record(TagId::ParaHeader, 0, &para_header_data(0, 0));
        payload.extend(make_record(TagId::ParaText, 1, &text));
        payload.extend(control);
        for path in r2_paths() {
            let parsed = parse_body_text(&r2_in_path(&payload, &path), &version()).unwrap();
            let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
            assert!(
                !warnings.iter().any(
                    |w| matches!(w, Hwp5Warning::DroppedControl { control, .. } if *control == kind)
                ),
                "{kind} {path:x?}: {warnings:?}"
            );
            let core = serde_json::to_string(&doc).unwrap();
            // Filled ClickHere must render user content; hints only fill empty spans.
            let marker = "R2필드본문";
            assert!(core.contains(marker), "{kind} {path:x?}: {core}");
            let xml = r2_xml(doc);
            assert!(xml.contains(marker), "{kind} {path:x?}: {xml}");
            let field_type = match kind {
                "clickhere" => "CLICK_HERE",
                "crossref" => "CROSSREF",
                "path" => "PATH",
                _ => "SUMMERY",
            };
            assert_eq!(
                xml.matches(&format!("type=\"{field_type}\"")).count(),
                1,
                "{kind} {path:x?}"
            );
        }
    }
}

/// 이것을 실패시키는 것: inline anchor가 없는 타입화 필드의 잔여 큐 경고 삭제.
#[test]
fn r2_unpaired_typed_fields_warn_in_every_host() {
    for (kind, _id, control) in r2_field_payloads().into_iter().chain([(
        "memo",
        CTRL_ID_MEMO,
        r1_fixture_control("sample-memo-basic.hwp", CTRL_ID_MEMO),
    )]) {
        let mut payload = r1_paragraph("unpaired");
        payload.extend(control);
        for path in r2_paths() {
            let parsed = parse_body_text(&r2_in_path(&payload, &path), &version()).unwrap();
            let (_, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
            assert_eq!(warnings.iter().filter(|w| matches!(w, Hwp5Warning::DroppedControl { control, reason } if *control == kind && reason.contains("no matching inline anchor"))).count(), 1, "{kind} {path:x?}: {warnings:?}");
        }
    }
}

/// 이것을 실패시키는 것: 메모 연결 한도에서 반환만 하고 이미 붙인 비메모 subtree 유지,
/// 연결 위치 예산 초기화, retain 결과 무시, 마지막 유효 깊이까지 잘라 버리기.
#[test]
fn r2_memo_join_trims_preparsed_container_bodies() {
    let mut anchor = r1_paragraph("memo anchor");
    anchor.extend(r1_fixture_control("sample-memo-basic.hwp", CTRL_ID_MEMO));
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
    cfb.create_stream("/DocInfo").unwrap();
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

/// 이것을 실패시키는 것: 마지막 깊이에서 leaf까지 제거, 묶음 자식 텍스트 예산
/// 차감/절단 생략, 묶음 자식 control의 retain 결과 무시.
#[test]
fn r2_memo_budget_covers_group_text_children_and_keeps_leaf_controls() {
    let mut anchor = r1_paragraph("memo anchor");
    anchor.extend(r1_fixture_control("sample-memo-basic.hwp", CTRL_ID_MEMO));
    let prototype = parse_body_text(&anchor, &version()).unwrap();
    let Hwp5Control::Memo(memo) = &prototype.paragraphs[0].controls[0] else { panic!("memo") };
    for (kind, cost) in [(R1_GROUP, 2), (R1_NESTED_GROUP, 3)] {
        for remaining in [1, 2, 3] {
            let host_depth = 31 - remaining;
            let mut wire = r1_in_path(&anchor, &vec![CTRL_ID_FOOTNOTE; host_depth]);
            wire.extend(make_record(TagId::MemoList, 1, &memo.command.memo_id.to_le_bytes()));
            wire.extend(make_record(TagId::ListHeader, 1, &1u32.to_le_bytes()));
            r1_shift(&mut wire, &r1_wrap(kind, &r1_paragraph("R2묶음끝")), 1);
            let parsed = parse_body_text(&wire, &version()).unwrap();
            assert_eq!(
                format!("{:?}", parsed.paragraphs).contains("R2묶음끝"),
                remaining >= cost,
                "{kind:x} remaining={remaining}"
            );
            assert_eq!(
                parsed.warnings.iter().any(|w| matches!(
                    w,
                    Hwp5Warning::DroppedControl { control: "memo_depth", .. }
                )),
                remaining < cost
            );
            let body = r1_at_path(&parsed.paragraphs, &vec![CTRL_ID_FOOTNOTE; host_depth]);
            let Hwp5Control::Memo(m) = &body[0].controls[0] else { panic!("memo") };
            let Hwp5Control::Group(g) = &m.paragraphs[0].controls[0] else { panic!("group") };
            if kind == R1_NESTED_GROUP && remaining == 1 {
                assert!(g.children.is_empty(), "over-budget child control must be removed");
            }
        }
    }
    let mut leaf = r1_paragraph("leaf at cap");
    leaf.extend(r1_fixture_control("sample-compose-basic.hwp", CTRL_ID_COMPOSE));
    let parsed = parse_body_text(&r1_in_path(&leaf, &[CTRL_ID_FOOTNOTE; 32]), &version()).unwrap();
    assert!(matches!(
        r1_at_path(&parsed.paragraphs, &[CTRL_ID_FOOTNOTE; 32])[0].controls[0],
        Hwp5Control::Compose(_)
    ));
    assert!(!parsed
        .warnings
        .iter()
        .any(|w| matches!(w, Hwp5Warning::DroppedControl { control: "memo_depth", .. })));
}

fn r3_asset_wire(id: u16) -> Vec<u8> {
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
        wire.extend(make_record(TagId::ShapeComponent, 2, &[0; 4]));
        wire.extend(make_record(tag, 2, &data));
    }
    wire
}

fn r3_chart_asset(id: u16) -> (String, Vec<u8>, Vec<u8>) {
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
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
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

/// 이것을 실패시키는 것: 캡션/셀 투영 순서로 머리말·꼬리말 방출, 셀 정렬 순서
/// 사용, 중첩 parser/묶음/메모에서 wire 순번 초기화, 부모보다 자식 먼저 방출.
#[test]
fn r3_header_footer_sibling_hosts_keep_wire_order() {
    for kind in [CTRL_ID_HEADER, CTRL_ID_FOOTER] {
        let host = |name: &str, nested: bool| {
            let mut inner = r1_paragraph(name);
            if nested {
                inner.extend(r1_wrap(kind, &r1_paragraph(&format!("{name}child"))));
            }
            r1_wrap(kind, &inner)
        };
        let mut wire = r1_wrap(R1_CAPTION, &host("R3caption", true));
        // Source cell order intentionally differs from column order.
        for (column, name) in [(1u16, "R3cellB"), (0, "R3cellA")] {
            let mut table = Vec::new();
            one_cell_table(&mut table, 1);
            let mut list = Record::parse_stream(&mut Cursor::new(table)).unwrap().pop().unwrap();
            list.data[8..10].copy_from_slice(&column.to_le_bytes());
            wire.extend(make_record(TagId::ListHeader, 2, &list.data));
            r1_shift(&mut wire, &r2_in_path(&host(name, true), &[R1_NESTED_GROUP, CTRL_ID_GSO]), 2);
        }
        // Correct the table's declared width to include both cells.
        let records = Record::parse_stream(&mut Cursor::new(wire)).unwrap();
        let mut wire = Vec::new();
        for mut r in records {
            if TagId::from(r.header.tag_id) == TagId::Table {
                r.data[6..8].copy_from_slice(&2u16.to_le_bytes());
            }
            wire.extend(make_record(TagId::from(r.header.tag_id), r.header.level, &r.data));
        }
        wire.extend(r2_in_path(&host("R3memo", true), &[CTRL_ID_MEMO]));
        let parsed = parse_body_text(&wire, &version()).unwrap();
        let Hwp5Control::Table(table) = &parsed.paragraphs[0].controls[0] else { panic!("table") };
        assert_eq!(table.cells.iter().map(|c| c.column).collect::<Vec<_>>(), [1, 0]);
        let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
        r4_assert_projection_warnings(&warnings, &[]);
        let units = if kind == CTRL_ID_HEADER {
            &doc.sections()[0].headers
        } else {
            &doc.sections()[0].footers
        };
        let expected = [
            "R3caption",
            "R3captionchild",
            "R3cellB",
            "R3cellBchild",
            "R3cellA",
            "R3cellAchild",
            "R3memo",
            "R3memochild",
        ];
        let actual: Vec<_> = units
            .iter()
            .map(|h| {
                h.paragraphs
                    .iter()
                    .take(1)
                    .flat_map(|p| &p.runs)
                    .filter_map(|r| r.content.as_text())
                    .collect::<String>()
            })
            .collect();
        assert_eq!(actual, expected);
        let xml = r2_xml(doc);
        let mut previous = 0;
        for name in expected {
            let position = xml.find(&format!("{name}</hp:t>")).unwrap();
            assert!(position > previous);
            previous = position;
        }
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

fn r4_memo(id: u32, body: &str) -> (Vec<u8>, Vec<u8>) {
    let header = r4_command(CTRL_ID_MEMO, &format!("MEMO/65535/{id}/123/456/author/\\;;"));
    let mut cluster = make_record(TagId::MemoList, 1, &id.to_le_bytes());
    cluster.extend(make_record(TagId::ListHeader, 1, &1u32.to_le_bytes()));
    r1_shift(&mut cluster, &r1_paragraph(body), 1);
    (header, cluster)
}

/// 이것을 실패시키는 것: 본문 전용/FIFO 책갈피 힌트 복귀, CtrlData 소유 깊이 무시,
/// 점 위치를 문단 끝으로 이동, span 이름 누락, 하이퍼링크·MEMO 중첩 호스트 누락.
#[test]
fn r4_bookmark_hyperlink_memo_matrix_uses_source_names_and_ranges() {
    for span in [false, true] {
        let mut payload = Vec::new();
        // References deliberately reverse the order of bookmark definitions.
        // One target is never referenced, so no ref-name FIFO can pass.
        for name in ["targetB", "targetA", "unreferenced"] {
            let (id, control) = r4_bookmark(name, span);
            let mut text = para_text_data("before");
            text.extend(r4_marker(if span { 3 } else { 0x16 }, id));
            text.extend(para_text_data(name));
            if span {
                text.extend(r4_marker(4, 0));
            }
            text.extend(para_text_data("after"));
            payload.extend(r4_para(&text, &control, false));
        }
        for name in ["targetA", "targetB"] {
            let id = CTRL_ID_FIELD_CROSSREF;
            let mut text = r4_marker(3, id);
            text.extend(para_text_data(&format!("ref-{name}")));
            text.extend(r4_marker(4, 0));
            payload.extend(r4_para(&text, &r4_command(id, &format!("?{name};6;0;0;0;")), false));
        }
        let id = crate::ctrl_ids::CTRL_ID_HYPERLINK;
        let mut text = r4_marker(3, id);
        text.extend(para_text_data("R4link"));
        text.extend(r4_marker(4, 0));
        payload.extend(r4_para(&text, &r4_command(id, "https\\://example.org/"), false));
        let (memo, cluster) = r4_memo(77, "R4memo-body");
        let mut text = r4_marker(3, crate::ctrl_ids::CTRL_ID_MEMO_INLINE);
        text.extend(para_text_data("R4memo-anchor"));
        text.extend(r4_marker(4, 0));
        payload.extend(r4_para(&text, &memo, false));
        for path in r2_paths() {
            let mut wire = r2_in_path(&payload, &path);
            wire.extend(&cluster);
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
            r4_assert_projection_warnings(&warnings, &[]);
            let core = serde_json::to_string(&doc).unwrap();
            for name in ["targetA", "targetB", "unreferenced"] {
                assert!(core.contains(name));
            }
            let xml = r2_xml(doc);
            for name in ["targetA", "targetB", "unreferenced"] {
                assert_eq!(xml.matches(&format!("name=\"{name}\"")).count(), 1, "{path:x?}: {xml}");
            }
            let first_mark = xml.find("name=\"targetB\"").unwrap();
            assert!(xml[..first_mark].contains("before"));
            assert!(xml[first_mark..].contains("after"));
            if !span {
                assert!(
                    first_mark < xml.find("<hp:t>targetBafter</hp:t>").unwrap(),
                    "point range {path:x?}"
                );
            }
            for token in [
                "type=\"CROSSREF\"",
                "type=\"HYPERLINK\"",
                "https://example.org/",
                "R4memo-body",
                "R4memo-anchor",
            ] {
                assert!(xml.contains(token), "{path:x?} missing {token}: {xml}");
            }
        }
    }
}

/// 이것을 실패시키는 것: active_field 덮어쓰기, 내부 끝에서 외부 종료, 의미 손실
/// 경고를 ledger에만 기록, 내부 메모 본문 폐기, 필드 안 객체 큐 지연/폐기.
#[test]
fn r4_compound_fields_preserve_outer_and_all_memos_with_and_without_cache() {
    for cache in [false, true] {
        for inner_memo in [false, true] {
            for object in [false, true] {
                let (outer, outer_cluster) = r4_memo(77, "R4outer-memo");
                let (inner, inner_cluster) = r4_memo(78, "R4inner-memo");
                let path_control =
                    r2_field_payloads().into_iter().find(|(k, _, _)| *k == "path").unwrap().2;
                let mut controls = outer;
                controls.extend(if inner_memo { inner } else { path_control });
                let mut text = r4_marker(3, crate::ctrl_ids::CTRL_ID_MEMO_INLINE);
                text.extend(para_text_data("R4앞부분"));
                text.extend(r4_marker(
                    3,
                    if inner_memo {
                        crate::ctrl_ids::CTRL_ID_MEMO_INLINE
                    } else {
                        CTRL_ID_FIELD_PATH
                    },
                ));
                text.extend(para_text_data("R4경로"));
                text.extend(r4_marker(4, 0));
                text.extend(para_text_data("R4뒷부분"));
                if object {
                    text.extend(r4_marker(0x0b, CTRL_ID_GSO));
                    let wrapped = r1_wrap(CTRL_ID_GSO, &r1_paragraph("R4object-body"));
                    let records = Record::parse_stream(&mut Cursor::new(wrapped)).unwrap();
                    for r in records.into_iter().skip(2) {
                        controls.extend(make_record(
                            TagId::from(r.header.tag_id),
                            r.header.level,
                            &r.data,
                        ));
                    }
                }
                text.extend(r4_marker(4, 0));
                text.extend(para_text_data("R4outside"));
                let payload = r4_para(&text, &controls, cache);
                for path in r2_paths() {
                    let mut wire = r2_in_path(&payload, &path);
                    wire.extend(&outer_cluster);
                    if inner_memo {
                        wire.extend(&inner_cluster);
                    }
                    let parsed = parse_body_text(&wire, &version()).unwrap();
                    let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
                    let expected = if object {
                        vec!["field.nested", "field.object"]
                    } else {
                        vec!["field.nested"]
                    };
                    r4_assert_projection_warnings(&warnings, &expected);
                    assert_eq!(
                        warnings
                            .iter()
                            .filter(|w| matches!(
                                w,
                                Hwp5Warning::ProjectionFallback { subject: "field.nested", .. }
                            ))
                            .count(),
                        1
                    );
                    let core = serde_json::to_string(&doc).unwrap();
                    assert!(core.contains("R4앞부분R4경로R4뒷부분"), "{path:x?}: {core}");
                    let xml = r2_xml(doc);
                    for token in ["R4앞부분R4경로R4뒷부분", "R4outer-memo", "R4outside"] {
                        assert_eq!(xml.matches(token).count(), 1, "{path:x?}: {xml}");
                    }
                    if inner_memo {
                        assert_eq!(xml.matches("R4inner-memo").count(), 1);
                    }
                    if object {
                        assert_eq!(xml.matches("R4object-body").count(), 1);
                    }
                }
            }
        }
    }
}

fn r4_note(kind: u32, text: &str) -> Vec<u8> {
    let wrapped = r1_wrap(kind, &r1_paragraph(text));
    let records = Record::parse_stream(&mut Cursor::new(wrapped)).unwrap();
    let mut control = Vec::new();
    for r in records.into_iter().skip(2) {
        control.extend(make_record(TagId::from(r.header.tag_id), r.header.level, &r.data));
    }
    r4_para(&r4_marker(0x11, kind), &control, false)
}

// Read each note's own first autoNum, independent of nested note sublists.
fn r4_assert_note_numbers(xml: &str, tag: &str, kind: &str, b: usize) {
    let mut stack = Vec::new();
    let mut facts: Vec<(String, String, Option<String>, String)> = Vec::new();
    for part in xml.split('<').skip(1) {
        let (head, text) = part.split_once('>').unwrap();
        if head.starts_with(&format!("hp:{tag} ")) {
            let number = r3_refs(head, "number").into_iter().next().expect("note number");
            let suffix = r3_refs(head, "suffixChar").into_iter().next().expect("note suffixChar");
            stack.push(facts.len());
            facts.push((number, suffix, None, String::new()));
        } else if head == format!("/hp:{tag}") {
            stack.pop().unwrap();
        } else if let Some(&i) = stack.last() {
            if head.starts_with("hp:autoNum ") && head.contains(&format!("numType=\"{kind}\"")) {
                assert!(facts[i].2.is_none(), "duplicate note head: {xml}");
                facts[i].2 = r3_refs(head, "num").into_iter().next();
            }
            if head.starts_with("hp:t") {
                facts[i].3.push_str(text);
            }
        }
    }
    for (i, (number, suffix, auto, _)) in facts.iter().enumerate() {
        assert_eq!(number, &(i + 1).to_string(), "{xml}");
        assert_eq!(suffix, "41");
        assert_eq!(auto.as_deref(), Some(number.as_str()), "{xml}");
    }
    for (text, number) in [("R4noteA", 1), ("R4noteB", b), ("R4noteC", b + 1)] {
        assert_eq!(
            facts.iter().find(|f| f.3.contains(text)).unwrap().0,
            number.to_string(),
            "{xml}"
        );
    }
}

/// 이것을 실패시키는 것: 어느 호스트든 번호 계획 방문 생략, 구역/중첩 카운터 리셋,
/// number/suffixChar/autoNum 누락 또는 B 뒤 본문 C 번호 미증가.
#[test]
fn r4_note_matrix_numbers_all_hosts_before_serialization() {
    for (kind, tag, num_type) in
        [(CTRL_ID_FOOTNOTE, "footNote", "FOOTNOTE"), (CTRL_ID_ENDNOTE, "endNote", "ENDNOTE")]
    {
        for path in r2_paths() {
            let mut wire = r4_note(kind, "R4noteA");
            wire.extend(r2_in_path(&r4_note(kind, "R4noteB"), &path));
            wire.extend(r4_note(kind, "R4noteC"));
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
            r4_assert_projection_warnings(&warnings, &[]);
            let xml = r2_xml(doc);
            r4_assert_note_numbers(
                &xml,
                tag,
                num_type,
                2 + path.iter().filter(|&&k| k == kind).count(),
            );
        }
    }
}

/// 이것을 실패시키는 것: 잘못된 깊이/다음 컨트롤의 이름 빌리기, 손상된 이름에서
/// 상호참조 이름으로 추정, 이름 없는 점 큐 슬롯을 생략하여 다음 위치 이동.
#[test]
fn r4_bookmark_missing_or_wrong_scope_never_borrows_a_target() {
    for bad_depth in [1, 2, 3] {
        let (id, wrong) = r4_bookmark("wrong", false);
        let records = Record::parse_stream(&mut Cursor::new(wrong)).unwrap();
        let mut controls = make_record(TagId::CtrlHeader, 1, &id.to_le_bytes());
        let mut name_data = records[1].data.clone();
        if bad_depth == 2 {
            name_data[0] ^= 1;
        }
        controls.extend(make_record(TagId::CtrlData, bad_depth, &name_data));
        controls.extend(r4_bookmark("right", false).1);
        let mut text = r4_marker(0x16, id);
        text.extend(para_text_data("first"));
        text.extend(r4_marker(0x16, id));
        text.extend(para_text_data("second"));
        let payload = r4_para(&text, &controls, false);
        for path in r2_paths() {
            let parsed = parse_body_text(&r2_in_path(&payload, &path), &version()).unwrap();
            let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
            // The malformed CtrlData is explicitly diagnosed by the parser.
            let semantic: Vec<_> = warnings
                .into_iter()
                .filter(|w| {
                    if let Hwp5Warning::UnsupportedTag { tag_id, .. } = w {
                        assert_eq!(*tag_id, u16::from(TagId::CtrlData));
                        false
                    } else {
                        true
                    }
                })
                .collect();
            r4_assert_projection_warnings(&semantic, &["field.bookmark_point"]);
            let xml = r2_xml(doc);
            assert!(!xml.contains("name=\"wrong\""));
            assert_eq!(xml.matches("name=\"right\"").count(), 1);
            assert!(xml.find("first").unwrap() < xml.find("name=\"right\"").unwrap());
            assert!(xml.find("name=\"right\"").unwrap() < xml.find("second").unwrap());
        }
    }
}

/// 이것을 실패시키는 것: 중첩 해석기 종료에서 구역 상태 경고 누락.
#[test]
fn r4_nested_section_settings_are_explicitly_diagnosed() {
    let mut ctrl = crate::ctrl_ids::CTRL_ID_COLUMN_DEF.to_le_bytes().to_vec();
    ctrl.extend([4, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let mut payload = r1_paragraph("settings");
    payload.extend(make_record(TagId::CtrlHeader, 1, &ctrl));
    for path in r2_paths().into_iter().filter(|p| !p.is_empty()) {
        let parsed = parse_body_text(&r2_in_path(&payload, &path), &version()).unwrap();
        assert!(
            parsed.warnings.iter().any(|w| matches!(
                w,
                Hwp5Warning::DroppedControl { control: "nested_section_settings", .. }
            )),
            "{path:x?}: {:?}",
            parsed.warnings
        );
    }
}

/// 이것을 실패시키는 것: MEMO 외 바깥 필드 유실, 복합 ClickHere의 가시 본문 유실,
/// 닫히지 않은 필드 경고를 줄 캐시 조건에 종속, dangling 내부 메모 본문 유실.
#[test]
fn r4_compound_outer_field_families_and_dangling_memos() {
    let mut outers = r2_field_payloads();
    outers.push((
        "hyperlink",
        crate::ctrl_ids::CTRL_ID_HYPERLINK,
        r4_command(crate::ctrl_ids::CTRL_ID_HYPERLINK, "https\\://example.org/"),
    ));
    let (bookmark_id, bookmark) = r4_bookmark("r4-span", true);
    outers.push(("bookmark", bookmark_id, bookmark));
    for (kind, _, wire) in &mut outers {
        if *kind == "clickhere" {
            let records =
                Record::parse_stream(&mut Cursor::new(r4_bookmark("r4-form", false).1)).unwrap();
            wire.extend(make_record(TagId::CtrlData, 2, &records[1].data));
        }
    }
    for cache in [false, true] {
        for closed in [false, true] {
            for (kind, id, outer) in &outers {
                let (memo, cluster) = r4_memo(88, "R4dangling-memo");
                let mut controls = outer.clone();
                controls.extend(memo);
                let mut text = r4_marker(3, *id);
                text.extend(para_text_data("R4prefix"));
                text.extend(r4_marker(3, crate::ctrl_ids::CTRL_ID_MEMO_INLINE));
                text.extend(para_text_data("R4inner"));
                if closed {
                    text.extend(r4_marker(4, 0));
                }
                text.extend(para_text_data("R4suffix"));
                if closed {
                    text.extend(r4_marker(4, 0));
                }
                for path in r2_paths() {
                    let mut wire = r2_in_path(&r4_para(&text, &controls, cache), &path);
                    wire.extend(&cluster);
                    let parsed = parse_body_text(&wire, &version()).unwrap();
                    let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
                    let expected = if closed {
                        vec!["field.nested"]
                    } else {
                        vec!["field.nested", "field.unclosed"]
                    };
                    r4_assert_projection_warnings(&warnings, &expected);
                    let xml = r2_xml(doc);
                    assert_eq!(
                        xml.matches("R4prefixR4innerR4suffix").count(),
                        1,
                        "{kind} {path:x?}: {xml}"
                    );
                    assert_eq!(
                        xml.matches("R4dangling-memo").count(),
                        1,
                        "{kind} {path:x?}: {xml}"
                    );
                    let token = match *kind {
                        "clickhere" => "CLICK_HERE",
                        "crossref" => "CROSSREF",
                        "hyperlink" => "HYPERLINK",
                        "bookmark" => "BOOKMARK",
                        "path" => "PATH",
                        _ => "SUMMERY",
                    };
                    assert_eq!(xml.matches(&format!("type=\"{token}\"")).count(), 1);
                }
            }
        }
    }
}

/// 이것을 실패시키는 것: 객체 marker를 필드 가시 텍스트로 재방출, 문자값만 보고
/// 사용자가 입력한 U+FFFC까지 삭제. 바깥 필드와 객체 본문을 모두 보존한다.
#[test]
fn r4_field_object_placeholders_are_not_user_text() {
    let mut outers = vec![
        ("memo", crate::ctrl_ids::CTRL_ID_MEMO_INLINE, r4_memo(89, "R4memo-text").0),
        ("clickhere", CTRL_ID_CLICK_HERE, make_record(TagId::CtrlHeader, 1, &r1_clickhere())),
        ("fallback", crate::ctrl_ids::CTRL_ID_HYPERLINK, Vec::new()),
    ];
    let (id, bookmark) = r4_bookmark("r4-object-span", true);
    outers.push(("bookmark", id, bookmark));
    for (kind, id, mut controls) in outers {
        if kind == "clickhere" {
            let records =
                Record::parse_stream(&mut Cursor::new(r4_bookmark("r4-form", false).1)).unwrap();
            controls.extend(make_record(TagId::CtrlData, 2, &records[1].data));
        }
        let records = Record::parse_stream(&mut Cursor::new(r1_wrap(
            CTRL_ID_GSO,
            &r1_paragraph("R4object-text"),
        )))
        .unwrap();
        for r in records.into_iter().skip(2) {
            controls.extend(make_record(TagId::from(r.header.tag_id), r.header.level, &r.data));
        }
        let mut text = r4_marker(3, id);
        text.extend(para_text_data("R4literal\u{fffc}before"));
        text.extend(r4_marker(0x0b, CTRL_ID_GSO));
        text.extend(para_text_data("after"));
        text.extend(r4_marker(4, 0));
        for path in r2_paths() {
            let mut wire = r2_in_path(&r4_para(&text, &controls, false), &path);
            if kind == "memo" {
                wire.extend(r4_memo(89, "R4memo-text").1);
            }
            let parsed = parse_body_text(&wire, &version()).unwrap();
            let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
            let expected = if kind == "fallback" {
                vec!["field.object", "field.hyperlink"]
            } else {
                vec!["field.object"]
            };
            r4_assert_projection_warnings(&warnings, &expected);
            let xml = r2_xml(doc);
            assert_eq!(xml.matches('\u{fffc}').count(), 1, "{kind} {path:x?}: {xml}");
            assert!(xml.contains("R4literal\u{fffc}before"));
            assert_eq!(xml.matches("after").count(), 1);
            assert_eq!(xml.matches("R4object-text").count(), 1);
            if kind == "memo" {
                assert_eq!(xml.matches("R4memo-text").count(), 1);
            }
        }
    }
}

/// 이것을 실패시키는 것: 채워진 누름틀의 display_text를 비우거나 빈 필드 힌트를 본문으로 미리 복사.
#[test]
fn r5_clickhere_preserves_body_and_only_empty_uses_hint() {
    for body in ["", " ", "가상시 가상로 2 가상청사", "a😀\t나\n끝"]
    {
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

/// 이것을 실패시키는 것: 자동 필드 상한 초과를 무경고 절단하거나 손실량을 글자 수로 계산.
#[test]
fn r5_auto_field_truncation_reports_exact_utf16_loss() {
    for (kind, id, control) in r2_field_payloads()
        .into_iter()
        .filter(|(kind, _, _)| matches!(*kind, "summary" | "datecode" | "path"))
    {
        for (body, lost) in [
            ("A".repeat(4096), 0),
            ("A".repeat(4097), 1),
            ("😀".repeat(2048), 0),
            ("😀".repeat(2049), 2),
            (format!("{}😀끝", "A".repeat(4095)), 3),
        ] {
            for closed in [true, false] {
                let mut text = r4_marker(3, id);
                text.extend(para_text_data(&body));
                if closed {
                    text.extend(r4_marker(4, 0));
                }
                let parsed = parse_body_text(&r4_para(&text, &control, false), &version()).unwrap();
                let (doc, warnings) = crate::projection::project_to_core(vec![parsed]).unwrap();
                let losses: Vec<_> = warnings
                    .iter()
                    .filter_map(|w| match w {
                        Hwp5Warning::ProjectionFallback {
                            subject: "field.display_text.truncated",
                            reason,
                        } => Some(reason),
                        _ => None,
                    })
                    .collect();
                assert_eq!(losses.len(), usize::from(lost > 0), "{kind}: {warnings:?}");
                if lost > 0 {
                    assert!(
                        losses[0].contains(&format!("lost {lost} UTF-16 units")),
                        "{kind}: {losses:?}"
                    );
                }
                let xml = r2_xml(doc);
                assert!(xml.contains(&body.chars().take(100).collect::<String>()));
            }
        }
    }
}

/// 표본 130번의 원본 Section0
/// 레코드 349·355 PARA_TEXT payload. 본문은 원본 그대로, 메타데이터만 최소 합성.
/// 이것을 실패시키는 것: 실제 누름틀 본문을 안내 문구로 대체하거나 두 번째 형제 필드 누락.
#[test]
fn r5_sample_130_contact_records_preserve_filled_clickhere() {
    let address = "가상시 가상구 가상로 1 가상청사 가상과";
    let email = "user@example.com";
    let fax = "000-0000-0000";
    assert_eq!([address.chars().count(), email.chars().count(), fax.chars().count()], [37, 18, 12]);
    for (payload, expected) in [
        (
            include_bytes!(
                "../../../../../tests/fixtures/nested_controls/clickhere-contact-349.bin"
            )
            .as_slice(),
            vec![address],
        ),
        (
            include_bytes!(
                "../../../../../tests/fixtures/nested_controls/clickhere-contact-355.bin"
            )
            .as_slice(),
            vec![email, fax],
        ),
    ] {
        let controls = make_record(TagId::CtrlHeader, 1, &r1_clickhere()).repeat(expected.len());
        let parsed = parse_body_text(&r4_para(payload, &controls, false), &version()).unwrap();
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
