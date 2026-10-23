//! Counts the source wire independently of the semantic parser under audit.
use super::{push_metric, MetricComparison};
use hwpforge_smithy_hwp5::Hwp5CensusReport;
use hwpforge_smithy_hwpx::{HwpxResult, PackageReader};
use quick_xml::{events::Event, Reader, XmlVersion};

#[derive(Default)]
struct Counts {
    tables: usize,
    compose: usize,
    chars: usize,
}

fn scan(xml: &str) -> Counts {
    let mut counts = Counts::default();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match e.local_name().as_ref() {
                "tbl" => counts.tables += 1,
                "compose" => counts.compose += 1,
                "t" => in_text = true,
                "tab" | "lineBreak" | "nbSpace" | "fwSpace" if in_text => counts.chars += 1,
                _ => {}
            },
            Ok(Event::End(e)) if e.local_name().as_ref() == "t" => in_text = false,
            Ok(Event::Text(t)) if in_text => {
                counts.chars += t.xml_content(XmlVersion::Explicit1_0).chars().count()
            }
            Ok(Event::CData(t)) if in_text => counts.chars += t.as_ref().chars().count(),
            Ok(Event::GeneralRef(_)) if in_text => counts.chars += 1,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    counts
}

pub(super) fn compare(
    source: &Hwp5CensusReport,
    bytes: &[u8],
    comparisons: &mut Vec<MetricComparison>,
    mismatch: &mut bool,
) -> HwpxResult<()> {
    let mut package = PackageReader::new(bytes)?;
    let masters = hwpforge_smithy_hwpx::master_page_part_paths(&mut package)?;
    for section in &source.sections {
        let mut output = if section.index < package.section_count() {
            scan(&package.read_section_xml(section.index)?)
        } else {
            Counts::default()
        };
        for part in masters.get(section.index).into_iter().flatten() {
            let master = scan(&package.read_text_entry(part)?);
            output.tables += master.tables;
            output.compose += master.compose;
            output.chars += master.chars;
        }
        let controls = |id: &str| {
            section
                .ctrl_ids
                .iter()
                .filter(|c| c.ctrl_id_ascii == id)
                .map(|c| c.count)
                .sum::<usize>()
        };
        let mut metric = |name: &str, input, out| {
            push_metric(
                comparisons,
                mismatch,
                &format!("raw.sections[{}].{name}", section.index),
                input,
                out,
            )
        };
        metric(
            "table_records",
            section.tag_counts.iter().filter(|t| t.tag_id == 0x4D).map(|t| t.count).sum(),
            output.tables,
        );
        metric("table_controls", controls("tbl "), output.tables);
        metric("compose_controls", controls("tcps"), output.compose);
        metric("para_text_chars", section.para_text_chars, output.chars);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// 이것을 실패시키는 것: 빈 t 종료 무시 / 엔티티·탭 무시 / 필드 파라미터 포함.
    #[test]
    fn raw_output_counts_only_visible_text() {
        let c = super::scan(
            r#"<p><t/><stringParam>not text</stringParam><t>A&amp;&#x1F600;<tab/><lineBreak/><nbSpace/><fwSpace/></t><compose/><tbl/></p>"#,
        );
        assert_eq!((c.tables, c.compose, c.chars), (1, 1, 7));
    }
}
