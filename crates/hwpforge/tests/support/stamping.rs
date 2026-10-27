#![allow(dead_code)]

use hwpforge::hwpx::stamp::{StampAction, StampMap, StampSpec};
use hwpforge::ops::stamp::{stamp, stamp_plan, StampOptions};

pub(crate) fn stamp_all(
    bytes: &[u8],
) -> Result<hwpforge::ops::stamp::StampOutput, hwpforge::ops::OpsError> {
    let specs: Vec<StampSpec> = stamp_plan(bytes)
        .expect("plan")
        .plan
        .text
        .into_iter()
        .map(|candidate| StampSpec {
            section: candidate.section,
            path: candidate.path,
            span: candidate.span,
            marker: candidate.marker.clone(),
            action: StampAction::Field { name: "소속".into(), hint: None },
        })
        .collect();
    assert!(!specs.is_empty(), "the paren blank is a stamp candidate");
    stamp(bytes, &StampMap::Legacy(specs), &StampOptions::default())
}
