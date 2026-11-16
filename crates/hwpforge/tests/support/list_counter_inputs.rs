//! Reproducible source packages for imported list contracts.
use super::word_counter_cases::{counter_cases, Case};
use std::io::{Cursor, Write};
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const FONTS:&str="<w:rFonts w:ascii='Malgun Gothic' w:hAnsi='Malgun Gothic' w:eastAsia='Malgun Gothic' w:cs='Malgun Gothic'/><w:sz w:val='22'/><w:szCs w:val='22'/>";
fn package(body: &str, definitions: &str, styles: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let document=format!("<w:document xmlns:w='{W}'><w:body>{body}<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:top='1417' w:bottom='1417' w:left='1417' w:right='1417' w:header='0' w:footer='0' w:gutter='0'/></w:sectPr></w:body></w:document>");
    let numbering = format!("<w:numbering xmlns:w='{W}'>{definitions}</w:numbering>");
    let styles=format!("<w:styles xmlns:w='{W}'><w:docDefaults><w:rPrDefault><w:rPr>{FONTS}<w:color w:val='000000'/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before='0' w:after='80' w:line='240' w:lineRule='auto'/></w:pPr></w:pPrDefault></w:docDefaults>{styles}</w:styles>");
    for(name,data) in [
        ("[Content_Types].xml","<Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/xml'/><Override PartName='/word/document.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/><Override PartName='/word/numbering.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml'/><Override PartName='/word/styles.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml'/></Types>"),
        ("_rels/.rels","<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='document' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='word/document.xml'/></Relationships>"),
        ("word/_rels/document.xml.rels","<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='numbering' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering' Target='numbering.xml'/><Relationship Id='styles' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles' Target='styles.xml'/></Relationships>"),
        ("word/document.xml",document.as_str()),("word/numbering.xml",numbering.as_str()),("word/styles.xml",styles.as_str()),
    ]{zip.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();zip.write_all(data.as_bytes()).unwrap();}
    zip.finish().unwrap().into_inner()
}
fn layout(definitions: &str) -> String {
    // Each source level may already carry paragraph or character properties.
    definitions
        .split_inclusive("</w:lvl>")
        .map(|level| {
            if !level.ends_with("</w:lvl>") {
                return level.to_owned();
            }
            let index = (0..10).find(|i| level.contains(&format!("<w:lvl w:ilvl='{i}'>"))).unwrap();
            let left = 720 + 360 * index;
            let mut properties = String::new();
            if !level.contains("<w:lvlJc ") {
                properties.push_str("<w:lvlJc w:val='left'/>");
            }
            if !level.contains("<w:pPr>") {
                properties
                    .push_str(&format!("<w:pPr><w:ind w:left='{left}' w:hanging='360'/></w:pPr>"));
            }
            if !level.contains("<w:rPr>") {
                properties.push_str(&format!("<w:rPr>{FONTS}</w:rPr>"));
            }
            let level = if level.contains("<w:rPr>") {
                level.replace("<w:rPr>", &format!("<w:rPr>{FONTS}"))
            } else {
                level.to_owned()
            };
            level.replace("</w:lvl>", &format!("{properties}</w:lvl>"))
        })
        .collect()
}

fn items(case: &Case, num_override: Option<u32>, bold: Option<usize>) -> String {
    case.uses.iter().enumerate().map(|(i,&(num,level))|{
        let num=num_override.unwrap_or(num);
        let extra=if bold==Some(i){"<w:rPr><w:b/></w:rPr>"}else{""};
        format!("<w:p><w:pPr><w:numPr><w:ilvl w:val='{level}'/><w:numId w:val='{num}'/></w:numPr>{extra}</w:pPr><w:r><w:t>item {}</w:t></w:r></w:p>",i+1)
    }).collect()
}
pub fn word_case(case: &Case) -> Vec<u8> {
    let bold = case.marker_bold;
    package(&items(case, None, bold), &layout(&case.definitions), case.styles)
}
pub fn roman_compound() -> Vec<u8> {
    let mut case = counter_cases()
        .into_iter()
        .find(|c| c.name == "compound_markers_keep_roman_ancestors")
        .unwrap();
    case.uses = vec![(7, 0), (7, 1), (7, 1), (7, 0), (7, 1)];
    package(&items(&case, None, Some(2)), &layout(&case.definitions), "")
}
pub fn legal_and_deep() -> Vec<u8> {
    let cases = counter_cases();
    let mut body = String::new();
    let mut definitions = String::new();
    for (i, name) in [
        "legal_markers_preserve_returning_parent_values",
        "legal_own_level_matches_word_display",
        "nonlegal_roman_control_matches_word_display",
        "deep_first_use_preserves_nonunit_starts",
        "deep_marker_style_split_preserves_nonunit_starts",
        "nonlegal_children_of_legal_parents_match_word_display",
    ]
    .iter()
    .enumerate()
    {
        let case = cases.iter().find(|c| c.name == *name).unwrap();
        let abstract_id = 801 + i;
        let num = 111 + i as u32;
        let defs = case
            .definitions
            .replace("w:abstractNumId='1'", &format!("w:abstractNumId='{abstract_id}'"))
            .replace(
                "<w:abstractNumId w:val='1'/>",
                &format!("<w:abstractNumId w:val='{abstract_id}'/>"),
            );
        let defs = defs
            .replace("w:numId='7'", &format!("w:numId='{num}'"))
            .replace("w:numId='8'", &format!("w:numId='{}'", num + 100));
        let kind = if i == 1 || i == 2 { "singleLevel" } else { "multilevel" };
        let defs = defs.replace(
            &format!("<w:abstractNum w:abstractNumId='{abstract_id}'>"),
            &format!(
                "<w:abstractNum w:abstractNumId='{abstract_id}'><w:multiLevelType w:val='{kind}'/>"
            ),
        );
        definitions.push_str(&layout(&defs));
        body.push_str(&format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", name.replace('_', " ")));
        body.push_str(&items(case, Some(num), case.marker_bold));
    }
    package(&body, &definitions, "")
}

pub fn marker_slot(
    compound: bool,
    nondecimal: bool,
    marker: &str,
    gap: i32,
    anchor: i32,
) -> Vec<u8> {
    marker_slot_layout(compound, nondecimal, marker, gap, anchor, ("left", false))
}
pub fn marker_slot_layout(
    compound: bool,
    nondecimal: bool,
    marker: &str,
    gap: i32,
    anchor: i32,
    layout: (&str, bool),
) -> Vec<u8> {
    let (align, explicit_tab) = layout;
    let tabs = if explicit_tab {
        format!("<w:tabs><w:tab w:val='num' w:pos='{}'/></w:tabs>", (anchor + gap) / 5)
    } else {
        String::new()
    };
    let own = if compound { 2 } else { 0 };
    let mut levels = String::new();
    for index in 0..=own {
        let format = if nondecimal && index == 0 { "upperRoman" } else { "decimal" };
        let text = if compound && index == own {
            "%1.%2.%3".to_owned()
        } else {
            format!("%{}.", index + 1)
        };
        let extra = format!("<w:lvlJc w:val='{align}'/><w:suff w:val='tab'/><w:pPr>{tabs}<w:ind w:left='{}' w:hanging='{}'/></w:pPr><w:rPr>{marker}</w:rPr>", (anchor + gap) / 5, gap / 5);
        levels.push_str(&super::word_counter_cases::level(index, Some(1), format, &text, &extra));
    }
    let definitions = format!(
        "{}{}",
        super::word_counter_cases::definition(1, &levels),
        super::word_counter_cases::num(7, 1, "")
    );
    let body = format!("<w:p><w:pPr><w:numPr><w:ilvl w:val='{own}'/><w:numId w:val='7'/></w:numPr></w:pPr><w:r><w:t>body</w:t></w:r></w:p>");
    package(&body, &definitions, "")
}
