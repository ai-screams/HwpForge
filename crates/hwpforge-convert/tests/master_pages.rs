use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
use hwpforge_smithy_hwp5::Hwp5SemanticMasterPageScope;
use hwpforge_smithy_hwpx::{HwpxDecoder, PackageReader};
use std::{fs, path::PathBuf};

// 이것을 실패시키는 것: master hint를 section 큐에 붙이기 / cache off에서 표 처리를 끄기 / part 순서 변경.
#[test]
fn master_parts_convert_in_both_cache_modes_and_are_deterministic() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/master_pages");
    for name in [
        "f5a-last-page",
        "f5b-optional-page3",
        "f5c-odd-even",
        "f5d-overlap",
        "f5e-front",
        "f5f-shapes",
    ] {
        let input = fs::read(root.join(format!("{name}.hwp"))).unwrap();
        let reference =
            HwpxDecoder::decode(&fs::read(root.join(format!("{name}.hwpx"))).unwrap()).unwrap();
        let source_core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&input).unwrap();
        let source_picture_ids: Vec<_> = source_core.document.sections()[0]
            .master_pages
            .iter()
            .flatten()
            .flat_map(|m| &m.paragraphs)
            .flat_map(|p| &p.runs)
            .filter_map(|r| {
                if let hwpforge_core::run::RunContent::Image(image) = &r.content {
                    image.inst_id
                } else {
                    None
                }
            })
            .collect();
        for carry_layout_cache in [false, true] {
            let mut options = ConvertOptions::default();
            options.carry_layout_cache = carry_layout_cache;
            let (bytes, warnings) = hwp5_to_hwpx_bytes_with_diagnostics(&input, options).unwrap();
            let (again, again_warnings) =
                hwp5_to_hwpx_bytes_with_diagnostics(&input, options).unwrap();
            assert_eq!(bytes, again, "same Core must emit same bytes: {name}");
            assert_eq!(format!("{warnings:?}"), format!("{again_warnings:?}"));
            let actual = HwpxDecoder::decode(&bytes).unwrap();
            let a = actual.document.sections()[0].master_pages.as_ref().unwrap();
            let b = reference.document.sections()[0].master_pages.as_ref().unwrap();
            assert_eq!(
                a.iter().map(Hwp5SemanticMasterPageScope::from).collect::<Vec<_>>(),
                b.iter().map(Hwp5SemanticMasterPageScope::from).collect::<Vec<_>>()
            );
            let mut pkg = PackageReader::new(&bytes).unwrap();
            let paths = hwpforge_smithy_hwpx::master_page_part_paths(&mut pkg).unwrap();
            assert_eq!(paths[0].len(), a.len());
            let mut written_picture_ids = Vec::new();
            for part in &paths[0] {
                let xml = pkg.read_text_entry(part).unwrap();
                let mut reader = quick_xml::Reader::from_str(&xml);
                loop {
                    match reader.read_event().unwrap() {
                        quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                            if e.local_name().as_ref() == "pic" =>
                        {
                            let id =
                                e.attributes().flatten().find(|a| a.key.as_ref() == "id").unwrap();
                            written_picture_ids.push(hwpforge_core::ObjectId::new(
                                id.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                    .unwrap()
                                    .parse()
                                    .unwrap(),
                            ));
                        }
                        quick_xml::events::Event::Eof => break,
                        _ => {}
                    }
                }
                assert_eq!(xml.contains("<hp:linesegarray>"), carry_layout_cache, "{name} {part}");
            }
            assert_eq!(source_picture_ids, written_picture_ids, "imported master pic@id: {name}");
            // Existing Core JSON carries master lists across a body edit.
            let mut json = serde_json::to_value(&actual.document).unwrap();
            let masters = json["sections"][0]["master_pages"].clone();
            json["sections"][0]["paragraphs"][0]["runs"] =
                serde_json::json!([{"content":{"Text":"edited body"},"char_shape_id":0}]);
            let edited: hwpforge_core::document::Document<hwpforge_core::document::Draft> =
                serde_json::from_value(json).unwrap();
            assert_eq!(
                serde_json::to_value(&edited).unwrap()["sections"][0]["master_pages"],
                masters
            );
        }
    }
}

mod wire {
    use std::collections::BTreeMap;
    use std::io::{Cursor, Read, Write};
    use std::path::{Path, PathBuf};
    struct Record {
        tag: u16,
        level: u16,
        data: Vec<u8>,
    }

    fn write_records(records: &[Record]) -> Vec<u8> {
        let mut out = Vec::new();
        for record in records {
            let size = u32::try_from(record.data.len()).expect("record size");
            let head = u32::from(record.tag) | (u32::from(record.level) << 10);
            if size >= 0xFFF {
                out.extend_from_slice(&(head | (0xFFF << 20)).to_le_bytes());
                out.extend_from_slice(&size.to_le_bytes());
            } else {
                out.extend_from_slice(&(head | (size << 20)).to_le_bytes());
            }
            out.extend_from_slice(&record.data);
        }
        out
    }

    fn read_streams(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut file = cfb::CompoundFile::open(Cursor::new(bytes)).expect("cfb");
        let paths: Vec<PathBuf> =
            file.walk().filter(|e| e.is_stream()).map(|e| e.path().to_path_buf()).collect();
        paths
            .into_iter()
            .map(|path| {
                let mut data = Vec::new();
                file.open_stream(&path).expect("open").read_to_end(&mut data).expect("read");
                (path.to_string_lossy().into_owned(), data)
            })
            .collect()
    }

    fn write_streams(streams: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
        let mut out = cfb::CompoundFile::create(Cursor::new(Vec::new())).expect("create");
        for (path, data) in streams {
            let path = Path::new(path);
            if let Some(parent) = path.parent().filter(|p| *p != Path::new("/")) {
                if !out.exists(parent) {
                    out.create_storage_all(parent).expect("storage");
                }
            }
            out.create_stream(path).expect("stream").write_all(data).expect("write");
        }
        out.flush().expect("flush");
        out.into_inner().into_inner()
    }

    fn compressed(streams: &BTreeMap<String, Vec<u8>>) -> bool {
        streams["/FileHeader"][36] & 1 != 0
    }

    fn set_section(streams: &mut BTreeMap<String, Vec<u8>>, records: &[Record]) {
        let data = write_records(records);
        let data = if compressed(streams) {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&data).expect("deflate");
            encoder.finish().expect("deflate")
        } else {
            data
        };
        streams.insert("/BodyText/Section0".to_string(), data);
    }

    fn rec(tag: u16, level: u16, data: Vec<u8>) -> Record {
        Record { tag, level, data }
    }
    fn marker(code: u16, id: u32) -> Vec<u8> {
        [code, id as u16, (id >> 16) as u16, 0, 0, 0, 0, code]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect()
    }
    fn para(level: u16, data: Vec<u8>) -> Vec<Record> {
        let mut header = vec![0; 22];
        header[..4].copy_from_slice(&((data.len() / 2) as u32).to_le_bytes());
        vec![rec(0x42, level, header), rec(0x43, level + 1, data)]
    }
    fn text(level: u16, value: &str) -> Vec<Record> {
        para(level, value.encode_utf16().chain([13]).flat_map(u16::to_le_bytes).collect())
    }
    fn secd(bits: u32, extended: u16) -> Record {
        let mut data = vec![0; 32];
        data[..4].copy_from_slice(&0x73656364u32.to_le_bytes());
        data[4..8].copy_from_slice(&bits.to_le_bytes());
        data[30..32].copy_from_slice(&extended.to_le_bytes());
        rec(0x47, 1, data)
    }
    fn master(level: u16, selector: u32) -> Record {
        let mut data = vec![0; 34];
        data[0] = 1;
        data[8..12].copy_from_slice(&48190u32.to_le_bytes());
        data[12..16].copy_from_slice(&67181u32.to_le_bytes());
        data[18..22].copy_from_slice(&selector.to_le_bytes());
        rec(0x48, level, data)
    }
    fn click(level: u16, label: &str) -> Vec<Record> {
        let mut data = marker(3, 0x25636c6b);
        data.extend(label.encode_utf16().flat_map(u16::to_le_bytes));
        data.extend(marker(4, 0));
        data.extend(13u16.to_le_bytes());
        let mut records = para(level, data);
        let shapes = [(0u32, 0u32), (9, 1)]
            .into_iter()
            .flat_map(|(pos, id)| [pos.to_le_bytes(), id.to_le_bytes()].concat())
            .collect();
        records.push(rec(0x44, level + 1, shapes));
        let command = "Clickhere:set:0:Direction:wstring:0: HelpState:wstring:0:  ";
        let mut ctrl = 0x25636c6bu32.to_le_bytes().to_vec();
        ctrl.extend(1u32.to_le_bytes());
        ctrl.push(9);
        ctrl.extend((command.encode_utf16().count() as u16).to_le_bytes());
        ctrl.extend(command.encode_utf16().flat_map(u16::to_le_bytes));
        ctrl.extend([0; 8]);
        records.push(rec(0x47, level + 1, ctrl));
        // Valid trailing name record, keeping warning equality meaningful.
        let mut name = vec![0x1b, 2, 1, 0, 0, 0, 0, 0x40, 1, 0];
        name.extend(0u16.to_le_bytes());
        records.push(rec(0x57, level + 2, name));
        records
    }
    fn table(level: u16) -> Vec<Record> {
        let mut body = vec![0; 20];
        body[4] = 1;
        body[6] = 1;
        let mut cell = vec![0; 34];
        cell[0] = 1;
        cell[12] = 1;
        cell[14] = 1;
        cell[16..20].copy_from_slice(&9000u32.to_le_bytes());
        cell[20..24].copy_from_slice(&3000u32.to_le_bytes());
        cell[32] = 1;
        vec![
            rec(0x47, level, 0x74626c20u32.to_le_bytes().to_vec()),
            rec(0x4d, level + 1, body),
            rec(0x48, level + 1, cell),
        ]
    }
    fn input(records: &[Record]) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/master_pages/f5b-optional-page3.hwp");
        let mut streams = read_streams(&std::fs::read(path).unwrap());
        set_section(&mut streams, records);
        write_streams(&streams)
    }
    fn dump(name: &str, bytes: &[u8]) {
        if let Ok(out) = std::env::var("MP_FIX1_SYNTHETIC_OUT") {
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(Path::new(&out).join(format!("{name}.hwp")), bytes).unwrap();
        }
    }
    fn ids(paragraph: &hwpforge_core::Paragraph) -> Vec<String> {
        use hwpforge_core::{control::FieldBoundary, run::RunContent, Control};
        paragraph
            .runs
            .iter()
            .filter_map(|run| match &run.content {
                RunContent::Control(ctrl) => match ctrl.as_ref() {
                    Control::FieldBoundary { boundary: FieldBoundary::Start(start) } => {
                        Some(start.id.clone())
                    }
                    Control::FieldBoundary { boundary: FieldBoundary::End { begin_id, .. } } => {
                        Some(begin_id.clone())
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }
    // 이것을 실패시키는 것: master를 뒤에 투영하면서 본문 필드 B의 ID를 2에서 1로 바꾸기.
    #[test]
    fn master_field_ids_follow_source_stream_in_core_and_xml() {
        let mut records = text(0, "host");
        records.push(secd(1 << 29, 0));
        records.push(master(2, 0));
        records.extend(click(2, "AA"));
        records.extend(click(0, "BB"));
        let bytes = input(&records);
        dump("fields", &bytes);
        let core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
        let section = &core.document.sections()[0];
        assert_eq!(ids(&section.master_pages.as_ref().unwrap()[0].paragraphs[0]), ["1", "1"]);
        assert_eq!(ids(&section.paragraphs[1]), ["2", "2"]);
        for cache in [false, true] {
            let mut options = hwpforge_convert::ConvertOptions::default();
            options.carry_layout_cache = cache;
            let (output, _) =
                hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(&bytes, options).unwrap();
            let mut pkg = hwpforge_smithy_hwpx::PackageReader::new(&output).unwrap();
            for (part, id) in [("Contents/masterpage0.xml", "1"), ("Contents/section0.xml", "2")] {
                let xml = pkg.read_text_entry(part).unwrap();
                assert!(xml.contains(&format!("<hp:fieldBegin id=\"{id}\"")), "{xml}");
                assert!(xml.contains(&format!("beginIDRef=\"{id}\"")), "{xml}");
            }
        }
    }
    fn body_table(section: &hwpforge_core::Section) -> &hwpforge_core::Table {
        section.paragraphs[0]
            .runs
            .iter()
            .find_map(|run| match &run.content {
                hwpforge_core::run::RunContent::Table(table) => Some(table.as_ref()),
                _ => None,
            })
            .unwrap()
    }

    // 이것을 실패시키는 것: master가 있다는 이유로 캡션 A → 셀 B의 원본 순서로 ID를 재배정하기.
    #[test]
    fn master_preserves_body_table_cell_before_caption_field_ids() {
        let mut records = para(0, marker(11, 0x74626c20));
        records.push(secd(0, 1));
        let mut table_records = table(1);
        records.push(table_records.remove(0));
        let mut caption = vec![0; 22];
        caption[0] = 1;
        caption[8] = 3;
        records.push(rec(0x48, 2, caption));
        records.extend(click(2, "AA"));
        records.extend(table_records);
        records.extend(click(2, "BB"));
        records.push(master(1, 3));
        records.extend(text(1, "master without fields"));
        let bytes = input(&records);
        dump("caption-fields", &bytes);
        let core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
        let table = body_table(&core.document.sections()[0]);
        assert_eq!(ids(&table.rows[0].cells[0].paragraphs[0]), ["1", "1"]);
        assert_eq!(ids(&table.caption.as_ref().unwrap().paragraphs[0]), ["2", "2"]);
        for cache in [false, true] {
            let mut options = hwpforge_convert::ConvertOptions::default();
            options.carry_layout_cache = cache;
            let (output, _) =
                hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(&bytes, options).unwrap();
            let mut pkg = hwpforge_smithy_hwpx::PackageReader::new(&output).unwrap();
            let xml = pkg.read_text_entry("Contents/section0.xml").unwrap();
            for id in ["1", "2"] {
                assert!(xml.contains(&format!("<hp:fieldBegin id=\"{id}\"")), "{xml}");
                assert!(xml.contains(&format!("beginIDRef=\"{id}\"")), "{xml}");
            }
        }
    }

    // 이것을 실패시키는 것: master를 host 전체 앞/뒤로 옮기기 / master가 본문 ControlRef를 소비하기.
    #[test]
    fn master_projection_shares_counters_at_host_control_position() {
        // Generated IDs follow the repaired ListHeader insertion order (2026-10-20).
        // ed491d39's overwritten host consumed A first: X=2, A=1, Y=3.
        // ControlRef alone stays flat. A mixed-format ClickHere host exercises
        // the actual structural branch and consumes its own generated ID first.
        for mode in 0..3 {
            let mut records = match mode {
                0 => text(0, "host"),
                1 => para(0, [marker(11, 0x74626c20), marker(11, 0x74626c20)].concat()),
                _ => {
                    let mut host = click(0, "HH");
                    let end = host[1].data.len() - 2;
                    host[1].data.truncate(end);
                    host[1].data.extend(marker(11, 0x74626c20));
                    host[1].data.extend(marker(11, 0x74626c20));
                    host[1].data.extend(13u16.to_le_bytes());
                    let units = (host[1].data.len() / 2) as u32;
                    host[0].data[..4].copy_from_slice(&units.to_le_bytes());
                    host
                }
            };
            records.push(secd(0, 1));
            records.extend(table(1));
            records.extend(click(2, "XX"));
            records.push(master(1, 3));
            records.extend(click(1, "AA"));
            records.extend(table(1));
            records.extend(click(2, "YY"));
            let bytes = input(&records);
            dump(&format!("tables-mode-{mode}"), &bytes);
            let core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
            let section = &core.document.sections()[0];
            let tables: Vec<_> = section.paragraphs[0]
                .runs
                .iter()
                .filter_map(|run| match &run.content {
                    hwpforge_core::run::RunContent::Table(table) => Some(table),
                    _ => None,
                })
                .collect();
            assert_eq!(tables.len(), 2);
            let offset = usize::from(mode == 2);
            if mode == 2 {
                assert_eq!(ids(&section.paragraphs[0]), ["1", "1"]);
            }
            for (paragraph, number) in [
                (&tables[0].rows[0].cells[0].paragraphs[0], 1 + offset),
                (&section.master_pages.as_ref().unwrap()[0].paragraphs[0], 2 + offset),
                (&tables[1].rows[0].cells[0].paragraphs[0], 3 + offset),
            ] {
                assert_eq!(ids(paragraph), [number.to_string(), number.to_string()]);
            }
        }
    }

    // 이것을 실패시키는 것: 두 master 중 하나 유실 / ID 중복·짝 불일치 / 실행마다 순서 변경.
    #[test]
    fn multiple_master_fields_are_unique_paired_and_deterministic() {
        let mut records = text(0, "host");
        records.push(secd(6 << 29, 0));
        records.push(master(2, 0));
        records.extend(click(2, "EVEN"));
        records.push(master(2, 0));
        records.extend(click(2, "ODD"));
        records.extend(click(0, "BB"));
        let bytes = input(&records);
        dump("multiple-fields", &bytes);
        let core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
        let section = &core.document.sections()[0];
        let masters = section.master_pages.as_ref().unwrap();
        assert_eq!(masters.len(), 2);
        assert_eq!(masters[0].apply_page_type, hwpforge_foundation::ApplyPageType::Even);
        assert_eq!(masters[1].apply_page_type, hwpforge_foundation::ApplyPageType::Odd);
        // Generated sequence, not an imported ID: restoring both fields means
        // EVEN=1, ODD=2, B=3. ed491d39 lost one master and assigned B=2.
        let paragraphs =
            [&masters[0].paragraphs[0], &masters[1].paragraphs[0], &section.paragraphs[1]];
        let mut starts = std::collections::BTreeSet::new();
        for (index, paragraph) in paragraphs.into_iter().enumerate() {
            let id = (index + 1).to_string();
            assert_eq!(ids(paragraph), [id.clone(), id.clone()]);
            assert!(starts.insert(id));
            // The fixture deliberately spans two character formats.
            let shapes: std::collections::BTreeSet<_> =
                paragraph.runs.iter().map(|r| r.char_shape_id).collect();
            assert!(shapes.len() >= 2);
        }
        let serialized = serde_json::to_value(&core.document).unwrap();
        assert_eq!(
            serialized,
            serde_json::to_value(
                hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap().document
            )
            .unwrap()
        );
        for cache in [false, true] {
            let mut options = hwpforge_convert::ConvertOptions::default();
            options.carry_layout_cache = cache;
            let (output, _) =
                hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(&bytes, options).unwrap();
            let (repeat, _) =
                hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(&bytes, options).unwrap();
            assert_eq!(output, repeat);
            let mut pkg = hwpforge_smithy_hwpx::PackageReader::new(&output).unwrap();
            for (part, id, label) in [
                ("Contents/masterpage0.xml", "1", "EVEN"),
                ("Contents/masterpage1.xml", "2", "ODD"),
                ("Contents/section0.xml", "3", "BB"),
            ] {
                let xml = pkg.read_text_entry(part).unwrap();
                assert_eq!(xml.matches("<hp:fieldBegin ").count(), 1, "{xml}");
                assert_eq!(xml.matches(&format!("<hp:fieldBegin id=\"{id}\"")).count(), 1, "{xml}");
                assert_eq!(xml.matches(&format!("beginIDRef=\"{id}\"")).count(), 1, "{xml}");
                // Text is split across runs at the format boundary.
                assert!(serde_json::to_string(paragraphs[id.parse::<usize>().unwrap() - 1])
                    .unwrap()
                    .contains(&label[1..]));
            }
        }
    }

    fn note(level: u16, id: u32, instance: u32, label: &str) -> Vec<Record> {
        let mut records = para(level, marker(6, id));
        let mut ctrl = vec![0; 20];
        ctrl[..4].copy_from_slice(&id.to_le_bytes());
        ctrl[16..20].copy_from_slice(&instance.to_le_bytes());
        records.push(rec(0x47, level + 1, ctrl));
        records.push(rec(0x48, level + 2, vec![1, 0, 0, 0, 0, 0]));
        records.extend(text(level + 2, label));
        records
    }

    // 이것을 실패시키는 것: master 각주/미주 경고 누락·위치 누락 / 본문 각주 변경 / master 내용 유실.
    #[test]
    fn master_notes_warn_once_with_source_location_and_preserve_body() {
        for (id, kind) in [(0x666e2020, "footnote"), (0x656e2020, "endnote")] {
            let body = note(0, id, 202, "body B");
            let reference = hwpforge_smithy_hwp5::decode_hwp5_to_core(&input(&body)).unwrap();
            let mut records = text(0, "host");
            records.push(secd(1 << 29, 0));
            records.push(master(2, 0));
            records.extend(note(2, id, 101, "master A"));
            records.extend(body);
            let bytes = input(&records);
            dump(kind, &bytes);
            let core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
            let section = &core.document.sections()[0];
            assert_eq!(
                serde_json::to_value(&section.paragraphs[1]).unwrap(),
                serde_json::to_value(&reference.document.sections()[0].paragraphs[0]).unwrap()
            );
            let master = serde_json::to_string(&section.master_pages).unwrap();
            assert!(master.contains("master A") && master.contains("101"));
            let warnings: Vec<_> = core
                .warnings
                .iter()
                .filter_map(|warning| match warning {
                    hwpforge_smithy_hwp5::Hwp5Warning::ProjectionFallback {
                        subject: "master_page.note_numbering",
                        reason,
                    } => Some(reason),
                    _ => None,
                })
                .collect();
            assert_eq!(warnings.len(), 1, "{:?}", core.warnings);
            assert_eq!(core.warnings.len(), 1, "{:?}", core.warnings);
            assert!(warnings[0].contains("master list 0 record 3 level 2"));
            assert!(warnings[0].contains(&format!("{kind} CtrlHeader record 6 level 3")));
            assert!(warnings[0].contains("NewNumber") && warnings[0].contains("cross-reference"));
            for cache in [false, true] {
                let mut options = hwpforge_convert::ConvertOptions::default();
                options.carry_layout_cache = cache;
                let (output, warnings) =
                    hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(&bytes, options).unwrap();
                assert_eq!(warnings.len(), 1, "{warnings:?}");
                let mut pkg = hwpforge_smithy_hwpx::PackageReader::new(&output).unwrap();
                assert!(pkg.read_text_entry("Contents/section0.xml").unwrap().contains("body B"));
                assert!(pkg
                    .read_text_entry("Contents/masterpage0.xml")
                    .unwrap()
                    .contains("master A"));
            }
        }
    }

    fn restart(level: u16, kind: u32) -> Vec<Record> {
        let mut records = para(level, marker(21, 0x6e776e6f));
        let mut ctrl = 0x6e776e6fu32.to_le_bytes().to_vec();
        ctrl.extend(kind.to_le_bytes());
        ctrl.extend(7u16.to_le_bytes());
        records.push(rec(0x47, level + 1, ctrl));
        records
    }

    fn memo(level: u16, id: u32) -> Vec<Record> {
        let mut records = para(level, [marker(3, 0x25256d65), marker(4, 0)].concat());
        let command = format!("MEMO/65535/{id}/123/456/author/\\;;");
        let mut ctrl = 0x25756e6bu32.to_le_bytes().to_vec();
        ctrl.extend([0; 4]);
        ctrl.extend((command.encode_utf16().count() as u16).to_be_bytes());
        ctrl.extend(command.encode_utf16().flat_map(u16::to_be_bytes));
        records.push(rec(0x47, level + 1, ctrl));
        records
    }

    fn numbering_warnings(warnings: &[hwpforge_smithy_hwp5::Hwp5Warning]) -> Vec<&str> {
        warnings
            .iter()
            .filter_map(|warning| match warning {
                hwpforge_smithy_hwp5::Hwp5Warning::ProjectionFallback {
                    subject: "master_page.note_numbering",
                    reason,
                } => Some(reason.as_str()),
                _ => None,
            })
            .collect()
    }

    // 이것을 실패시키는 것: fn/en 없는 master의 NewNumber 경고 누락 / 다른 종류 오경고.
    #[test]
    fn master_note_restarts_warn_without_direct_notes() {
        for kind in 0..=5 {
            let mut records = text(0, "host");
            records.push(secd(1 << 29, 0));
            records.push(master(2, 0));
            records.extend(restart(2, kind));
            records.extend(note(0, 0x666e2020, 202, "body B"));
            let bytes = input(&records);
            dump(&format!("restart-{kind}"), &bytes);
            let core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
            let warnings = numbering_warnings(&core.warnings);
            assert_eq!(warnings.len(), usize::from(matches!(kind, 1 | 2)), "{:?}", core.warnings);
            if matches!(kind, 1 | 2) {
                let name = if kind == 1 { "Footnote" } else { "Endnote" };
                assert!(
                    warnings[0].contains(&format!("NewNumber({name}) CtrlHeader record 6 level 3"))
                );
                assert!(warnings[0].contains("master list 0 record 3 level 2"));
            }
            let mut body = restart(0, kind);
            body.extend(note(0, 0x666e2020, 202, "body B"));
            let reference = hwpforge_smithy_hwp5::decode_hwp5_to_core(&input(&body)).unwrap();
            assert!(reference.warnings.is_empty(), "{:?}", reference.warnings);
            assert_eq!(
                serde_json::to_value(&core.document.sections()[0].paragraphs[1]).unwrap(),
                serde_json::to_value(&reference.document.sections()[0].paragraphs[1]).unwrap()
            );
        }
    }

    // 이것을 실패시키는 것: 지연 연결 메모 각주 누락 / 직접 각주 중복 / 본문 메모 오경고 / 원시 위치 상실.
    #[test]
    fn master_memo_notes_warn_once_after_final_ownership() {
        for (id, kind) in [(0x666e2020, "footnote"), (0x656e2020, "endnote")] {
            for with_master in [false, true] {
                let mut records = if with_master {
                    let mut records = text(0, "host");
                    records.push(secd(1 << 29, 0));
                    let mut list = master(2, 0);
                    list.data[0] = 2;
                    records.push(list);
                    records.extend(note(2, id, 101, "direct A"));
                    records.extend(memo(2, 7));
                    records
                } else {
                    memo(0, 7)
                };
                records.extend(note(0, id, 202, "body B"));
                // A separate body-owned memo must stay warning-free even when
                // the same section has masters and delayed master memo notes.
                records.extend(memo(0, 8));
                records.push(rec(0x5d, 1, 7u32.to_le_bytes().to_vec()));
                records.push(rec(0x48, 1, 1u32.to_le_bytes().to_vec()));
                let note_record = records.len() + 2;
                records.extend(note(1, id, 303, "memo A"));
                records.push(rec(0x5d, 1, 8u32.to_le_bytes().to_vec()));
                records.push(rec(0x48, 1, 1u32.to_le_bytes().to_vec()));
                records.extend(note(1, id, 404, "body memo"));
                let bytes = input(&records);
                dump(&format!("memo-{kind}-{with_master}"), &bytes);
                let core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
                let warnings = numbering_warnings(&core.warnings);
                assert_eq!(warnings.len(), if with_master { 2 } else { 0 }, "{:?}", core.warnings);
                assert_eq!(core.warnings.len(), warnings.len(), "{:?}", core.warnings);
                if with_master {
                    assert!(warnings[0].contains(&format!("{kind} CtrlHeader record 6 level 3")));
                    assert!(warnings[1]
                        .contains(&format!("{kind} CtrlHeader record {note_record} level 2")));
                    assert!(
                        warnings[1].contains("master list 0 record 3 level 2")
                            && warnings[1].contains("memo_id=7")
                    );
                    let master =
                        serde_json::to_string(&core.document.sections()[0].master_pages).unwrap();
                    assert!(master.contains("memo A") && master.contains("303"));
                }
                for cache in [false, true] {
                    let mut options = hwpforge_convert::ConvertOptions::default();
                    options.carry_layout_cache = cache;
                    let (output, warnings) =
                        hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(&bytes, options)
                            .unwrap();
                    assert_eq!(warnings.len(), if with_master { 2 } else { 0 }, "{warnings:?}");
                    let mut pkg = hwpforge_smithy_hwpx::PackageReader::new(&output).unwrap();
                    let part = if with_master {
                        "Contents/masterpage0.xml"
                    } else {
                        "Contents/section0.xml"
                    };
                    let xml = pkg.read_text_entry(part).unwrap();
                    assert!(xml.contains("memo A") && xml.contains("instId=\"303\""), "{xml}");
                }
            }
        }
    }

    // 이것을 실패시키는 것: 각주/필드/중첩 표가 닫히기 전에 소유 표를 마감하기.
    #[test]
    fn master_boundary_preserves_last_cell_nested_contents_and_conversion() {
        use hwpforge_core::run::RunContent;
        for kind in ["note", "field", "table"] {
            let mut records = para(0, marker(11, 0x74626c20));
            records.push(secd(0, 1));
            records.extend(table(1));
            match kind {
                "note" => {
                    records.extend(para(2, marker(6, 0x666e2020)));
                    records.push(rec(0x47, 3, 0x666e2020u32.to_le_bytes().to_vec()));
                    records.push(rec(0x48, 4, vec![1, 0, 0, 0, 0, 0]));
                    records.extend(text(4, "cell note"));
                }
                "field" => records.extend(click(2, "cell field")),
                _ => {
                    records.extend(para(2, marker(11, 0x74626c20)));
                    records.extend(table(3));
                    records.extend(text(4, "inner cell"));
                }
            }
            let mut reference =
                records.iter().map(|r| rec(r.tag, r.level, r.data.clone())).collect::<Vec<_>>();
            // Same owner is closed through ordinary EOF dispatch, without any master.
            reference[2] = secd(0, 0);
            let reference_bytes = input(&reference);
            let expected = hwpforge_smithy_hwp5::decode_hwp5_to_core(&reference_bytes).unwrap();
            records.push(master(1, 3));
            records.extend(text(1, "master"));
            let bytes = input(&records);
            dump(kind, &bytes);
            let core = hwpforge_smithy_hwp5::decode_hwp5_to_core(&bytes).unwrap();
            let table = |section: &hwpforge_core::Section| {
                section.paragraphs[0]
                    .runs
                    .iter()
                    .find_map(|r| match &r.content {
                        RunContent::Table(t) => Some(serde_json::to_value(t).unwrap()),
                        _ => None,
                    })
                    .unwrap()
            };
            let actual = table(&core.document.sections()[0]);
            assert_eq!(actual, table(&expected.document.sections()[0]), "{kind}");
            assert!(actual.to_string().contains(match kind {
                "note" => "cell note",
                "field" => "cell",
                _ => "inner cell",
            }));
            for cache in [false, true] {
                let mut options = hwpforge_convert::ConvertOptions::default();
                options.carry_layout_cache = cache;
                let (_, warnings) =
                    hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(&bytes, options).unwrap();
                let (_, expected_warnings) = hwpforge_convert::hwp5_to_hwpx_bytes_with_diagnostics(
                    &reference_bytes,
                    options,
                )
                .unwrap();
                assert_eq!(format!("{warnings:?}"), format!("{expected_warnings:?}"), "{kind}");
            }
        }
    }
}
