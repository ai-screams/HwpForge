use super::*;

// Widths remain rational until each final column is rounded once.
#[derive(Clone, Copy)]
struct Fraction {
    n: i128,
    d: i128,
}
impl Fraction {
    fn new(n: i128, d: i128) -> Self {
        let g = gcd(n, d);
        Self { n: n / g, d: d / g }
    }
    fn add(self, b: Self) -> Option<Self> {
        let g = gcd(self.d, b.d);
        let d = self.d.checked_mul(b.d / g)?;
        Some(Self::new(self.n.checked_mul(b.d / g)?.checked_add(b.n.checked_mul(self.d / g)?)?, d))
    }
    fn ratio(self, total: Self, width: i32) -> Option<i32> {
        let n = self.n.checked_mul(total.d)?.checked_mul(i128::from(width))?;
        let d = self.d.checked_mul(total.n)?;
        i32::try_from(n.checked_mul(2)?.checked_add(d)? / d.checked_mul(2)?).ok()
    }
}
fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}
impl WidthPlan {
    pub(super) fn apply(
        self,
        table: &mut Table,
        container: i32,
        part: &str,
        w: &mut Vec<DocxWarning>,
    ) -> Result<()> {
        let error = || {
            DocxError::invalid(
                part,
                &self.at,
                "inconsistent or overflowing table width distribution",
            )
        };
        if container <= 0 {
            return Err(error());
        }
        let width = self.width.unwrap_or(container).min(container);
        let mut approximate = self.width.is_none() || self.width.is_some_and(|v| v > container);
        let mut weights: Vec<_> =
            self.columns.iter().map(|v| v.map(|n| Fraction::new(i128::from(n), 1))).collect();
        // A declared grid wins every conflicting cell preference. Where the grid
        // leaves a column unspecified, the first explicit cell supplies its share.
        for cell in &self.cells {
            if let Some(preferred) = cell.preferred {
                let share = Fraction::new(i128::from(preferred), cell.span as i128);
                for weight in &mut weights[cell.col..cell.col + cell.span] {
                    if weight.is_none() {
                        *weight = Some(share);
                    }
                }
            }
        }
        let known = weights
            .iter()
            .flatten()
            .try_fold(Fraction::new(0, 1), |a, b| a.add(*b))
            .ok_or_else(error)?;
        let missing = weights.iter().filter(|v| v.is_none()).count();
        let remainder = i128::from(width)
            .checked_mul(known.d)
            .and_then(|v| v.checked_sub(known.n))
            .ok_or_else(error)?;
        if missing > 0 {
            approximate = true;
            let share = if remainder > 0 {
                Fraction::new(remainder, known.d.checked_mul(missing as i128).ok_or_else(error)?)
            } else {
                Fraction::new(1, 1)
            };
            for weight in &mut weights {
                if weight.is_none() {
                    *weight = Some(share);
                }
            }
        }
        let total = weights
            .iter()
            .flatten()
            .try_fold(Fraction::new(0, 1), |a, b| a.add(*b))
            .ok_or_else(error)?;
        if total.n <= 0 {
            return Err(error());
        }
        approximate |= total.n != i128::from(width) * total.d;
        let mut columns = Vec::with_capacity(weights.len());
        let mut sum = 0_i32;
        for (i, weight) in weights.iter().enumerate() {
            let value = if i + 1 == weights.len() {
                width.checked_sub(sum).ok_or_else(error)?
            } else {
                weight.unwrap().ratio(total, width).ok_or_else(error)?
            };
            if value <= 0 {
                return Err(error());
            }
            sum = sum.checked_add(value).ok_or_else(error)?;
            columns.push(value);
        }
        table.width = Some(HwpUnit::new(width).map_err(|_| error())?);
        let margin = table.in_margin.unwrap_or_default();
        for cell in self.cells {
            let width: i32 = columns[cell.col..cell.col + cell.span].iter().sum();
            approximate |= cell.preferred.is_some_and(|p| p != width);
            let target = &mut table.rows[cell.row].cells[cell.cell];
            target.width = HwpUnit::new(width).map_err(|_| error())?;
            let margin = target.margin.unwrap_or(margin);
            let content = width
                .checked_sub(margin.left.as_i32())
                .and_then(|v| v.checked_sub(margin.right.as_i32()))
                .ok_or_else(error)?;
            if content <= 0 {
                return Err(error());
            }
            apply_pending(
                cell.nested,
                &mut target.paragraphs,
                &cell.anchor_boundaries,
                content,
                part,
                w,
            )?;
        }
        if approximate {
            record(w, Kind::TableWidthApproximated, "table_width", "fit_columns", || {
                (Some(part),Some(&self.at),"Table/column widths use container proportions and final-column rounding remainder")
            })?;
        }
        Ok(())
    }
}
pub(in crate::body) fn apply_pending(
    pending: Vec<(usize, WidthPlan)>,
    paragraphs: &mut Vec<Paragraph>,
    boundaries: &[usize],
    container: i32,
    part: &str,
    w: &mut Vec<DocxWarning>,
) -> Result<Vec<usize>> {
    let synthetic: Vec<_> = pending.iter().map(|(index, _)| *index).collect();
    for (index, plan) in pending {
        let Some(RunContent::Table(table)) =
            paragraphs.get_mut(index).and_then(|p| p.runs.first_mut()).map(|r| &mut r.content)
        else {
            return Err(DocxError::invalid(part, &plan.at, "missing deferred table"));
        };
        plan.apply(table, container, part, w)?;
    }
    Ok(move_anchors(paragraphs, &synthetic, boundaries))
}

// Only deferred plans identify generated holders. All widths, merges and heights
// are settled before their paragraph indices change. Budget counts remain conservative.
fn move_anchors(
    paragraphs: &mut Vec<Paragraph>,
    synthetic: &[usize],
    boundaries: &[usize],
) -> Vec<usize> {
    let mut removed = Vec::new();
    let mut output = Vec::with_capacity(paragraphs.len());
    let mut holders: Vec<(usize, Paragraph)> = Vec::new();
    for (index, mut paragraph) in std::mem::take(paragraphs).into_iter().enumerate() {
        if synthetic.binary_search(&index).is_ok() {
            holders.push((index, paragraph));
            continue;
        }
        if boundaries.binary_search(&index).is_ok() {
            output.extend(holders.drain(..).map(|(_, p)| p));
        } else if !holders.is_empty() {
            let mut runs = Vec::with_capacity(holders.len() + paragraph.runs.len());
            for (index, mut holder) in holders.drain(..) {
                removed.push(index);
                runs.push(holder.runs.remove(0));
            }
            runs.append(&mut paragraph.runs);
            paragraph.runs = runs;
        }
        output.push(paragraph);
    }
    output.extend(holders.into_iter().map(|(_, p)| p));
    *paragraphs = output;
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    // 이것을 실패시키는 것: 비율 분모를 무시하거나 컨테이너 폭을 곱하지 않음.
    #[test]
    fn normalized_half_uses_container_width() {
        assert_eq!(Fraction::new(1, 2).ratio(Fraction::new(1, 1), 1000), Some(500));
    }
}
