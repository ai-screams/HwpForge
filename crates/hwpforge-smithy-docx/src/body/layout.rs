//! Section geometry and bounded, package-local header/footer references.
use super::{attribute, Location};
use crate::{
    error::{quote, Result},
    format::{attr, PropertyState},
    package::{Part, Relationship, RelationshipKind, Target},
    warning::{record, Kind},
    xml::{Element, R_NS, W_NS},
    DocxError, DocxWarning,
};
use hwpforge_core::{PageSettings, Section};
use hwpforge_foundation::HwpUnit;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RelationKey(usize);
#[derive(Default)]
pub(super) struct Relations {
    names: HashMap<String, RelationKey>,
    // Integer keys select shared Part handles; no target path is copied per reference.
    targets: Vec<(bool, Part)>,
}
impl Relations {
    pub(super) fn admit(&mut self, relation: &Relationship, part: &str) -> Result<()> {
        let footer = match relation.kind {
            RelationshipKind::Header => false,
            RelationshipKind::Footer => true,
            _ => return Ok(()),
        };
        let Target::Internal(target) = &relation.target else { return Ok(()) };
        crate::identifier::check(&relation.id, part, "relationship")?;
        let key = RelationKey(self.targets.len());
        self.targets.push((footer, target.clone()));
        self.names.insert(relation.id.clone(), key);
        Ok(())
    }
    fn resolve(&self, id: &str, footer: bool, part: &str, at: &str) -> Result<Option<RelationKey>> {
        crate::identifier::check(id, part, at)?;
        Ok(self.names.get(id).copied().filter(|key| self.targets[key.0].0 == footer))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct References {
    // Slots: default, first, even. Outer None is omitted; Some(None) is an
    // explicit unresolved reference and must not inherit an earlier default.
    pub(super) headers: [Option<Option<RelationKey>>; 3],
    pub(super) footers: [Option<Option<RelationKey>>; 3],
}
impl References {
    pub(super) fn inherit_defaults(&mut self, previous: Self) {
        self.headers[0] = self.headers[0].or(previous.headers[0]);
        self.footers[0] = self.footers[0].or(previous.footers[0]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Break {
    Page,
    Column,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Origin {
    pub(super) paragraph: usize,
    // One source paragraph currently owns one potential list item. Splitting
    // never creates another item; only fragment zero can carry its marker.
    pub(super) list_item: usize,
    pub(super) fragment: usize,
    pub(super) cause: Option<Break>,
}
pub(super) struct DecodedSection {
    pub(super) core: Section,
    pub(super) origins: Vec<Origin>,
    pub(super) references: References,
    // Physical content extents, after orientation; consumers must not use
    // portrait-ordered PageSettings.width as the landscape container width.
    pub(super) content_width: HwpUnit,
    pub(super) content_height: HwpUnit,
}

#[derive(Default)]
pub(super) struct Layout {
    page: PageSettings,
    size_present: [bool; 2],
    margins_present: [bool; 7],
    columns_approximated: bool,
    orient: Option<bool>,
    pub(super) references: References,
}
impl Layout {
    pub(super) fn read(
        &mut self,
        e: &Element,
        part: &str,
        at: &Location,
        relations: Option<&Relations>,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<PropertyState> {
        if e.namespace_id != W_NS.0 {
            return exclude(part, at, warnings);
        }
        let known: &[&str] = match e.name.as_str() {
            "pgSz" => {
                for ((key, target), present) in
                    [("w", &mut self.page.width), ("h", &mut self.page.height)]
                        .into_iter()
                        .zip(&mut self.size_present)
                {
                    if let Some(value) = attr(e, key) {
                        *target = dimension(value, 1, part, at, key)?;
                        *present = true;
                    }
                }
                self.orient = match attr(e, "orient") {
                    Some("landscape") => Some(true),
                    Some("portrait") => Some(false),
                    None => None,
                    Some(_) => {
                        warn(
                            warnings,
                            part,
                            at,
                            "page_orientation",
                            "approximate",
                            "Unsupported orientation; physical dimensions determine orientation",
                        )?;
                        // The normalization still reports one orientation loss, not two.
                        Some(self.page.width > self.page.height)
                    }
                };
                &["w", "h", "orient", "code"]
            }
            "pgMar" => {
                for ((key, target), present) in [
                    ("top", &mut self.page.margin_top),
                    ("right", &mut self.page.margin_right),
                    ("bottom", &mut self.page.margin_bottom),
                    ("left", &mut self.page.margin_left),
                    ("header", &mut self.page.header_margin),
                    ("footer", &mut self.page.footer_margin),
                    ("gutter", &mut self.page.gutter),
                ]
                .into_iter()
                .zip(&mut self.margins_present)
                {
                    if let Some(value) = attr(e, key) {
                        *target = dimension(value, 0, part, at, key)?;
                        *present = true;
                    }
                }
                &["top", "right", "bottom", "left", "header", "footer", "gutter"]
            }
            "type" => {
                if !matches!(attr(e, "val"), None | Some("nextPage")) {
                    warn(
                        warnings,
                        part,
                        at,
                        "section_start",
                        "new_page",
                        "Section start is approximated by a new page",
                    )?;
                }
                &["val"]
            }
            "cols" => {
                if let Some(value) = attr(e, "num") {
                    let num = value.parse::<u32>().ok().filter(|n| *n > 0).ok_or_else(|| {
                        DocxError::invalid(
                            part,
                            at,
                            format!("cols num observed '{}', allowed 1..4294967295", quote(value)),
                        )
                    })?;
                    if num > 1 {
                        self.approximate_columns(part, at, warnings)?;
                    }
                }
                if matches!(attr(e, "equalWidth"), Some("0" | "false" | "off")) {
                    self.approximate_columns(part, at, warnings)?;
                }
                &["num", "space", "equalWidth", "sep"]
            }
            "headerReference" | "footerReference" => {
                let footer = e.name == "footerReference";
                let slot = match attr(e, "type") {
                    Some("default") => 0,
                    Some("first") => 1,
                    Some("even") => 2,
                    _ => {
                        warn(
                            warnings,
                            part,
                            at,
                            "header_footer_reference",
                            "omit",
                            "Missing or unsupported header/footer type",
                        )?;
                        return Ok(PropertyState::Excluded);
                    }
                };
                let id = attribute(e, R_NS.0, "id");
                let key = if let Some(id) = id {
                    crate::identifier::check(id, part, &at.to_string())?;
                    relations
                        .map(|r| r.resolve(id, footer, part, &at.to_string()))
                        .transpose()?
                        .flatten()
                } else {
                    None
                };
                if key.is_none() {
                    record(
                        warnings,
                        Kind::ContentOmitted,
                        "header_footer_reference",
                        "omit",
                        || (Some(part), Some(at), "Unresolved header/footer reference excluded"),
                    )?;
                }
                let refs = if footer {
                    &mut self.references.footers
                } else {
                    &mut self.references.headers
                };
                if refs[slot].is_some() {
                    return Err(DocxError::invalid(
                        part,
                        at,
                        "duplicate header/footer reference type",
                    ));
                }
                refs[slot] = Some(key);
                &["type"]
            }
            _ => return exclude(part, at, warnings),
        };
        for (ns, key, _) in &e.attributes {
            if !(*ns == W_NS.0 && known.contains(&key.as_str())
                || *ns == R_NS.0
                    && key == "id"
                    && matches!(e.name.as_str(), "headerReference" | "footerReference"))
            {
                warn(
                    warnings,
                    part,
                    at,
                    "page_attribute",
                    "omit",
                    "Unsupported section attribute excluded",
                )?;
            }
        }
        Ok(PropertyState::Leaf)
    }
    pub(super) fn approximate_columns(
        &mut self,
        part: &str,
        at: &Location,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<()> {
        if !self.columns_approximated {
            warn(
                warnings,
                part,
                at,
                "columns",
                "single_column",
                "Multiple or unequal columns use one content column",
            )?;
            self.columns_approximated = true;
        }
        Ok(())
    }
    pub(super) fn finish(
        mut self,
        part: &str,
        at: &Location,
        warnings: &mut Vec<DocxWarning>,
    ) -> Result<(PageSettings, References, HwpUnit, HwpUnit)> {
        for (present, loss) in [
            (self.size_present.as_slice(), "page_size"),
            (self.margins_present.as_slice(), "page_margins"),
        ] {
            if !present.iter().all(|present| *present) {
                warn(
                    warnings,
                    part,
                    at,
                    loss,
                    "use_defaults",
                    "Missing section page setting uses Core defaults",
                )?;
            }
        }
        let width = self.page.width;
        let height = self.page.height;
        let actual = if width == height { self.orient.unwrap_or(false) } else { width > height };
        // Absent orient defaults to portrait, including square pages.
        if self.orient.map_or(actual, |orient| orient != actual) {
            warn(
                warnings,
                part,
                at,
                "page_orientation",
                "approximate",
                if self.orient.is_none() {
                    "Orientation inferred from page dimensions"
                } else {
                    "Orientation contradicts page dimensions; physical dimensions take precedence"
                },
            )?;
        }
        self.page.landscape = actual;
        self.page.width = width.min(height);
        self.page.height = width.max(height);
        let content_width = i64::from(width.as_i32())
            - i64::from(self.page.margin_left.as_i32())
            - i64::from(self.page.margin_right.as_i32())
            - i64::from(self.page.gutter.as_i32());
        let content_height = i64::from(height.as_i32())
            - i64::from(self.page.margin_top.as_i32())
            - i64::from(self.page.margin_bottom.as_i32());
        for (name, value) in [("content width", content_width), ("content height", content_height)]
        {
            if value <= 0 {
                return Err(DocxError::invalid(
                    part,
                    at,
                    format!("{name} observed {value}, allowed 1..100000000 HwpUnit"),
                ));
            }
        }
        Ok((
            self.page,
            self.references,
            HwpUnit::new(content_width as i32)
                .map_err(|e| DocxError::invalid(part, at, quote(e)))?,
            HwpUnit::new(content_height as i32)
                .map_err(|e| DocxError::invalid(part, at, quote(e)))?,
        ))
    }
}
pub(super) fn exclude(
    part: &str,
    at: &Location,
    warnings: &mut Vec<DocxWarning>,
) -> Result<PropertyState> {
    warn(
        warnings,
        part,
        at,
        "page_property",
        "omit",
        "Unsupported section property subtree excluded",
    )?;
    Ok(PropertyState::Excluded)
}
fn dimension(value: &str, minimum: i32, part: &str, at: &Location, key: &str) -> Result<HwpUnit> {
    let n = crate::format::structural_length(value, minimum).map_err(|reason| {
        DocxError::invalid(
            part,
            at,
            format!(
                "{key} observed '{}' twip; allowed {minimum}..100000000 HwpUnit: {reason}",
                quote(value)
            ),
        )
    })?;
    HwpUnit::new(n).map_err(|e| DocxError::invalid(part, at, quote(e)))
}
pub(super) fn warn(
    w: &mut Vec<DocxWarning>,
    part: &str,
    at: &Location,
    loss: &'static str,
    action: &'static str,
    reason: &'static str,
) -> Result<()> {
    record(w, Kind::PageLayoutApproximated, loss, action, || (Some(part), Some(at), reason))
}
