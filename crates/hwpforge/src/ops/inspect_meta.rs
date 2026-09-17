//! The serde wire wrapper for [`inspect`](super::inspect::inspect).
//!
//! [`InspectOutput`] carries a typed payload beside a list of [`OpsWarning`]s,
//! which is the right shape for a Rust caller but not a wire shape: warnings
//! are an enum over peer types and do not derive serde. [`InspectMeta`] is the
//! serialisable projection — the report's own fields, flattened, plus the
//! warnings as [`WarningInfo`].
//!
//! It lives in its own module rather than beside [`InspectReport`] because the
//! report is the *payload* and the meta is the *envelope*; the same split
//! repeats for every other operation, where the envelope sits next to the
//! operation that produces it.
//!
//! # Why a wrapper at all
//!
//! [`InspectReport`] is already serialisable, so the envelope only adds
//! `warnings`. It is still a separate type on purpose: attaching `warnings` to
//! a payload DTO would change that DTO's wire schema for every other consumer
//! of it. The envelope keeps the payload's schema untouched.
//!
//! [`OpsWarning`]: super::OpsWarning

use hwpforge_foundation::diagnostics::WarningInfo;
use serde::Serialize;

use super::inspect::{InspectOutput, InspectReport};

/// The `inspect` wire payload: the report's fields plus `warnings`.
///
/// # Keys
///
/// `metadata`, `sections`, `paragraphs`, `tables`, `images`, `charts`,
/// `fields`, `section_details`, `warnings`, and `styles` only when
/// [`InspectOptions::with_styles`](super::InspectOptions::with_styles) asked
/// for it — the one conditional key, because [`InspectReport::styles`] keeps
/// its `skip_serializing_if` through the flatten.
///
/// `warnings` is always present, as a list, even when empty: a consumer's
/// type stub declares it unconditionally.
///
/// [`Deserialize`](serde::Deserialize) is deliberately not derived. It would
/// be derivable here, but no other operation's envelope can derive it (their
/// payload DTOs are `Serialize`-only), and an envelope that round-trips on
/// one operation and not the others is a worse contract than one that never
/// does. Parse the payload DTO directly if you need to read the wire form
/// back.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InspectMeta {
    /// The structural report, flattened into this object.
    #[serde(flatten)]
    pub report: InspectReport,
    /// Non-fatal diagnostics raised while decoding.
    pub warnings: Vec<WarningInfo>,
}

impl InspectOutput {
    /// The wire shape of this result.
    ///
    /// Clones the report, so it is a projection rather than a move: the
    /// output stays usable afterwards.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hwpforge::ops::{inspect, InspectOptions};
    ///
    /// let bytes = std::fs::read("document.hwpx")?;
    /// let meta = inspect(&bytes, &InspectOptions::default())?.meta();
    /// println!("{}", serde_json::to_string(&meta)?);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn meta(&self) -> InspectMeta {
        InspectMeta {
            report: self.report.clone(),
            warnings: self.warnings.iter().map(super::OpsWarning::info).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ops::{inspect, InspectOptions, OpsWarning};
    use hwpforge_smithy_hwpx::DecodeWarning;

    /// A one-paragraph package, built rather than loaded so that this test
    /// does not depend on a fixture file or on any DTO field list.
    fn minimal_package() -> Vec<u8> {
        use hwpforge_core::image::ImageStore;
        use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

        let paragraph = Paragraph::with_runs(
            vec![Run::text("본문", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        let mut document = Document::new();
        document.add_section(Section::with_paragraphs(vec![paragraph], PageSettings::a4()));
        let validated = document.validate().expect("validate");
        let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").expect("preset");
        hwpforge_smithy_hwpx::HwpxEncoder::encode(&validated, &styles, &ImageStore::default())
            .expect("encode")
    }

    #[test]
    fn warnings_become_their_wire_form() {
        let mut output = inspect(&minimal_package(), &InspectOptions::default()).expect("inspect");
        output.warnings.push(OpsWarning::Decode(DecodeWarning::UnknownEnumValue {
            attribute: "align",
            raw: "sideways".into(),
            fallback: "left",
        }));

        let meta = output.meta();

        assert_eq!(meta.warnings.len(), 1);
        assert_eq!(meta.warnings[0].code, "UNKNOWN_ENUM_VALUE");
        assert!(meta.warnings[0].message.contains("sideways"), "{}", meta.warnings[0].message);
    }

    #[test]
    fn meta_does_not_consume_the_output() {
        let output = inspect(&minimal_package(), &InspectOptions::default()).expect("inspect");

        let first = output.meta();
        let second = output.meta();

        assert_eq!(first.report.sections, second.report.sections);
        assert_eq!(output.report.sections, 1, "the output is still readable");
    }
}
