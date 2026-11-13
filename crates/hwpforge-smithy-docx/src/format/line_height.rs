//! Word natural line height approximation; no font files or line breaking.
use super::*;
use hwpforge_foundation::HwpUnit;

#[derive(Clone, Copy)]
pub(super) struct Metrics {
    upm: u16,
    ascent: u16,
    descent: u16,
}

pub(super) fn metrics(name: &str) -> Option<Metrics> {
    // Same exact spelling comparison as Fonts::intern. Aliases do not merge source IDs.
    // All listed Regular faces have OS/2 USE_TYPO_METRICS clear; omit hhea lineGap.
    let (upm, ascent, descent) = match name {
        // Microsoft Malgun Gothic 6.67 (6.50M has the same Win metrics).
        "맑은 고딕" | "Malgun Gothic" => (2048, 2229, 495),
        // Microsoft batang.ttc: Batang, BatangChe, Gungsuh, GungsuhChe 5.00.
        "바탕" | "Batang" | "바탕체" | "BatangChe" | "궁서" | "Gungsuh" | "궁서체"
        | "GungsuhChe" => (1024, 879, 145),
        // Microsoft gulim.ttc: Gulim 5.01; GulimChe, Dotum, DotumChe 5.00.
        "굴림" | "Gulim" | "굴림체" | "GulimChe" | "돋움" | "Dotum" | "돋움체" | "DotumChe" => {
            (1024, 879, 145)
        }
        // Hancom HCR Batang 2.100 (not Haansoft Batang).
        "함초롬바탕" | "HCR Batang" => (1000, 1070, 230),
        // Hancom HCR Dotum 2.100.
        "함초롬돋움" | "HCR Dotum" => (1000, 1070, 230),
        // Microsoft Calibri 6.20.
        "Calibri" => (2048, 1950, 550),
        // Microsoft Cambria 6.90i; Cambria Math is deliberately not an alias.
        "Cambria" => (2048, 1946, 455),
        _ => return None,
    };
    Some(Metrics { upm, ascent, descent })
}

pub(super) fn east_asian(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x11ff | 0x2e80..=0xa4cf | 0xa960..=0xa97f
        | 0xac00..=0xd7ff | 0xf900..=0xfaff | 0xfe10..=0xfe1f | 0xfe30..=0xfe4f
        | 0xff00..=0xffef | 0x20000..=0x323af)
}

// Physical Core/Blueprint line spacing uses the HWPX default branch's
// packed scale (HwpUnit × 2 = pt × 200). HwpUnitChar case writes half of it.
// Keep ordinary distances and percentage spacing outside this conversion.
pub(super) fn physical_spacing(hwp_units: f64) -> f64 {
    hwp_units * 2.0
}

#[derive(Clone, Default)]
pub(super) struct Height {
    ascent: f64,
    descent: f64,
    size: i32,
    max_single: f64,
    different_sizes: bool,
    unknown: bool,
}
impl Height {
    pub(super) fn add(&mut self, size: i32, metrics: Option<Metrics>) {
        self.different_sizes |= self.size != 0 && self.size != size;
        self.size = self.size.max(size);
        let Some(m) = metrics else {
            self.unknown = true;
            return;
        };
        let ascent = f64::from(size) * f64::from(m.ascent) / f64::from(m.upm);
        let descent = f64::from(size) * f64::from(m.descent) / f64::from(m.upm);
        self.ascent = self.ascent.max(ascent);
        self.descent = self.descent.max(descent);
        self.max_single = self.max_single.max(ascent + descent);
    }
    pub(super) fn mixed(&self) -> bool {
        // A single face supplying both extrema needs no mixed-font warning,
        // including ties and aliases. Only a composite height exceeds every
        // individual height; size variation still approximates line breaking.
        self.different_sizes || self.ascent + self.descent > self.max_single
    }
    fn natural(&self, east: bool) -> f64 {
        (self.ascent + self.descent) * if east { 1.3 } else { 1.0 }
    }
    // Word line advance is shared by paragraph spacing projection and cell minima.
    fn word_spacing(&self, p: &ParaDelta, east: bool) -> f64 {
        let rule = if p.line.is_none() { 0 } else { p.line_rule.unwrap_or(0) };
        let line = f64::from(p.line.unwrap_or(240));
        // Unknown metrics retain the existing raw multiplier / specified minimum fallback.
        let natural = if self.unknown { f64::from(self.size) } else { self.natural(east) };
        match rule {
            1 => line * 5.0,
            2 => {
                if self.unknown {
                    line * 5.0
                } else {
                    natural.max(line * 5.0)
                }
            }
            _ => natural * line / 240.0,
        }
    }
    pub(super) fn word_line(&self, p: &ParaDelta, east: bool) -> Result<i32> {
        let height = self.word_spacing(p, east);
        let height = units::canonical_float(height.round())?;
        HwpUnit::new(height as i32)
            .map(|h| h.as_i32())
            .map_err(|e| DocxError::invalid("#format", "line", e.to_string()))
    }
    pub(super) fn marker_before(&self, body: &Self, p: &ParaDelta) -> Result<i32> {
        if self.unknown || body.unknown || self.size == 0 || body.size == 0 {
            return Ok(0);
        }
        // Compare unrounded Word advances. Exact spacing has equal advances;
        // atLeast clamps the composite and body, and auto scales the difference.
        // A numbering head uses the ordinary font advance, without the East Asian
        // paragraph grid expansion used for body line projection.
        let delta = (self.word_spacing(p, false) - body.word_spacing(p, false)).max(0.0);
        let delta = units::canonical_float(delta)?;
        HwpUnit::new(delta.round() as i32)
            .map(|h| h.as_i32())
            .map_err(|e| DocxError::invalid("#format", "spacing", e.to_string()))
    }
    pub(super) fn project(&self, p: &ParaDelta, east: bool) -> Result<Option<u64>> {
        if self.unknown || self.size == 0 {
            return Ok(None);
        }
        let spacing = self.word_spacing(p, east);
        let rule = if p.line.is_none() { 0 } else { p.line_rule.unwrap_or(0) };
        let value = if rule == 2 {
            physical_spacing(spacing.round())
        } else {
            spacing * 100.0 / f64::from(self.size)
        };
        Ok(Some(units::canonical_float(value)?.to_bits()))
    }
}
