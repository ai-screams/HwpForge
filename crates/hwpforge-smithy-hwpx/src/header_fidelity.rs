//! Private HWPX header scalars absent from the shared Core model. Kept in
//! HwpxStyleStore's existing private storage, including its JSON exchange.
//! No fields or signatures are added to the published Core/para/head types.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::header::{HxBulletParaHead, HxRefList};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub(crate) struct HeaderFidelity {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    /// One-based definition position -> head level -> metrics.
    pub numbering: BTreeMap<u32, BTreeMap<u32, HeadMetrics>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    /// One-based definition position -> head level -> metrics.
    pub bullet: BTreeMap<u32, BTreeMap<u32, HeadMetrics>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub(crate) struct HeadMetrics {
    pub align: String,
    pub use_inst_width: u32,
    pub auto_indent: u32,
    pub width_adjust: u32,
    pub text_offset_type: String,
    pub text_offset: u32,
    pub char_pr_id_ref: u32,
}

impl HeadMetrics {
    pub fn generated(use_inst_width: u32) -> Self {
        Self {
            align: "LEFT".into(),
            use_inst_width,
            auto_indent: 1,
            width_adjust: 0,
            text_offset_type: "PERCENT".into(),
            text_offset: 50,
            char_pr_id_ref: u32::MAX,
        }
    }

    pub fn apply(&self, head: &mut HxBulletParaHead) {
        head.align.clone_from(&self.align);
        head.use_inst_width = self.use_inst_width;
        head.auto_indent = self.auto_indent;
        head.width_adjust = self.width_adjust;
        head.text_offset_type.clone_from(&self.text_offset_type);
        head.text_offset = self.text_offset;
        head.char_pr_id_ref = self.char_pr_id_ref;
    }
}

impl HeaderFidelity {
    pub fn is_empty(&self) -> bool {
        self.numbering.is_empty() && self.bullet.is_empty()
    }

    pub fn capture(&mut self, refs: &HxRefList) {
        // Keys are one-based definition positions, independent of public id labels.
        // JSON editors that move/delete definitions must move/delete these entries too.
        macro_rules! capture_heads {
            ($defs:expr, $target:expr, $default:expr) => {
                if let Some(defs) = $defs {
                    for (index, def) in defs.items.iter().enumerate() {
                        for head in &def.para_heads {
                            let metrics = HeadMetrics {
                                align: head.align.clone(),
                                use_inst_width: head.use_inst_width,
                                auto_indent: head.auto_indent,
                                width_adjust: head.width_adjust,
                                text_offset_type: head.text_offset_type.clone(),
                                text_offset: head.text_offset,
                                char_pr_id_ref: head.char_pr_id_ref,
                            };
                            if metrics != HeadMetrics::generated($default) {
                                $target
                                    .entry(index as u32 + 1)
                                    .or_default()
                                    .insert(head.level, metrics);
                            }
                        }
                    }
                }
            };
        }
        capture_heads!(&refs.numberings, self.numbering, 1);
        capture_heads!(&refs.bullets, self.bullet, 0);
    }
}
