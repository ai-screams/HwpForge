//! Original-package layout proofs and reusable cache invalidation.
//!
//! # Three boundaries
//!
//! 1. [`PackageAnalysis::read`] reads original package XML and resolves definition,
//!    binary and chart dependencies. The original is never decoded/re-encoded to
//!    manufacture a baseline. [`PackageAnalysis::read_emitted`] consumes actual
//!    encoder bytes plus [`crate::HwpxEncoder::encode_with_origins`]'s origin map.
//! 2. [`ParagraphCorrespondence`] pairs original paragraphs with emitted Core
//!    origins. The JSON adapter uses typed Core lists: equal lengths pair by
//!    position, changed lengths admit only the identical prefix. Paragraph ids
//!    and text are never identities. Origins are recorded at paragraph emission,
//!    reordered by final XML assembly, and stripped of private markers before ZIP
//!    packaging. Synthetic/ambiguous origins have no trusted correspondence.
//! 3. [`plan_invalidation`] combines package differences with a [`ChangeSet`],
//!    returning per-paragraph and per-section **reasons**. Flow tails, host chains,
//!    floating objects, keep/widow links, numbering/page dependencies, unknown
//!    controls and unresolved references all use this one policy. JSON only
//!    selects trusted base caches and applies the result. Typed Core/style checks
//!    are supplemental rejection evidence, never sufficient for preservation.
//!
//! # Original-byte emission
//!
//! `encode_with_base` first validates measure output, including odd physical
//! line spacing. Inputs needing measure normalization cannot return original bytes.
//! With a resolved style 0, it compares the complete typed Core (excluding supplied
//! caches), style store and image bytes with a fresh decode of the base. If all
//! are unchanged, it returns the **entire original ZIP byte for byte**. This is
//! a separate emission strategy, not a bypass attaching caches to regenerated
//! XML. It preserves `sz`, `zOrder`, offsets, matrices, shape comments, unknown
//! definitions and decoder-unrepresented content together with their caches.
//! No ledger translation is needed because no wire token moves. Original invalid
//! caches are not certified or repaired by this identity operation. Decoder
//! content-loss diagnostics still describe the Core projection, not output loss.
//! Any semantic edit falls through to the fingerprint/ledger proof below;
//! identical sections alone are insufficient because dependencies cross sections.
//! `memo_create_time` supplies a default only when a memo is actually written.
//!
//! # Canonical form
//!
//! * Names use namespace URI plus local name. Declarations/prefix spelling,
//!   attribute order, comments, XML declaration and empty-tag spelling disappear.
//!   Unbound prefixes and DTDs fail the analysis rather than weaken the proof.
//! * Attributes are sorted by expanded name. Character/entity/CDATA spellings
//!   become decoded XML 1.0 text. Structural whitespace-only text is ignored;
//!   text-bearing `t`, `stringParam`, `script`, `meta`, `title`, mixed content and
//!   inherited `xml:space="preserve"` keep exact spaces and child-boundary positions.
//! * Child order and nonempty structure remain significant. No guessed default
//!   values, numeric coercions, font substitutions or compatibility equivalences
//!   are admitted. `linesegarray` is evidence being validated, never an input.
//! * Definition ids are replaced by resolved, typed targets (font ids also have
//!   a language scope). Reference attribute names/order and rooted cycle edges
//!   remain significant. SCC traversal ordinals and component boundaries replace
//!   raw ids; duplicate/missing definitions and cycles over 256 members fail closed.
//!   Unreferenced paragraph/object ids do not affect layout; other nonresolved
//!   link attributes retain their values, so unfamiliar links remain conservative.
//!   Local fieldBegin ids/beginIDRef pairs use definition-order ordinals; changed
//!   targets remain different, missing/duplicate begins are unresolved. Other
//!   field attributes (including fieldid's type-specific magic values) stay exact.
//! * Header properties outside `refList` are global layout inputs, including
//!   compatibility. `head@secCnt` is derived package structure, already represented
//!   by section graphs. Referenced definitions contribute their entire XML.
//! * `content.hpf` metadata is a document input.
//!   `fieldBegin`, page fields and `$`-bearing `stringParam` read these inputs;
//!   a changed input invalidates their conservative dependent flows. BinData
//!   references resolve through manifest ids to target bytes; chart paths resolve
//!   to canonical target XML. Missing targets never prove equality.
//!   Package input classification and its evidence table live together in
//!   `layout_edit/inputs.rs`: packaging provenance and known caret coordinates
//!   are excluded; print settings and unknown package parts are global inputs.
//! * Decode/encode `ContentLost` seeds section or document invalidation. Unknown
//!   loss scope is document-wide. This deliberately includes non-layout losses
//!   until a narrower, proven classification exists.
//!
//! # Adding an editor
//!
//! Read the original package once, encode with origins if re-emitting, and analyze
//! those emitted bytes. Supply proven paragraph pairs (or `identity` for a
//! structure-preserving editor) and add explicit edits/losses to `ChangeSet`.
//! Call `plan_invalidation`; apply every nonempty paragraph reason set to the
//! original cache store. Never implement another floating/keep/numbering policy.
//! Paths are the existing value-only `ParagraphPath(Vec<PathSeg>)` (section,
//! paragraph and nested indices); an adapter can map these to the shared fbody
//! path without changing graph policy. Unbounded/ambiguous containers close the
//! affected section. Consumers must still validate carried text positions with
//! the encoder's wire ledger; this module proves layout inputs, not cache syntax.
//!
//! Diagnostic facts name changed XML attributes/elements; each affected paragraph
//! receives each cause once, including propagated causes. Fingerprints retain
//! ordering even when the diagnostic fact sets have no more specific difference.

use hwpforge_core::{Document, Draft, Validated};
use serde_json::Value;

use crate::decoder::{ParagraphPath, PathSeg};
use crate::{EncodeWarning, HwpxDocument, HwpxError, HwpxResult, HwpxStyleStore};

fn tree<T: serde::Serialize>(value: &T) -> HwpxResult<Value> {
    serde_json::to_value(value).map_err(|e| HwpxError::InvalidStructure { detail: e.to_string() })
}

fn paragraph(value: &Value) -> bool {
    value.get("runs").is_some_and(Value::is_array)
        && value.get("para_shape_id").is_some_and(Value::is_u64)
}

/// Comparison uses the typed Core serialization, never the caller's JSON.
/// Only typed paragraph objects lose their cache field. Metadata.extras values
/// are strings, even when users name them runs, para_shape_id or layout_cache.
fn clean(value: &mut Value) {
    let is_paragraph = paragraph(value);
    match value {
        Value::Object(fields) => {
            if is_paragraph {
                fields.remove("layout_cache");
            }
            for child in fields.values_mut() {
                clean(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(clean),
        _ => {}
    }
}

/// Container geometry and content outside nested paragraph lists. A cell's
/// text change does not change its siblings' geometry; a table width change
/// does. Lists themselves are handled by the paragraph correspondence pass.
fn shell(value: &Value) -> Value {
    match value {
        Value::Array(items) if items.first().is_some_and(paragraph) => Value::Array(Vec::new()),
        Value::Array(items) => Value::Array(items.iter().map(shell).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(k, _)| !paragraph(value) || k.as_str() != "layout_cache")
                .map(|(k, v)| (k.clone(), shell(v)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

// Legacy provenance checks are rejection-only. Emitted fingerprints are
// mandatory even when this supplemental check admits a Core reference.
mod styles;
use styles::Styles;
mod wire;
use wire::PackageAnalysis as Wire;
mod planner;
pub use planner::{plan_invalidation, ChangeSet, InvalidationPlan, ParagraphCorrespondence};
pub use wire::{DependencyKey, PackageAnalysis};

mod correspondence;
use correspondence::CoreCorrespondence;

fn supplied_without_base(original: &Value, edited: &Value) -> bool {
    match edited {
        Value::Object(fields) => {
            (paragraph(edited)
                && fields.get("layout_cache").is_some_and(|v| !v.is_null())
                && original.get("layout_cache").is_none_or(Value::is_null))
                || fields.iter().any(|(k, v)| {
                    (!paragraph(edited) || k != "layout_cache")
                        && supplied_without_base(&original[k], v)
                })
        }
        Value::Array(items) => items
            .iter()
            .enumerate()
            .any(|(i, v)| supplied_without_base(original.get(i).unwrap_or(&Value::Null), v)),
        _ => false,
    }
}

/// A different proof from emitted-XML equality: the encoder emits the *entire*
/// original package. Equality of the typed edit surface authorizes that choice;
/// it never authorizes attaching original caches to regenerated XML. Compare
/// metadata and every nested container as well as definitions and target bytes.
/// Unknown/decode-lost content remains in the original ZIP, so it is not lost
/// by this write. Deliberately do not generalize this to individual sections:
/// counters, floating objects, fields and unknown controls can cross boundaries.
pub(crate) fn unchanged(
    base: &HwpxDocument,
    document: &Document<Validated>,
    styles: &HwpxStyleStore,
    images: &hwpforge_core::image::ImageStore,
) -> HwpxResult<bool> {
    crate::paragraph_measure::validate_measure_output(styles)?;
    if base.requires_header_normalization() {
        return Ok(false);
    }
    if crate::header_references::HeaderReferences::build(styles)?.style_index(0).is_err() {
        return Ok(false);
    }
    if &base.style_store != styles || &base.image_store != images {
        return Ok(false);
    }
    let mut original = tree(&base.document)?;
    let mut edited = tree(document)?;
    // The ordinary path supplies the existing diagnostic for a cache forged
    // where the base had none. It will still never copy that supplied cache.
    if supplied_without_base(&original, &edited) {
        return Ok(false);
    }
    clean(&mut original);
    clean(&mut edited);
    Ok(original == edited)
}

/// Returns a Core copy whose only paragraph caches came from the base and
/// survived the shared flow invalidation rule. Hand-edited JSON caches are
/// ignored even if they look plausible; deleting a JSON cache does not revoke
/// an unchanged base cache either.
pub(crate) fn prepare(
    base_bytes: &[u8],
    base: &HwpxDocument,
    document: &Document<Validated>,
    style_store: &HwpxStyleStore,
    images: &hwpforge_core::image::ImageStore,
    options: crate::EncodeOptions,
) -> HwpxResult<(Document<Validated>, Vec<EncodeWarning>, crate::EncodeOutcome)> {
    let trusted = tree(&base.document)?;
    let supplied = tree(document)?;
    let mut original = trusted.clone();
    let mut edited = supplied.clone();
    clean(&mut original);
    clean(&mut edited);
    // The cache was computed against these original bytes, not encode(decode(base)).
    let (output_xml, origins) = crate::HwpxEncoder::encode_with_origins(
        document,
        style_store,
        images,
        options.with_emit_layout_cache(false),
    )?;
    let base_wire = Wire::read(base_bytes)?;
    let edited_wire = Wire::read_emitted(&output_xml.bytes, &origins)?;
    let base_styles = styles::cache_tree(&base.style_store)?;
    let edited_styles = styles::cache_tree(style_store)?;
    let styles = Styles::new(&base_styles, &edited_styles);
    let mut plan = CoreCorrespondence::default();
    let old = original["sections"].as_array().expect("Core sections");
    let sections = edited["sections"].as_array().expect("Core sections");
    if old.len() != sections.len() {
        plan.warnings.push(EncodeWarning::LayoutCacheDropped {
            path: ParagraphPath(vec![PathSeg::Section(0)]),
            reason: "section structure changed; only the identical prefix has base correspondence"
                .into(),
        });
    }
    let prefix = if old.len() == sections.len() {
        sections.len()
    } else {
        old.iter().zip(sections).take_while(|(a, b)| a == b).count()
    };
    let mut changes = ChangeSet::default();
    for warning in &base.warnings {
        if let crate::DecodeWarning::ContentLost { path, kind, .. } = warning {
            let reason = format!("ContentLost:{kind}");
            if let Some(PathSeg::Section(s)) = path.0.first() {
                changes.sections.entry(*s).or_default().insert(reason);
            } else {
                changes.document.insert(reason);
            }
        }
    }
    for warning in &output_xml.warnings {
        if let EncodeWarning::ContentLost { path, kind, .. } = warning {
            let reason = format!("ContentLost:{kind}");
            if let Some(PathSeg::Section(s)) = path.0.first() {
                changes.sections.entry(*s).or_default().insert(reason);
            } else {
                changes.document.insert(reason);
            }
        }
    }
    for (s, section) in sections.iter().enumerate() {
        plan.walk(
            old.get(s).filter(|_| s < prefix),
            section,
            &trusted["sections"][s],
            &supplied["sections"][s],
            &format!("/sections/{s}"),
            &[PathSeg::Section(s)],
            None,
            s >= prefix,
            &styles,
        );
        plan.masterpage_offset += section["master_pages"].as_array().map_or(0, Vec::len);
    }
    for &i in &plan.changed {
        changes
            .paragraph(plan.candidates[i].path.clone(), "Core edit/provenance or XML dependency");
    }
    let correspondence = ParagraphCorrespondence {
        pairs: plan
            .candidates
            .iter()
            .map(|p| ((!p.unmatched).then(|| p.path.clone()), p.path.clone()))
            .collect(),
        para_list: plan.para_list,
        list_host: plan.list_host,
    };
    let invalidation = plan_invalidation(&base_wire, &edited_wire, &correspondence, &changes);
    for (candidate, reasons) in plan.candidates.into_iter().zip(invalidation.paragraphs) {
        let stale = !reasons.is_empty();
        if stale || candidate.cache.is_none() {
            if candidate.cache.is_some() || candidate.supplied_cache {
                plan.warnings.push(EncodeWarning::LayoutCacheDropped {
                    path: candidate.path,
                    reason: if stale {
                        format!(
                            "intentionally invalidated: {}",
                            reasons.into_iter().collect::<Vec<_>>().join("; ")
                        )
                    } else {
                        "no trusted base cache; supplied layout_cache is not trusted".into()
                    },
                });
            }
        } else if let Some(cache) = candidate.cache {
            // 이것을 실패시키는 것: base 캐시 대신 JSON의 layout_cache를 복사하기.
            edited
                .pointer_mut(&candidate.pointer)
                .expect("Core paragraph pointer")
                .as_object_mut()
                .expect("Core paragraph")
                .insert("layout_cache".into(), cache);
        }
    }
    let mut draft: Document<Draft> = serde_json::from_value(edited)
        .map_err(|e| HwpxError::InvalidStructure { detail: e.to_string() })?;
    // Cache selection only changes layout_cache. Restore the source evidence
    // omitted by JSON, then verify it against this operation's independent base.
    crate::HwpxDecoder::restore_source_provenance(&mut draft, base)?;
    Ok((draft.validate()?, plan.warnings, output_xml))
}

#[cfg(test)]
mod cache_boundary_tests {
    use super::*;
    use serde_json::json;

    // 이것을 실패시키는 것: shell/clean/supplied_without_base 중 어느 하나라도
    // 문단 판정 없이 layout_cache 이름을 특별 취급하거나 문자열 extras를 문단으로 오인하기.
    #[test]
    fn only_typed_paragraph_cache_is_special() {
        let metadata = json!({"layout_cache":"old", "runs":"user", "para_shape_id":"user"});
        assert_eq!(shell(&metadata), metadata);
        let mut copy = metadata.clone();
        clean(&mut copy);
        assert_eq!(copy, metadata);
        assert!(!supplied_without_base(&Value::Null, &metadata));
        let paragraph = json!({"runs":[], "para_shape_id":0, "layout_cache":{"lines":[]}});
        assert!(supplied_without_base(&Value::Null, &paragraph));
        let mut cleaned = paragraph.clone();
        clean(&mut cleaned);
        assert!(cleaned.get("layout_cache").is_none());
        assert!(shell(&paragraph).get("layout_cache").is_none());
        // Recursive containers must still detect forged nested paragraph caches.
        assert!(supplied_without_base(&Value::Null, &json!({"children":[paragraph]})));
    }
}
