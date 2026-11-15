//! Fixed strings observed from numbered paragraphs, independent of either counter implementation.
#![allow(dead_code)]
pub struct Definition {
    pub starts: &'static [u32],
    pub texts: &'static [&'static str],
    pub formats: &'static [&'static str],
}
pub struct Case {
    pub name: &'static str,
    pub definitions: &'static [Definition],
    /// One-based definition position and zero-based level. A zero definition is an ordinary paragraph.
    pub uses: &'static [(usize, usize)],
    pub expected: Option<&'static [&'static str]>,
}
const DECIMAL: Definition = Definition { starts: &[1], texts: &["^1."], formats: &["Digit"] };
const HANGUL: Definition =
    Definition { starts: &[1, 1], texts: &["^1.", "^2."], formats: &["Digit", "HangulSyllable"] };
const COMPOUND: Definition =
    Definition { starts: &[1, 1], texts: &["^1.", "^1.^2"], formats: &["Digit", "Digit"] };
pub const CASES: &[Case] = &[
    Case {
        name: "zero start displays one",
        definitions: &[Definition { starts: &[0], ..DECIMAL }],
        uses: &[(1, 0), (1, 0), (1, 0)],
        expected: Some(&["1.", "2.", "3."]),
    },
    Case {
        name: "parent resets child",
        definitions: &[HANGUL],
        uses: &[(1, 0), (1, 1), (1, 1), (1, 0), (1, 1)],
        expected: Some(&["1.", "가.", "나.", "2.", "가."]),
    },
    Case {
        name: "parent resets child to its start",
        definitions: &[Definition { starts: &[1, 3], ..HANGUL }],
        uses: &[(1, 0), (1, 1), (1, 1), (1, 0), (1, 1)],
        expected: Some(&["1.", "다.", "라.", "2.", "다."]),
    },
    Case {
        name: "other definition leaves child state",
        definitions: &[HANGUL, Definition { starts: &[2], ..DECIMAL }],
        uses: &[(1, 0), (1, 1), (1, 1), (2, 0), (1, 1), (1, 0)],
        expected: Some(&["1.", "가.", "나.", "2.", "다.", "2."]),
    },
    Case {
        name: "returning definition keeps own state",
        definitions: &[DECIMAL, Definition { starts: &[3], ..DECIMAL }],
        uses: &[(1, 0), (1, 0), (2, 0), (1, 0)],
        expected: Some(&["1.", "2.", "3.", "3."]),
    },
    Case {
        name: "successive definitions keep fixed starts",
        definitions: &[
            DECIMAL,
            Definition { starts: &[3], ..DECIMAL },
            Definition { starts: &[4], ..DECIMAL },
        ],
        uses: &[(1, 0), (1, 0), (2, 0), (3, 0)],
        expected: Some(&["1.", "2.", "3.", "4."]),
    },
    Case {
        name: "ordinary paragraphs do not reset",
        definitions: &[DECIMAL],
        uses: &[(1, 0), (0, 0), (0, 0), (0, 0), (1, 0)],
        expected: Some(&["1.", "2."]),
    },
    Case {
        name: "roman parent in compound child",
        definitions: &[Definition { formats: &["RomanCapital", "Digit"], ..COMPOUND }],
        uses: &[(1, 0), (1, 1), (1, 1)],
        expected: Some(&["I.", "I.1", "I.2"]),
    },
    Case {
        name: "body and table cells",
        definitions: &[DECIMAL],
        uses: &[(1, 0), (1, 0), (1, 0), (1, 0), (1, 0), (1, 0)],
        expected: Some(&["1.", "2.", "3.", "4.", "5.", "6."]),
    },
    Case {
        name: "body and nested paragraphs",
        definitions: &[DECIMAL],
        uses: &[(1, 0), (1, 0), (1, 0)],
        expected: Some(&["1.", "2.", "3."]),
    },
    Case {
        name: "section boundary",
        definitions: &[DECIMAL],
        uses: &[(1, 0), (1, 0), (1, 0), (1, 0)],
        expected: Some(&["1.", "2.", "3.", "4."]),
    },
    Case {
        name: "child initializes implicit parent",
        definitions: &[Definition { texts: &["^1.", "^2)"], ..COMPOUND }],
        uses: &[(1, 1), (1, 1), (1, 0), (1, 1)],
        expected: Some(&["1)", "2)", "2.", "1)"]),
    },
    Case {
        name: "compound child initializes implicit parent",
        definitions: &[COMPOUND],
        uses: &[(1, 1), (1, 1), (1, 0), (1, 1)],
        expected: Some(&["1.1", "1.2", "2.", "2.1"]),
    },
    Case {
        name: "nonunit starts with child first",
        definitions: &[Definition { starts: &[2, 3], ..COMPOUND }],
        uses: &[(1, 1), (1, 0), (1, 1)],
        expected: Some(&["2.3", "3.", "3.3"]),
    },
    Case {
        name: "literal head",
        definitions: &[Definition { texts: &["0."], ..DECIMAL }],
        uses: &[(1, 0), (1, 0)],
        expected: Some(&["0.", "0."]),
    },
    Case {
        name: "decimal width transition",
        definitions: &[Definition { starts: &[99], ..DECIMAL }],
        uses: &[(1, 0), (1, 0), (1, 0)],
        expected: Some(&["99.", "100.", "101."]),
    },
    Case {
        name: "skipped parent is unconfirmed",
        definitions: &[Definition {
            starts: &[1, 1, 1],
            texts: &["^1.", "^2.", "^3."],
            formats: &["Digit", "Digit", "Digit"],
        }],
        uses: &[(1, 0), (1, 2)],
        expected: None,
    },
    Case {
        name: "deep segment start is unconfirmed",
        definitions: &[Definition {
            starts: &[1, 1, 1],
            texts: &["^1.", "^2.", "^3."],
            formats: &["Digit", "Digit", "Digit"],
        }],
        uses: &[(1, 0), (1, 1), (1, 2), (1, 2)],
        expected: None,
    },
    Case {
        name: "shared caption and cell is unconfirmed",
        definitions: &[DECIMAL],
        uses: &[(1, 0), (1, 0)],
        expected: None,
    },
];
