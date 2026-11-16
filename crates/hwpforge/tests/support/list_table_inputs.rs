//! Small source documents with independently specified numbering values.
use super::docx;

pub struct Case {
    pub name: &'static str,
    pub body: String,
    pub expected: &'static [&'static str],
    pub definitions: usize,
    pub splits: u64,
    pub hosts: Vec<(&'static str, usize)>,
}
pub fn item(id: u32, level: u8, text: &str) -> String {
    format!("<w:p><w:pPr><w:numPr><w:numId w:val='{id}'/><w:ilvl w:val='{level}'/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
}
pub fn table(rows: &[Vec<String>]) -> String {
    let rows: String = rows
        .iter()
        .map(|cells| {
            let cells: String = cells.iter().map(|body| format!("<w:tc>{body}</w:tc>")).collect();
            format!("<w:tr>{cells}</w:tr>")
        })
        .collect();
    format!("<w:tbl>{rows}</w:tbl>")
}
pub fn definitions() -> String {
    (1..=2).map(|id| format!("<w:abstractNum w:abstractNumId='{id}'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/><w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr></w:lvl><w:lvl w:ilvl='1'><w:start w:val='1'/><w:numFmt w:val='lowerLetter'/><w:lvlText w:val='%2)'/><w:pPr><w:ind w:left='1080' w:hanging='360'/></w:pPr></w:lvl></w:abstractNum><w:num w:numId='{id}'><w:abstractNumId w:val='{id}'/></w:num>")).collect()
}
pub fn source(body: &str) -> Vec<u8> {
    docx::package_with_numbering(body, &definitions())
}
pub fn cases() -> Vec<Case> {
    let a = item(1, 0, "a");
    let b = item(1, 0, "b");
    let c = item(1, 0, "c");
    let d = item(1, 0, "d");
    let e = item(1, 0, "e");
    let f = item(1, 0, "f");
    let g = item(1, 0, "g");
    let pair = table(&[vec![b.clone(), c.clone()]]);
    let nested = table(&[vec![c.clone(), d.clone()]]);
    let plain = "<w:p><w:r><w:t>plain</w:t></w:r></w:p>";
    vec![
        Case {
            name: "numbered host",
            body: format!("{a}{pair}{d}{e}"),
            expected: &["1.", "2.", "3.", "4.", "5."],
            definitions: 4,
            splits: 3,
            hosts: vec![("d", 1)],
        },
        Case {
            name: "rows",
            body: format!(
                "{a}{}{f}",
                table(&[vec![b.clone(), c.clone()], vec![d.clone(), e.clone()]])
            ),
            expected: &["1.", "2.", "3.", "4.", "5.", "6."],
            definitions: 3,
            splits: 2,
            hosts: vec![("f", 1)],
        },
        Case {
            name: "cell paragraphs",
            body: format!("{a}{}{e}", table(&[vec![format!("{b}{c}"), item(1, 1, "d")]])),
            expected: &["1.", "2.", "3.", "a)", "4."],
            definitions: 3,
            splits: 2,
            hosts: vec![("e", 1)],
        },
        Case {
            name: "table first",
            body: format!("{}{c}", table(&[vec![a.clone(), b.clone()]])),
            expected: &["1.", "2.", "3."],
            definitions: 2,
            splits: 1,
            hosts: vec![("c", 1)],
        },
        Case {
            name: "independent cells",
            body: format!("{a}{}{d}", table(&[vec![item(2, 0, "b"), item(2, 0, "c")]])),
            expected: &["1.", "1.", "2.", "2."],
            definitions: 2,
            splits: 0,
            hosts: vec![("d", 1)],
        },
        Case {
            name: "nested plain host",
            body: format!("{a}{}{e}", table(&[vec![format!("{b}{nested}<w:p/>")]])),
            expected: &["1.", "2.", "3.", "4.", "5."],
            definitions: 3,
            splits: 2,
            hosts: vec![("e", 1), ("", 1)],
        },
        Case {
            name: "plain host",
            body: format!("{a}{pair}{plain}{d}"),
            expected: &["1.", "2.", "3.", "4."],
            definitions: 1,
            splits: 0,
            hosts: vec![("plain", 1)],
        },
        Case {
            name: "trailing table",
            body: format!("{a}{pair}"),
            expected: &["1.", "2.", "3."],
            definitions: 1,
            splits: 0,
            hosts: vec![("", 1)],
        },
        Case {
            name: "nested numbered host",
            body: format!("{a}{}{plain}{f}", table(&[vec![format!("{b}{nested}{e}")]])),
            expected: &["1.", "2.", "3.", "4.", "5.", "6."],
            definitions: 4,
            splits: 3,
            hosts: vec![("plain", 1), ("e", 1)],
        },
        Case {
            name: "consecutive tables",
            body: format!("{a}{pair}{}{f}{g}", table(&[vec![d, e]])),
            expected: &["1.", "2.", "3.", "4.", "5.", "6.", "7."],
            definitions: 4,
            splits: 3,
            hosts: vec![("f", 2)],
        },
    ]
}
