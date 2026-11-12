//! Input decode loss can be tolerated only with decoder-owned preservation evidence.
use crate::decoder::preserved_loss::{LossCause, LossLocation};
use crate::raw_package::RawPackage;
use crate::{DecodeWarning, HwpxDocument, HwpxError, HwpxResult};

fn invalid(detail: &str) -> HwpxError {
    HwpxError::InvalidStructure { detail: detail.into() }
}

fn require(proven: bool) -> HwpxResult<()> {
    if proven {
        Ok(())
    } else {
        Err(invalid("unproven input decode loss"))
    }
}

pub(super) fn check_candidates(
    base: &[u8],
    package: &RawPackage,
    decoded: &HwpxDocument,
) -> HwpxResult<()> {
    let basis = decoded.source_basis.as_ref().ok_or_else(|| invalid("missing input source"))?;
    require(basis.matches_package(base))?;
    // The decoder emits evidence in warning order, including group retries and
    // section offsets. Consume both streams once; equality also rejects gaps,
    // duplicate indices and evidence attached to a non-loss warning.
    let mut losses = basis.losses.iter();
    use sha2::Digest;
    let header_digest: [u8; 32] =
        sha2::Sha256::digest(package.read_entry(crate::encoder::package::HEADER_PART)?).into();
    let mut section_paths = std::collections::HashMap::<String, usize>::new();
    for section in 0..decoded.source_maps.len() {
        *section_paths.entry(package.section_path(section)?).or_default() += 1;
    }
    let mut section_properties = std::collections::HashSet::new();
    for (index, warning) in decoded.warnings.iter().enumerate() {
        crate::work::tick();
        if !warning.is_content_loss() {
            continue;
        }
        let evidence = losses.next().ok_or_else(|| invalid("unproven input decode loss"))?;
        crate::work::tick();
        require(evidence.warning == index)?;
        let DecodeWarning::ContentLost { part, kind, .. } = warning else { return require(false) };
        require(kind == evidence.cause.kind())?;
        match (&evidence.cause, &evidence.location) {
            (LossCause::PerLanguageScalar, LossLocation::Header { definition, digest }) => {
                require(part == crate::encoder::package::HEADER_PART)?;
                require((*definition as usize) < decoded.style_store.char_shape_count())?;
                require(*digest == header_digest)?;
            }
            (
                LossCause::DrawnExtentOnly
                | LossCause::GroupPictureAppearance
                | LossCause::InlineStyle,
                LossLocation::Body { part: original, .. },
            ) => {
                require(part == original)?;
            }
            (
                LossCause::SectionOutlineSelection,
                LossLocation::SectionProperty { part: original, span, .. },
            ) => {
                require(part == original)?;
                // resolve_spine_with_mode rejects duplicate spine targets;
                // PackageOrder::resolve falls back to unique ZIP paths. The count-2 defense
                // is exercised by outline_duplicate_section_path_refused with
                // only RawPackage's private resolved order changed.
                require(section_paths.get(original) == Some(&1))?;
                require(section_properties.insert((original, span.start, span.end)))?;
            }
            (
                LossCause::HeaderOnlyNonlinearOrder
                | LossCause::ScriptAuxiliaryOrder
                | LossCause::SingleSectionNonlinearOrder,
                LossLocation::Package { sections },
            ) => {
                require(part == "Contents/content.hpf / META-INF/container.xml")?;
                if evidence.cause == LossCause::SingleSectionNonlinearOrder {
                    require(sections.len() == 1)?;
                }
                require(
                    *sections
                        == (0..decoded.source_maps.len())
                            .map(|s| package.section_path(s))
                            .collect::<HwpxResult<Vec<_>>>()?,
                )?;
            }
            _ => return require(false),
        }
    }
    require(losses.next().is_none())
}

pub(super) fn check_locations(
    decoded: &HwpxDocument,
    effects: &crate::derived_values::DerivedEffects<'_, '_>,
) -> HwpxResult<()> {
    let basis = decoded.source_basis.as_ref().ok_or_else(|| invalid("missing input source"))?;
    let mut body_losses = basis
        .losses
        .iter()
        .filter_map(|loss| {
            crate::work::tick();
            match &loss.location {
                LossLocation::Body { part, span, digest } => Some((part, span, digest)),
                _ => None,
            }
        })
        .peekable();
    let properties: Vec<_> = basis
        .losses
        .iter()
        .filter_map(|loss| match &loss.location {
            LossLocation::SectionProperty { part, span, digest } => Some((part, span, digest)),
            _ => None,
        })
        .collect();
    if !properties.is_empty() {
        let mut owners = std::collections::HashMap::<&str, std::collections::HashSet<usize>>::new();
        for (section, map) in decoded.source_maps.iter().enumerate() {
            for node in map.nodes() {
                crate::work::tick();
                if !node.part().is_empty() {
                    owners.entry(node.part()).or_default().insert(section);
                }
            }
        }
        let locations = properties
            .into_iter()
            .map(|(part, span, digest)| {
                let sections =
                    owners.get(part.as_str()).ok_or_else(|| invalid("missing section owner"))?;
                require(sections.len() == 1)?;
                Ok((*sections.iter().next().unwrap(), span.clone(), *digest))
            })
            .collect::<HwpxResult<Vec<_>>>()?;
        effects.check_section_outline_losses(decoded, &locations)?;
    }
    // Header/package-only evidence needs no body location index.
    if body_losses.peek().is_none() {
        return Ok(());
    }
    // A location is usable only when exactly one Run across all sections owns
    // that entire range. Keep counts rather than overwriting duplicate owners.
    let mut locations = std::collections::HashMap::new();
    for (section, map) in decoded.source_maps.iter().enumerate() {
        for node in map.nodes() {
            crate::work::tick();
            if node.kind() == hwpforge_core::node_path::NodeKind::Run {
                if let [span] = node.ranges() {
                    let (count, _) = locations
                        .entry((node.part(), span.start, span.end))
                        .or_insert((0usize, section));
                    *count += 1;
                }
            }
        }
    }
    for (part, span, digest) in body_losses {
        let &(count, section) = locations
            .get(&(part.as_str(), span.start, span.end))
            .ok_or_else(|| invalid("unproven input decode loss"))?;
        require(count == 1)?;
        effects.check_preserved_loss(section, span, digest)?;
    }
    Ok(())
}
