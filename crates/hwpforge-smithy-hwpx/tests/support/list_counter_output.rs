//! Evaluate emitted numbering using parsed XML; expected observations stay in the input tables.
use super::{package, xml};
pub fn rendered(bytes: &[u8]) -> Vec<String> {
    rendered_items(bytes).into_iter().map(|(_, value)| value).collect()
}
pub fn rendered_items(bytes: &[u8]) -> Vec<(String, String)> {
    use std::collections::BTreeMap;
    let h = xml::Element::parse(&package::part(bytes, "Contents/header.xml"));
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut names: Vec<_> = archive
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .filter(|n| n.starts_with("Contents/section") && n.ends_with(".xml"))
        .collect();
    names.sort();
    let mut states: BTreeMap<u32, ([u64; 10], [bool; 10])> = BTreeMap::new();
    let mut output = Vec::new();
    for name in names {
        let section = xml::Element::parse(&package::part(bytes, &name));
        for p in section.descendants("hp:p") {
            let shape = h
                .descendants("hh:paraPr")
                .into_iter()
                .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap();
            let heading = shape.child("hh:heading");
            if heading.attr("type") != Some("NUMBER") {
                continue;
            }
            let id = heading.attr("idRef").unwrap().parse::<u32>().unwrap();
            let level = heading.attr("level").unwrap().parse::<usize>().unwrap();
            let def = h
                .descendants("hh:numbering")
                .into_iter()
                .find(|d| d.attr("id") == Some(id.to_string().as_str()))
                .unwrap();
            let heads = def.descendants("hh:paraHead");
            let mut starts = [1; 10];
            for head in &heads {
                starts[head.attr("level").unwrap().parse::<usize>().unwrap() - 1] =
                    head.attr("start").unwrap().parse::<u64>().unwrap().max(1);
            }
            let (values, used) = states.entry(id).or_insert(([0; 10], [false; 10]));
            for i in 0..=level {
                if !used[i] {
                    values[i] = starts[i];
                    used[i] = true;
                } else if i == level {
                    values[i] += 1;
                }
            }
            for bit in &mut used[level + 1..] {
                *bit = false;
            }
            let head = heads
                .iter()
                .find(|head| head.attr("level") == Some((level + 1).to_string().as_str()))
                .unwrap();
            let mut text = head.text.clone();
            for i in (0..10).rev() {
                let format = heads
                    .iter()
                    .find(|head| head.attr("level") == Some((i + 1).to_string().as_str()))
                    .and_then(|head| head.attr("numFormat"))
                    .unwrap_or("DIGIT");
                let number = match format {
                    "ROMAN_CAPITAL" => [
                        "", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI",
                        "XII", "XIII",
                    ][values[i] as usize]
                        .into(),
                    "ROMAN_SMALL" => [
                        "", "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi",
                        "xii", "xiii",
                    ][values[i] as usize]
                        .into(),
                    "LATIN_SMALL" => {
                        ["", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m"]
                            [values[i] as usize]
                            .into()
                    }
                    "LATIN_CAPITAL" => {
                        ["", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M"]
                            [values[i] as usize]
                            .into()
                    }
                    "HANGUL_SYLLABLE" => ["", "가", "나", "다", "라"][values[i] as usize].into(),
                    _ => values[i].to_string(),
                };
                text = text.replace(&format!("^{}", i + 1), &number);
            }
            let own_text = p
                .children
                .iter()
                .filter(|r| r.name == "hp:run")
                .flat_map(|r| &r.children)
                .filter(|t| t.name == "hp:t")
                .map(|t| t.text.as_str())
                .collect();
            output.push((own_text, text));
        }
    }
    output
}
