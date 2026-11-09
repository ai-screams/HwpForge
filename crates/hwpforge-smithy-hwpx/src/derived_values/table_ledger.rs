//! Table ownership used only when checking stored derived values.
use super::{require, HwpxResult};
use crate::cache_flow::ParagraphFlow;
use hwpforge_core::source_xml::XmlSource;
use std::ops::Range;

#[derive(Debug, Default)]
pub(super) struct TableLedger {
    pub(super) tables: Vec<FlowTable>,
    table_owner: Vec<Option<usize>>,
    paragraph_boundary: Vec<usize>,
}

/// Original table identity and its flow/containment edges from decoder records.
#[derive(Debug)]
pub(super) struct FlowTable {
    pub(super) outer: Range<usize>,
    pub(super) host: Option<usize>,
    pub(super) parent: Option<usize>,
    pub(super) boundary: usize,
}

impl TableLedger {
    /// Reuse the decision's parsed source; never attach decision storage to decoding.
    pub(super) fn from_source(source: &XmlSource, flow: &ParagraphFlow) -> HwpxResult<Self> {
        let mut ledger = Self::default();
        // Only active containment ranges are needed, including paragraph hosts.
        let mut scopes = vec![(usize::MAX, None, 0, None)];
        for (id, element) in source.elements().iter().enumerate().skip(1) {
            while scopes.last().unwrap().0 <= element.outer().start {
                scopes.pop();
            }
            let (_, owner, boundary, host) = *scopes.last().unwrap();
            match element.name() {
                "header" | "footer" => {
                    scopes.push((element.outer().end, None, id, None));
                }
                "tbl" => {
                    let table = ledger.tables.len();
                    ledger.tables.push(FlowTable {
                        outer: element.outer(),
                        host,
                        parent: owner,
                        boundary,
                    });
                    scopes.push((element.outer().end, Some(table), boundary, host));
                }
                "p" => {
                    let p = ledger.table_owner.len();
                    require(flow.span(p) == Some(element.outer()))?;
                    ledger.table_owner.push(owner);
                    ledger.paragraph_boundary.push(boundary);
                    scopes.push((element.outer().end, owner, boundary, Some(p)));
                }
                _ => {}
            }
        }
        require(ledger.table_owner.len() == flow.paragraph_count())?;
        Ok(ledger)
    }

    pub(super) fn table_owner(&self, p: usize) -> Option<usize> {
        self.table_owner.get(p).copied().flatten()
    }

    pub(super) fn affected_tables(&self, flow: &ParagraphFlow, seeds: &[bool]) -> Vec<bool> {
        let mut stale = flow.affected_paragraphs(seeds);
        // Dependent containers inherit their stale host; independent flows stop it.
        for p in 0..stale.len() {
            crate::work::tick();
            if let Some(parent) = flow.parent(p) {
                stale[p] |=
                    stale[parent] && self.paragraph_boundary[p] == self.paragraph_boundary[parent];
            }
        }
        let body_seed =
            seeds.iter().enumerate().any(|(p, &s)| s && self.paragraph_boundary[p] == 0);
        let mut affected = vec![false; self.tables.len()];
        for (t, table) in self.tables.iter().enumerate() {
            crate::work::tick();
            affected[t] |= table.host.map_or(table.boundary == 0 && body_seed, |p| stale[p]);
        }
        affected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 이것을 실패시키는 것: 닫힌 표/독립 흐름의 소유 상태 누출, 중첩 부모·host·boundary 손실.
    #[test]
    fn table_ownership_matches_ancestor_boundaries() {
        use hwpforge_core::source_xml::XmlSource;
        for depth in [0, 1, 4, 12] {
            let mut nested = "<p><run><t>칸</t></run></p>".to_owned();
            for _ in 0..depth {
                nested = format!("<p><run><tbl><tr><tc><subList>{nested}</subList></tc></tr></tbl><tbl/></run></p>");
            }
            let xml = format!("<sec><p><run><tbl><tr><tc><subList>{nested}<p><run><header><subList>{nested}</subList></header><footer><subList>{nested}</subList></footer><tbl/></run></p></subList></tc></tr></tbl><tbl/>{nested}</run></p><p><run><tbl/></run></p></sec>");
            let source = XmlSource::read(&xml).unwrap().1;
            let flow = ParagraphFlow::from_source(&source);
            let ledger = TableLedger::from_source(&source, &flow).unwrap();
            let elements = source.elements();
            let table_ids: Vec<_> = elements
                .iter()
                .enumerate()
                .filter(|(_, e)| e.name() == "tbl")
                .map(|(id, _)| id)
                .collect();
            let paragraph_ids: Vec<_> = elements
                .iter()
                .enumerate()
                .filter(|(_, e)| e.name() == "p")
                .map(|(id, _)| id)
                .collect();
            for (id, element) in
                elements.iter().enumerate().filter(|(_, e)| matches!(e.name(), "p" | "tbl"))
            {
                // Independent oracle walks ancestors, without the production range stack.
                let mut at = element.parent();
                let (mut owner, mut host, mut boundary) = (None, None, 0);
                while at != 0 {
                    match elements[at].name() {
                        "header" | "footer" => {
                            boundary = at;
                            break;
                        }
                        "tbl" if owner.is_none() => owner = table_ids.iter().position(|&i| i == at),
                        "p" if host.is_none() => host = paragraph_ids.iter().position(|&i| i == at),
                        _ => {}
                    }
                    at = elements[at].parent();
                }
                if element.name() == "p" {
                    let p = paragraph_ids.iter().position(|&i| i == id).unwrap();
                    assert_eq!(ledger.table_owner(p), owner);
                    assert_eq!(ledger.paragraph_boundary[p], boundary);
                } else {
                    let t = table_ids.iter().position(|&i| i == id).unwrap();
                    let record = &ledger.tables[t];
                    assert_eq!(record.outer, element.outer());
                    assert_eq!(record.parent, owner);
                    assert_eq!(record.host, host);
                    assert_eq!(record.boundary, boundary);
                }
            }
        }
    }

    // 이것을 실패시키는 것: 다른 원문의 문단 주소나 누락된 문단을 같은 flow로 승인.
    #[test]
    fn table_ledger_requires_matching_paragraph_ranges() {
        let source = XmlSource::read("<sec><p/><p/></sec>").unwrap().1;
        let flow = ParagraphFlow::from_source(&source);
        assert!(TableLedger::from_source(&source, &flow).is_ok());
        for xml in ["<sec> <p/><p/></sec>", "<sec><p/></sec>", "<sec><p/><p/><p/></sec>"] {
            let other = XmlSource::read(xml).unwrap().1;
            assert!(TableLedger::from_source(&other, &flow).is_err(), "{xml}");
        }
    }
}
