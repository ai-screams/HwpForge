//! Input-local whole-table properties shared by style definitions and direct properties.
use super::*;
use crate::xml::{NamespaceId, W14_NS};
use hwpforge_foundation::HwpUnit;

/// Source-only facts survive value overlays and are consumed at table assembly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpaceUncertainty {
    pub(crate) format: bool,
    pub(crate) border: bool,
}
impl SpaceUncertainty {
    pub(crate) fn omit_format(&mut self) {
        self.format = true;
    }
    pub(crate) fn merge(&mut self, other: Self) {
        self.format |= other.format;
        self.border |= other.border;
    }
}

// left, right, top, bottom, insideH, insideV. None means inherit, not no line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TableProperties {
    pub(crate) space_uncertainty: SpaceUncertainty,
    pub(crate) borders: [Option<bool>; 6],
    // Source widths in HwpUnit, independent of the approximated output strokes.
    pub(crate) border_widths: [Option<i32>; 6],
    pub(crate) width: Option<Option<i32>>,
    pub(crate) width_kind: WidthKind,
    pub(crate) width_diagnostic: Option<Rc<WidthDiagnostic>>,
    pub(crate) width_diagnostics: Vec<Rc<WidthDiagnostic>>,
    pub(crate) indent: Option<i32>,
    pub(crate) alignment: Option<TableAlignment>,
    pub(crate) bidi: Option<bool>,
    pub(crate) margins: [Option<i32>; 4],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WidthKind {
    #[default]
    Unspecified,
    Dxa,
    Auto,
    Nil,
    Pct,
    Unknown,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct WidthDiagnostic {
    warning_index: usize,
    pub(crate) grid: std::cell::Cell<bool>,
    pub(crate) container: std::cell::Cell<bool>,
}
pub(crate) fn prepare_width_diagnostics<'a>(
    diagnostics: impl IntoIterator<Item = &'a Rc<WidthDiagnostic>>,
    warnings: &mut [DocxWarning],
) {
    // Numbering has saved positions in this vector. Keep every source slot
    // until its existing finalizer has inserted and merged delayed warnings.
    let decisions: std::collections::BTreeMap<_, _> = diagnostics
        .into_iter()
        .map(|d| (d.warning_index, !d.grid.get() || d.container.get()))
        .collect();
    for (index, warning) in warnings.iter_mut().enumerate() {
        if let Some(retain) = decisions.get(&index) {
            if let DocxWarning::TableWidthApproximated { action, .. } = warning {
                *action = if *retain { "use_container" } else { "discard_table_width" };
            }
        }
    }
}
pub(crate) fn finish_width_diagnostics(warnings: &mut Vec<DocxWarning>) {
    warnings.retain(|warning| warning.action() != "discard_table_width");
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TableAlignment {
    Left,
    Center,
    Right,
    Unsupported,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum TableProperty {
    Root,
    CellRoot,
    Borders,
    Margins,
    Headers,
    Leaf,
}
impl TableProperties {
    pub(crate) fn overlay(&mut self, other: &Self) {
        self.space_uncertainty.merge(other.space_uncertainty);
        for (a, b) in self.borders.iter_mut().zip(other.borders) {
            if b.is_some() {
                *a = b;
            }
        }
        for (a, b) in self.border_widths.iter_mut().zip(other.border_widths) {
            if b.is_some() {
                *a = b;
            }
        }
        for (a, b) in self.margins.iter_mut().zip(other.margins) {
            if b.is_some() {
                *a = b;
            }
        }
        if other.indent.is_some() {
            self.indent = other.indent;
        }
        if other.alignment.is_some() {
            self.alignment = other.alignment;
        }
        if other.bidi.is_some() {
            self.bidi = other.bidi;
        }
        if other.width.is_some() {
            self.width = other.width;
            self.width_kind = other.width_kind;
            self.width_diagnostic = other.width_diagnostic.clone();
        }
    }
    // Record unsupported value effects before approximation or inheritance can hide them.
    // Preferred widths only change horizontal extent: source border costs and the
    // forced-line row minimum do not depend on width-dependent wrapping.
    fn observe_value(&mut self, state: TableProperty, e: &Element) {
        let unsupported = match (state, e.name.as_str()) {
            (TableProperty::Root, "bidiVisual") => {
                !matches!(attr(e, "val"), Some("0" | "false" | "off"))
            }
            (TableProperty::Root, "jc") => {
                !matches!(attr(e, "val"), Some("left" | "start" | "center" | "right" | "end"))
            }
            // Percentage and automatic indents are ignored by the source format.
            (TableProperty::Root, "tblInd") => {
                !matches!(attr(e, "type"), None | Some("dxa" | "pct" | "auto"))
            }
            (TableProperty::Root | TableProperty::CellRoot, "tblW" | "tcW" | "tblCellSpacing") => {
                !matches!(attr(e, "type"), None | Some("dxa" | "pct" | "auto" | "nil"))
            }
            (TableProperty::Root, "tblLayout") => {
                !matches!(attr(e, "type"), None | Some("fixed" | "autofit"))
            }
            _ => false,
        };
        if unsupported {
            self.space_uncertainty.omit_format();
        }
    }
    // Direct tables are transient; transfer every declaration to the document owner
    // as part of reading it. Style definitions retain their own declaration list.
    pub(crate) fn read_and_collect(
        &mut self,
        state: TableProperty,
        e: &Element,
        source: (&str, &str),
        w: &mut Vec<DocxWarning>,
        diagnostics: &mut Vec<Rc<WidthDiagnostic>>,
    ) -> Result<TableProperty> {
        let state = self.read(state, e, source.0, source.1, w)?;
        diagnostics.append(&mut self.width_diagnostics);
        Ok(state)
    }
    pub(crate) fn read(
        &mut self,
        state: TableProperty,
        e: &Element,
        part: &str,
        at: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<TableProperty> {
        if child_admission(state.vocabulary_parent(), e) == Admission::Defer {
            self.space_uncertainty.omit_format();
        }
        if e.namespace_id != W_NS.0 {
            return omitted(part, at, w);
        }
        if attributes(e, state.vocabulary_parent(), part, at, w)?
            && !matches!(state, TableProperty::Borders)
        {
            self.space_uncertainty.omit_format();
        }
        self.observe_value(state, e);
        match (state, e.name.as_str()) {
            (TableProperty::Root | TableProperty::CellRoot, "tblBorders" | "tcBorders") => {
                Ok(TableProperty::Borders)
            }
            (TableProperty::Root | TableProperty::CellRoot, "tblCellMar" | "tcMar") => {
                Ok(TableProperty::Margins)
            }
            // Only table roots own deferred width decisions. Cell roots use width()
            // below, so they cannot create a declaration without a collector.
            (TableProperty::Root, "tblW") => {
                self.width_kind = match attr(e, "type") {
                    None | Some("dxa") => WidthKind::Dxa,
                    Some("auto") => WidthKind::Auto,
                    Some("nil") => WidthKind::Nil,
                    Some("pct") => WidthKind::Pct,
                    _ => WidthKind::Unknown,
                };
                self.width_diagnostic = None;
                self.width = Some(if matches!(self.width_kind, WidthKind::Auto | WidthKind::Nil) {
                    let diagnostic = Rc::new(WidthDiagnostic {
                        warning_index: w.len(),
                        grid: false.into(),
                        container: false.into(),
                    });
                    let mut pending = Vec::new();
                    warn(&mut pending, part, at, Kind::TableWidthApproximated,
                        "table_width", "pending_table_width",
                        "Relative or automatic width uses the container and known column proportions")?;
                    w.extend(pending);
                    self.width_diagnostics.push(diagnostic.clone());
                    self.width_diagnostic = Some(diagnostic);
                    None
                } else {
                    width(e, part, at, w)?
                });
                Ok(TableProperty::Leaf)
            }
            (TableProperty::CellRoot, "tblW")
            | (TableProperty::Root | TableProperty::CellRoot, "tcW") => {
                self.width = Some(width(e, part, at, w)?);
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Root, "tblInd") => {
                if !matches!(attr(e, "type"), None | Some("dxa")) {
                    return omitted(part, at, w);
                }
                let value = attr(e, "w")
                    .ok_or_else(|| DocxError::invalid(part, at, "missing table indent"))?;
                self.indent = Some(
                    super::units::length(value, 5, -HwpUnit::MAX_VALUE)
                        .map_err(|reason| DocxError::invalid(part, at, reason))?,
                );
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Root, "jc") => {
                self.alignment = Some(match attr(e, "val") {
                    Some("left" | "start") => TableAlignment::Left,
                    Some("center") => TableAlignment::Center,
                    Some("right" | "end") => TableAlignment::Right,
                    _ => TableAlignment::Unsupported,
                });
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Root, "bidiVisual") => {
                self.bidi = boolean(e, "val", Some(true), part, at, w)?;
                Ok(TableProperty::Leaf)
            }
            (TableProperty::CellRoot, "headers") => {
                omitted(part, at, w)?;
                Ok(TableProperty::Headers)
            }
            (TableProperty::Headers, "header") => omitted(part, at, w),
            (TableProperty::Root, "tblLayout") => omitted(part, at, w),
            (TableProperty::Root | TableProperty::CellRoot, "shd") => {
                warn(
                    w,
                    part,
                    at,
                    Kind::ContentLost,
                    "cell_background",
                    "omit",
                    "Table/cell background is omitted",
                )?;
                Ok(TableProperty::Leaf)
            }
            (TableProperty::Borders, name) => {
                if let Some(index) = ["left", "right", "top", "bottom", "insideH", "insideV"]
                    .iter()
                    .position(|n| *n == name)
                {
                    let line = attr(e, "val");
                    let solid = !matches!(line, Some("none" | "nil"));
                    self.borders[index] = Some(solid);
                    self.border_widths[index] = Some(if solid {
                        let eighths = attr(e, "sz")
                            .unwrap_or("4")
                            .parse::<u32>()
                            .map_err(|_| DocxError::invalid(part, at, "invalid border width"))?;
                        // 1pt = 100 HwpUnit; round half a unit upward.
                        i32::try_from((u64::from(eighths) * 100 + 4) / 8)
                            .ok()
                            .filter(|width| *width <= HwpUnit::MAX_VALUE)
                            .ok_or_else(|| DocxError::invalid(part, at, "border width overflow"))?
                    } else {
                        0
                    });
                    self.space_uncertainty.border |=
                        unsupported_border(e, self.border_widths[index]);
                    if solid
                        && (line != Some("single")
                            || attr(e, "sz").is_some()
                            || attr(e, "color").is_some_and(|c| !matches!(c, "auto" | "000000"))
                            || e.attributes.iter().any(|(_, k, v)| {
                                !(matches!(k.as_str(), "val" | "sz" | "color")
                                    || k == "space" && v == "0")
                            }))
                    {
                        warn(
                            w,
                            part,
                            at,
                            Kind::TableBorderApproximated,
                            "table_border",
                            "use_default",
                            "Border line, width or color uses the default solid line",
                        )?;
                    }
                    Ok(TableProperty::Leaf)
                } else {
                    omitted(part, at, w)
                }
            }
            (TableProperty::Margins, name) => {
                if let Some(index) =
                    ["left", "right", "top", "bottom"].iter().position(|n| *n == name)
                {
                    if !matches!(attr(e, "type"), None | Some("dxa")) {
                        self.space_uncertainty.omit_format();
                        return omitted(part, at, w);
                    }
                    self.margins[index] =
                        Some(length(e, "w", StructuralDimension::Margin, part, at)?);
                    Ok(TableProperty::Leaf)
                } else {
                    omitted(part, at, w)
                }
            }
            (TableProperty::Root | TableProperty::CellRoot, "tblCellSpacing") => {
                self.space_uncertainty.border |= attr(e, "w").is_none_or(|v| !known_zero(v));
                omitted(part, at, w)
            }
            _ => {
                if child_admission(state.vocabulary_parent(), e) != Admission::Neutral {
                    self.space_uncertainty.omit_format();
                }
                omitted(part, at, w)
            }
        }
    }
}
// Read source facts even when the caller cannot emit this border face.
pub(crate) fn unsupported_border(e: &Element, width: Option<i32>) -> bool {
    let solid = !matches!(attr(e, "val"), Some("none" | "nil"));
    // Missing or reassignable line sizes cannot establish an original width.
    solid
        && (attr(e, "val") != Some("single")
            || width.is_none_or(|w| w <= 0)
            || attr(e, "sz")
                .and_then(|v| v.parse::<u32>().ok())
                .is_none_or(|v| !(2..=96).contains(&v)))
        || attr(e, "space").is_some_and(|v| !known_zero(v))
        || e.attributes
            .iter()
            .any(|(ns, k, _)| vocabulary("border", *ns, k, true).0 == Admission::Defer)
}

// Space is an integer value; equivalent zero spellings have the same cost.
fn known_zero(value: &str) -> bool {
    value.parse::<u32>() == Ok(0)
}

pub(crate) fn length(
    e: &Element,
    key: &str,
    dimension: StructuralDimension,
    part: &str,
    at: &str,
) -> Result<i32> {
    let value =
        attr(e, key).ok_or_else(|| DocxError::invalid(part, at, "missing structural length"))?;
    structural_length(value, dimension).map_err(|reason| DocxError::invalid(part, at, reason))
}
pub(crate) fn width(
    e: &Element,
    part: &str,
    at: &str,
    w: &mut Vec<DocxWarning>,
) -> Result<Option<i32>> {
    match attr(e, "type") {
        None | Some("dxa") => Ok(Some(length(e, "w", StructuralDimension::Width, part, at)?)),
        Some("auto" | "nil" | "pct") => {
            warn(
                w,
                part,
                at,
                Kind::TableWidthApproximated,
                "table_width",
                "use_container",
                "Relative or automatic width uses the container and known column proportions",
            )?;
            Ok(None)
        }
        _ => {
            warn(
                w,
                part,
                at,
                Kind::UnsupportedAttribute,
                "table_width_type",
                "omit",
                "Unknown width type omitted; width uses the container",
            )?;
            Ok(None)
        }
    }
}
fn omitted(part: &str, at: &str, w: &mut Vec<DocxWarning>) -> Result<TableProperty> {
    warn(
        w,
        part,
        at,
        Kind::UnsupportedAttribute,
        "table_property",
        "omit",
        "Unsupported table property is omitted",
    )?;
    Ok(TableProperty::Leaf)
}

// All table vocabulary decisions live here. Names prefixed by @ are attributes;
// other names are immediate children. A space separates exact names, never prefixes.
// Style cell/row contexts differ because their geometry is not applied to direct cells/rows.
// Unknown names, namespaces and children of leaves are deferred before discarding them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    Read,
    Neutral,
    Defer,
}
// (parent/owner, names, namespace, admission, preserve attribute omission diagnostic)
const TABLE_VOCABULARY: &[(&str, &str, NamespaceId, Admission, bool)] = &[
    // 표 속성·격자·행을 구조 파서가 읽는다.
    ("tbl", "tblPr tblGrid tr", W_NS.0, Admission::Read, false),
    // 행 속성과 칸을 구조 파서가 읽는다.
    ("tr", "trPr tc", W_NS.0, Admission::Read, false),
    // 행별 표 속성 예외의 기하를 적용하지 않는다.
    ("tr", "tblPrEx", W_NS.0, Admission::Defer, false),
    // 칸 속성·문단·중첩 표는 각각의 소유자가 읽는다.
    ("tc", "tcPr p tbl", W_NS.0, Admission::Read, false),
    // 열 너비를 읽는다.
    ("tblGrid", "gridCol", W_NS.0, Admission::Read, false),
    // 격자 변경 추적은 기존 오류 정책을 유지한다.
    ("tblGrid", "tblGridChange", W_NS.0, Admission::Defer, false),
    // 선택 스타일·폭·위치·선·여백·간격·배경의 기존 판독을 사용한다.
    ("tblPr", "tblStyle tblW jc bidiVisual tblInd tblBorders tblCellMar tblCellSpacing shd", W_NS.0, Admission::Read, false),
    // 격자와 명시 폭의 기존 계산을 유지하며 자동 맞춤 알고리즘은 구현하지 않는다.
    ("tblPr", "tblLayout", W_NS.0, Admission::Read, false),
    // 조건부 스타일 존재는 선택 스타일 사슬에서 별도로 보류한다.
    ("tblPr", "tblLook", W_NS.0, Admission::Neutral, false),
    // 행 띠 크기는 별도로 보류하는 조건부 스타일에만 쓰인다.
    ("tblPr", "tblStyleRowBandSize", W_NS.0, Admission::Neutral, false),
    // 열 띠 크기는 별도로 보류하는 조건부 스타일에만 쓰인다.
    ("tblPr", "tblStyleColBandSize", W_NS.0, Admission::Neutral, false),
    // 부동 표 겹침 허용은 위치를 지정하지 않으며 tblpPr은 별도로 보류한다.
    ("tblPr", "tblOverlap", W_NS.0, Admission::Neutral, false),
    // 접근성 표 제목은 기하와 무관한 메타데이터다.
    ("tblPr", "tblCaption", W_NS.0, Admission::Neutral, false),
    // 접근성 표 설명은 기하와 무관한 메타데이터다.
    ("tblPr", "tblDescription", W_NS.0, Admission::Neutral, false),
    // 부동 위치 또는 변경 전 속성을 적용하지 않는다.
    ("tblPr", "tblpPr tblPrChange", W_NS.0, Admission::Defer, false),
    // 행 격자와 높이 예산을 읽는다.
    ("trPr", "gridBefore gridAfter trHeight", W_NS.0, Admission::Read, false),
    // 반복 머리행은 쪽 나눔 전용이며 기존 출력 정책을 유지한다.
    ("trPr styleTrPr", "tblHeader", W_NS.0, Admission::Neutral, false),
    // 행 나누지 않음은 쪽 나눔 전용이며 기존 출력 정책을 유지한다.
    ("trPr styleTrPr", "cantSplit", W_NS.0, Admission::Neutral, false),
    // 조건부 서식·연결 문단·전후 폭·간격·행 위치·숨김·변경 추적은 해석하지 않는다.
    ("trPr", "cnfStyle divId wBefore wAfter tblCellSpacing jc hidden ins del trPrChange", W_NS.0, Admission::Defer, false),
    // 칸 폭·병합·선·여백·배경·수직 맞춤을 읽는다.
    ("tcPr", "tcW gridSpan vMerge tcBorders tcMar shd vAlign", W_NS.0, Admission::Read, false),
    // 측정하지 않은 판단: noWrap은 줄바꿈과 행 높이를, hideMark는 끝 표식을 포함한 최소 높이를 바꿀 수 있다.
    // textDirection은 글 진행 축을, tcFitText는 내용 폭을, hMerge는 칸 경계를 바꾼다.
    // 변경 추적은 기존 오류다.
    ("tcPr styleTcPr", "cnfStyle hMerge textDirection tcFitText noWrap hideMark cellIns cellDel cellMerge tcPrChange", W_NS.0, Admission::Defer, false),
    // Word가 무시하는 머리칸 연결 정보는 공간 서식에 영향을 주지 않는다.
    ("tcPr styleTcPr", "headers", W_NS.0, Admission::Neutral, false),
    // 행별 예외는 하위 속성을 적용하지 않고 소유 표를 보류한다.
    ("tblPrEx", "tblW jc tblCellSpacing tblInd tblBorders shd tblLayout tblCellMar tblLook tblPrExChange", W_NS.0, Admission::Defer, false),
    // 스타일 소유자를 유지하며 각 속성 문맥과 조건부 판정을 사용한다.
    ("style", "tblPr tcPr trPr tblStylePr", W_NS.0, Admission::Read, false),
    // wholeTable 안의 표 속성 덮어쓰기는 구현하지 않는다.
    ("tblStylePr", "tblPr", W_NS.0, Admission::Defer, false),
    // 소유 스타일에 칸·행·문단·문자 속성의 기존 판독을 연결한다.
    ("tblStylePr", "tcPr trPr pPr rPr", W_NS.0, Admission::Read, false),
    // 배경은 기존 생략 진단으로 처리하며 기하를 바꾸지 않는다.
    ("styleTcPr", "shd", W_NS.0, Admission::Read, false),
    // 스타일 칸 속성의 기하를 직접 칸에 적용하지 않는다.
    ("styleTcPr", "tcW gridSpan vMerge tcBorders tcMar vAlign", W_NS.0, Admission::Defer, false),
    // 스타일 행 높이·위치·격자 등은 실제 행에 적용하지 않는다.
    ("styleTrPr", "cnfStyle divId gridBefore gridAfter wBefore wAfter trHeight tblCellSpacing jc hidden ins del trPrChange", W_NS.0, Admission::Defer, false),
    // 직접 선 폭과 내부선의 원본 불확정을 읽는다.
    ("tblBorders tcBorders", "top left bottom right insideH insideV", W_NS.0, Admission::Read, false),
    // 논리 방향선과 대각선 기하를 구현하지 않는다.
    ("tblBorders tcBorders", "start end tl2br tr2bl", W_NS.0, Admission::Defer, false),
    // 네 면 여백을 읽는다.
    ("tblCellMar tcMar", "top left bottom right", W_NS.0, Admission::Read, false),
    // 논리 방향 여백을 구현하지 않는다.
    ("tblCellMar tcMar", "start end", W_NS.0, Admission::Defer, false),
    // 머리칸 참조도 무시되는 연결 정보다.
    ("headers", "header", W_NS.0, Admission::Neutral, false),
    // 폭과 단위를 읽는다.
    ("tblW tcW tblInd", "@w @type", W_NS.0, Admission::Read, false),
    // 열 폭을 읽는다.
    ("gridCol", "@w", W_NS.0, Admission::Read, false),
    // 해당 속성의 값은 소유 문맥의 판독 또는 보류로 처리한다.
    ("jc bidiVisual gridBefore gridAfter gridSpan vMerge vAlign tblStyle", "@val", W_NS.0, Admission::Read, false),
    // 행·칸의 표 맞춤/방향 속성은 적용하지 않으며 기존 속성 생략 진단을 유지한다.
    ("nonTablePlacement", "@val", W_NS.0, Admission::Defer, true),
    // 높이 값과 exact/auto/minimum 규칙을 읽는다.
    ("trHeight", "@val @hRule", W_NS.0, Admission::Read, false),
    // 여백·선 속성을 읽고 공간 비용과 선 미지원 여부를 판정한다.
    ("border", "@val @sz @color @space @themeColor", W_NS.0, Admission::Read, false),
    // 색 보정은 기하를 바꾸지 않으며 기존 속성 생략 진단은 유지한다.
    ("border", "@themeTint @themeShade", W_NS.0, Admission::Neutral, true),
    // 여백의 크기와 단위를 읽는다. 선 속성과 문맥을 섞지 않는다.
    ("margin", "@w @type", W_NS.0, Admission::Read, false),
    // 그림자·프레임의 추가 기하는 구현하지 않는다.
    ("border", "@shadow @frame", W_NS.0, Admission::Defer, true),
    // 배경 색·무늬는 선 기하·칸 여백·행 예산·가로 위치를 바꾸지 않는다.
    ("shd", "@val @color @fill @themeColor @themeTint @themeShade @themeFill @themeFillTint @themeFillShade", W_NS.0, Admission::Neutral, false),
    // 조건부 서식은 선택 스타일에서 별도로 판정한다.
    ("tblLook", "@val @firstRow @lastRow @firstColumn @lastColumn @noHBand @noVBand", W_NS.0, Admission::Neutral, false),
    // 알려진 레이아웃 값은 기존 계산을 유지하고 미지 값은 보류한다.
    ("tblLayout", "@type", W_NS.0, Admission::Read, false),
    // 띠 크기는 별도 보류되는 조건부 서식에만 쓰인다.
    ("tblStyleRowBandSize tblStyleColBandSize", "@val", W_NS.0, Admission::Neutral, true),
    // 접근성 문자열은 배치와 무관하다.
    ("tblCaption tblDescription", "@val", W_NS.0, Admission::Neutral, true),
    // 쪽 나눔 플래그는 기존 출력 정책을 유지한다.
    ("tblHeader cantSplit", "@val", W_NS.0, Admission::Neutral, true),
    // 부동 표 겹침 플래그이며 부동 위치 자체는 별도로 보류한다.
    ("tblOverlap", "@val", W_NS.0, Admission::Neutral, true),
    // 간격 수치를 읽어 0만 수용하고 비영 값은 보류한다.
    ("tblCellSpacing", "@w @type", W_NS.0, Admission::Read, true),
    // 편집 세션 식별자는 기하와 무관하다.
    ("*", "@rsidR @rsidRPr @rsidRDefault @rsidTr @rsidDel", W_NS.0, Admission::Neutral, false),
    // 공동 편집·주석 연결 식별자는 배치·크기·선과 무관하다.
    ("tr", "@paraId @textId", W14_NS.0, Admission::Neutral, true),
    // wholeTable과 조건부 스타일을 구분한다.
    ("tblStylePr", "@type", W_NS.0, Admission::Read, false),
    // 조건부 서식 마스크를 적용하지 않는다.
    ("cnfStyle", "@val @firstRow @lastRow @firstColumn @lastColumn @oddVBand @evenVBand @oddHBand @evenHBand @firstRowFirstColumn @firstRowLastColumn @lastRowFirstColumn @lastRowLastColumn", W_NS.0, Admission::Defer, true),
    // 연결·숨김·글 흐름·줄바꿈·높이·병합·머리칸 효과를 해석하지 않는다.
    ("divId hidden textDirection tcFitText noWrap hideMark hMerge", "@val", W_NS.0, Admission::Defer, true),
    // 행 전후 폭을 해석하지 않는다.
    ("wBefore wAfter", "@w @type", W_NS.0, Admission::Defer, true),
    // 부동 표 위치와 본문과의 거리를 적용하지 않는다.
    ("tblpPr", "@leftFromText @rightFromText @topFromText @bottomFromText @vertAnchor @horzAnchor @tblpXSpec @tblpX @tblpYSpec @tblpY", W_NS.0, Admission::Defer, true),
    // 변경 추적 메타데이터는 기존 오류/처리를 유지한다.
    ("tblPrChange trPrChange tcPrChange tblPrExChange tblGridChange ins del cellIns cellDel cellMerge", "@id @author @date", W_NS.0, Admission::Defer, true),
    // 변경 전후 병합을 적용하지 않는다.
    ("cellMerge", "@vMerge @vMergeOrig", W_NS.0, Admission::Defer, true),
    // 머리칸 연결 문자열과 대상 식별자는 공간 서식에 영향을 주지 않는다.
    ("header", "@val", W_NS.0, Admission::Neutral, true),
    ("tc", "@id", W_NS.0, Admission::Neutral, true),
 ];

fn vocabulary(parent: &str, ns: NamespaceId, name: &str, attribute: bool) -> (Admission, bool) {
    TABLE_VOCABULARY
        .iter()
        .find_map(|(owners, names, namespace, admission, diagnostic)| {
            (*namespace == ns
                && (*owners == "*" || owners.split_ascii_whitespace().any(|p| p == parent))
                && names.split_ascii_whitespace().any(|n| {
                    if attribute {
                        n.strip_prefix('@') == Some(name)
                    } else {
                        n == name
                    }
                }))
            .then_some((*admission, *diagnostic))
        })
        .unwrap_or((Admission::Defer, true))
}

pub(crate) fn child_admission(parent: &str, e: &Element) -> Admission {
    vocabulary(parent, e.namespace_id, &e.name, false).0
}

impl TableProperty {
    pub(crate) fn vocabulary_parent(self) -> &'static str {
        match self {
            Self::Root => "tblPr",
            Self::CellRoot => "tcPr",
            Self::Borders => "tblBorders",
            Self::Margins => "tblCellMar",
            Self::Headers => "headers",
            Self::Leaf => "leaf",
        }
    }
}

pub(crate) fn attribute_admission(
    e: &Element,
    parent: &str,
    ns: NamespaceId,
    key: &str,
) -> (Admission, bool) {
    if e.namespace_id != W_NS.0 {
        return (Admission::Defer, true);
    }
    let owner = match parent {
        "tblBorders" | "tcBorders" => "border",
        "tblCellMar" | "tcMar" => "margin",
        _ if matches!(e.name.as_str(), "jc" | "bidiVisual") && parent != "tblPr" => {
            "nonTablePlacement"
        }
        _ => &e.name,
    };
    vocabulary(owner, ns, key, true)
}

// Attribute names are finite; input strings never become diagnostic keys.
pub(crate) fn attributes(
    e: &Element,
    parent: &str,
    part: &str,
    at: &str,
    w: &mut Vec<DocxWarning>,
) -> Result<bool> {
    let mut omitted = false;
    for (ns, key, _) in &e.attributes {
        let (admission, diagnostic) = attribute_admission(e, parent, *ns, key);
        omitted |= admission == Admission::Defer;
        if diagnostic {
            warn(
                w,
                part,
                at,
                Kind::UnsupportedAttribute,
                "table_attribute",
                "omit",
                "Unsupported table attribute omitted",
            )?;
        }
    }
    Ok(omitted)
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
