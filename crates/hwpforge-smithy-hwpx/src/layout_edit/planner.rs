//! Editor-independent change closure. No Core JSON or encoder calls belong here.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::wire::{PackageAnalysis, Signals};
use crate::{ParagraphPath, PathSeg};

/// Correspondence and flow topology supplied by an editor. Paths are simple
/// section/paragraph/container indices, suitable for adapting to a shared path type.
#[derive(Default)]
pub struct ParagraphCorrespondence {
    pub(crate) pairs: Vec<(Option<ParagraphPath>, ParagraphPath)>,
    pub(crate) para_list: Vec<usize>,
    pub(crate) list_host: Vec<Option<usize>>,
}

impl ParagraphCorrespondence {
    /// Pair original paragraphs with emitted Core origins. Never match by text
    /// or paragraph ids. An insertion/deletion admits only a proven prefix.
    pub fn new(pairs: Vec<(Option<ParagraphPath>, ParagraphPath)>) -> Self {
        let indices: HashMap<_, _> =
            pairs.iter().enumerate().map(|(i, (_, p))| (p.0.clone(), i)).collect();
        let mut lists = HashMap::new();
        let mut result = Self { pairs, ..Self::default() };
        for (_, path) in &result.pairs {
            let prefix = &path.0[..path.0.len().saturating_sub(1)];
            let next = lists.len();
            let list = *lists.entry(prefix.to_vec()).or_insert(next);
            if list == result.list_host.len() {
                let host =
                    (0..prefix.len()).rev().find_map(|i| indices.get(&prefix[..=i]).copied());
                result.list_host.push(host);
            }
            result.para_list.push(list);
        }
        result
    }

    /// Identity correspondence for an editor that preserves paragraph structure.
    pub fn identity(package: &PackageAnalysis) -> Self {
        Self::new(package.paths().map(|p| (Some(p.clone()), p.clone())).collect())
    }
}

/// Explicit edits and losses from an editor, in addition to package differences.
#[derive(Default)]
pub struct ChangeSet {
    /// Changed paragraphs and reasons (geometry, text, insertion, etc.).
    pub paragraphs: BTreeMap<String, (ParagraphPath, BTreeSet<String>)>,
    /// Changed wire definitions or package targets; dependents are found by the graph.
    pub definitions: BTreeSet<super::DependencyKey>,
    /// Section-scoped changes or losses.
    pub sections: BTreeMap<usize, BTreeSet<String>>,
    /// Changes or losses with document-wide scope.
    pub document: BTreeSet<String>,
}

impl ChangeSet {
    /// Seed a paragraph edit. The planner applies all nonlocal policies.
    pub fn paragraph(&mut self, path: ParagraphPath, reason: impl Into<String>) {
        self.paragraphs
            .entry(path.to_string())
            .or_insert_with(|| (path, BTreeSet::new()))
            .1
            .insert(reason.into());
    }
}

/// Per-paragraph and per-section invalidation reasons; an empty set permits
/// cache reuse only when a trusted original cache and correspondence exist.
#[derive(Default, Debug)]
pub struct InvalidationPlan {
    /// Results in correspondence order.
    pub paragraphs: Vec<BTreeSet<String>>,
    /// Reasons for invalidating entire sections.
    pub sections: BTreeMap<usize, BTreeSet<String>>,
}

/// Compute the same floating, keep, numbering, unknown-reference, document-input
/// and flow/host closure for every editor. Package differences seed themselves;
/// editors only need to supply edits not visible in the emitted package.
pub fn plan_invalidation(
    base: &PackageAnalysis,
    emitted: &PackageAnalysis,
    correspondence: &ParagraphCorrespondence,
    changes: &ChangeSet,
) -> InvalidationPlan {
    let count = emitted.sections.len();
    let reading_order_changed = base.reading_order_changed(emitted);
    let definition_edits: Vec<BTreeSet<String>> = correspondence
        .pairs
        .iter()
        .map(|(old, path)| {
            changes
                .definitions
                .iter()
                .filter(|key| {
                    old.as_ref()
                        .and_then(|p| base.paragraph_dependencies(p))
                        .is_some_and(|deps| deps.contains(*key))
                        || emitted
                            .paragraph_dependencies(path)
                            .is_some_and(|deps| deps.contains(*key))
                })
                .map(|(kind, id)| format!("definition changed:{kind}/{id}"))
                .collect()
        })
        .collect();
    let mut changed = (0..count).map(|s| !base.section_same(emitted, s)).collect::<Vec<_>>();
    for (s, flag) in changed.iter_mut().enumerate() {
        *flag |= reading_order_changed
            || !changes.document.is_empty()
            || changes.sections.contains_key(&s)
            || changes.paragraphs.values().any(|(p, _)| p.0.first() == Some(&PathSeg::Section(s)))
            || correspondence.pairs.iter().zip(&definition_edits).any(|((_, path), edits)| {
                !edits.is_empty() && path.0.first() == Some(&PathSeg::Section(s))
            });
    }
    let mut dependencies = Signals::default();
    for flags in base.sections.iter().chain(&emitted.sections) {
        dependencies.merge(*flags);
    }
    let backwards =
        dependencies.backwards && (base.sections.len() != count || changed.iter().any(|&v| v));
    let mut result = InvalidationPlan::default();
    let mut numbering_changed = false;
    for (s, &is_changed) in changed.iter().enumerate() {
        let starts_changed = base.starts.get(s) != emitted.starts.get(s);
        numbering_changed |= starts_changed || (dependencies.numbering && is_changed);
        let mut local = emitted.sections[s];
        if let Some(base) = base.sections.get(s) {
            local.merge(*base);
        }
        let floating_changed = local.floating && is_changed;
        let linked_changed = local.linked && is_changed;
        let unknown_changed = local.unknown && is_changed;
        let unmapped = is_changed
            && correspondence.pairs.iter().any(|(old, path)| {
                path.0.first() == Some(&PathSeg::Section(s))
                    && old
                        .as_ref()
                        .is_some_and(|old| !base.mapped(&old.0) || !emitted.mapped(&path.0))
            });
        let reasons = result.sections.entry(s).or_default();
        reasons.extend(changes.document.iter().cloned());
        reasons.extend(changes.sections.get(&s).into_iter().flatten().cloned());
        for (condition, reason) in [
            (reading_order_changed, "policy: spine reading order changed"),
            (base.document_input_changed(emitted, s), "policy: document input consumer"),
            (backwards, "policy: numbering/page dependencies (backwards)"),
            (numbering_changed, "policy: numbering/page dependencies"),
            (floating_changed, "policy: floating-object influence"),
            (linked_changed, "policy: linked paragraphs"),
            (unknown_changed, "policy: unknown/unresolved emitted dependency"),
            (local.unresolved, "policy: unresolved reference"),
            (unmapped, "policy: unmapped paragraph topology"),
        ] {
            if condition {
                reasons.insert(reason.into());
            }
        }
        if !reasons.is_empty() {
            reasons.extend(base.section_reasons(emitted, s));
        }
    }
    for ((original, path), definitions) in correspondence.pairs.iter().zip(definition_edits) {
        let mut reasons = original.as_ref().map_or_else(
            || BTreeSet::from(["no base paragraph correspondence".into()]),
            |old| base.paragraph_reasons(emitted, old, path).into_iter().collect(),
        );
        if let Some((_, edits)) = changes.paragraphs.get(&path.to_string()) {
            reasons.extend(edits.iter().cloned());
        }
        if let Some(PathSeg::Section(s)) = path.0.first() {
            reasons.extend(result.sections.get(s).into_iter().flatten().cloned());
        }
        reasons.extend(definitions);
        result.paragraphs.push(reasons);
    }
    // Propagate the actual cause, not only a boolean: census counts affected
    // paragraphs once per cause, including the flow tail and host chain.
    let mut by_reason: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, reasons) in result.paragraphs.iter().enumerate() {
        for reason in reasons {
            by_reason.entry(reason.clone()).or_default().push(i);
        }
    }
    for (reason, seeds) in by_reason {
        let stale = crate::cache_flow::stale_paragraphs(
            &correspondence.para_list,
            &correspondence.list_host,
            seeds,
            &mut || {},
        );
        for (reasons, stale) in result.paragraphs.iter_mut().zip(stale) {
            if stale {
                reasons.insert(reason.clone());
            }
        }
    }
    result
}
