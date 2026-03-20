//! Tab property XML helpers for `header.xml`.

use hwpforge_core::{TabDef, TabStop};

use crate::error::{HwpxError, HwpxResult};
use crate::schema::header::HxTabItem;
use crate::style_store::HwpxStyleStore;

/// Builds `<hh:tabProperties>` XML from the store's effective tab definitions.
///
/// The emitted list always includes the 3 built-in Hancom definitions
/// (`id=0..=2`) and merges any explicit overrides/custom tabs from the store.
pub(super) fn build_tab_properties_xml(store: &HwpxStyleStore) -> HwpxResult<String> {
    let tabs = TabDef::merged_with_defaults(store.iter_tabs());

    let count = tabs.len();
    let mut xml = format!(r#"<hh:tabProperties itemCnt="{count}">"#);
    for tab in &tabs {
        let atl = u32::from(tab.auto_tab_left);
        let atr = u32::from(tab.auto_tab_right);
        if tab.stops.is_empty() {
            xml.push_str(&format!(
                r#"<hh:tabPr id="{}" autoTabLeft="{atl}" autoTabRight="{atr}"/>"#,
                tab.id,
            ));
            continue;
        }

        xml.push_str(&format!(
            r#"<hh:tabPr id="{}" autoTabLeft="{atl}" autoTabRight="{atr}">"#,
            tab.id,
        ));
        xml.push_str("<hp:switch>");
        xml.push_str(
            r#"<hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar">"#,
        );
        for stop in &tab.stops {
            xml.push_str(&build_tab_item_xml(stop, false)?);
        }
        xml.push_str("</hp:case>");
        xml.push_str("<hp:default>");
        for stop in &tab.stops {
            xml.push_str(&build_tab_item_xml(stop, true)?);
        }
        xml.push_str("</hp:default>");
        xml.push_str("</hp:switch>");
        xml.push_str("</hh:tabPr>");
    }
    xml.push_str("</hh:tabProperties>");
    Ok(xml)
}

fn build_tab_item(stop: &TabStop, legacy_default_units: bool) -> HwpxResult<HxTabItem> {
    let pos = if legacy_default_units {
        stop.position.as_i32().saturating_mul(2)
    } else {
        stop.position.as_i32()
    };
    let pos = u32::try_from(pos).map_err(|_| HwpxError::InvalidStructure {
        detail: format!(
            "tab stop position {} cannot be serialized as unsigned HWPX tabItem pos",
            stop.position.as_i32()
        ),
    })?;
    Ok(HxTabItem {
        pos,
        tab_type: stop.align.to_hwpx_str().to_string(),
        leader: stop.leader.as_hwpx_str().to_string(),
        unit: if legacy_default_units { String::new() } else { "HWPUNIT".to_string() },
    })
}

fn build_tab_item_xml(stop: &TabStop, legacy_default_units: bool) -> HwpxResult<String> {
    let item = build_tab_item(stop, legacy_default_units)?;
    if item.unit.is_empty() {
        Ok(format!(
            r#"<hh:tabItem pos="{}" type="{}" leader="{}"/>"#,
            item.pos, item.tab_type, item.leader,
        ))
    } else {
        Ok(format!(
            r#"<hh:tabItem pos="{}" type="{}" leader="{}" unit="{}"/>"#,
            item.pos, item.tab_type, item.leader, item.unit,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::build_tab_properties_xml;
    use super::*;
    use hwpforge_blueprint::registry::StyleRegistry;
    use hwpforge_blueprint::template::Template;
    use hwpforge_foundation::{HwpUnit, TabAlign, TabLeader};

    #[test]
    fn build_tab_properties_xml_merges_defaults_and_explicit_switch_items() {
        let mut store = HwpxStyleStore::new();
        store.push_tab(TabDef {
            id: 3,
            auto_tab_left: false,
            auto_tab_right: false,
            stops: vec![TabStop {
                position: HwpUnit::new(15000).unwrap(),
                align: TabAlign::Left,
                leader: TabLeader::from_hwpx_str("DASH"),
            }],
        });

        let xml = build_tab_properties_xml(&store).unwrap();

        assert!(xml.contains(r#"<hh:tabProperties itemCnt="4">"#));
        assert!(xml.contains(r#"<hh:tabPr id="0" autoTabLeft="0" autoTabRight="0"/>"#));
        assert!(xml.contains(r#"<hh:tabPr id="1" autoTabLeft="1" autoTabRight="0"/>"#));
        assert!(xml.contains(r#"<hh:tabPr id="2" autoTabLeft="0" autoTabRight="1"/>"#));
        assert!(xml.contains(r#"<hh:tabPr id="3" autoTabLeft="0" autoTabRight="0">"#));
        assert!(xml.contains(
            r#"<hp:case hp:required-namespace="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar"><hh:tabItem pos="15000" type="LEFT" leader="DASH" unit="HWPUNIT"/></hp:case>"#
        ));
        assert!(xml.contains(
            r#"<hp:default><hh:tabItem pos="30000" type="LEFT" leader="DASH"/></hp:default>"#
        ));
    }

    #[test]
    fn build_tab_properties_xml_from_template_tabs_reaches_header() {
        let yaml = r#"
meta:
  name: tabbed
styles:
  body:
    char_shape:
      font: 한컴바탕
      size: 10pt
    para_shape:
      tab_def_id: 3
tabs:
  - id: 3
    auto_tab_left: false
    auto_tab_right: false
    stops:
      - position: 75pt
        align: Left
        leader: DASH
"#;
        let template = Template::from_yaml(yaml).unwrap();
        let registry = StyleRegistry::from_template(&template).unwrap();
        let store = HwpxStyleStore::from_registry(&registry).unwrap();

        let xml = build_tab_properties_xml(&store).unwrap();

        assert!(xml.contains(r#"<hh:tabPr id="3" autoTabLeft="0" autoTabRight="0">"#));
        assert!(
            xml.contains(r#"<hh:tabItem pos="7500" type="LEFT" leader="DASH" unit="HWPUNIT"/>"#)
        );
        assert!(xml.contains(
            r#"<hp:default><hh:tabItem pos="15000" type="LEFT" leader="DASH"/></hp:default>"#
        ));
    }

    #[test]
    fn build_tab_properties_xml_rejects_negative_tab_stop_positions() {
        let mut store = HwpxStyleStore::new();
        store.push_tab(TabDef {
            id: 3,
            auto_tab_left: false,
            auto_tab_right: false,
            stops: vec![TabStop {
                position: HwpUnit::new(-100).unwrap(),
                align: TabAlign::Left,
                leader: TabLeader::none(),
            }],
        });

        let err = build_tab_properties_xml(&store).unwrap_err();
        assert!(matches!(err, HwpxError::InvalidStructure { .. }));
    }
}
