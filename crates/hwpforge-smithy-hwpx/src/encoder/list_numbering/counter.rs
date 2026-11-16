use crate::paragraph_measure::invalid;
use crate::HwpxResult;
#[derive(Clone, Debug)]
pub(super) struct CounterState {
    pub values: [u64; 10],
    pub used: [bool; 10],
    starts: [u32; 10],
}
impl CounterState {
    pub fn new(starts: [u32; 10]) -> Self {
        Self { values: [0; 10], used: [false; 10], starts }
    }
    pub fn advance(&mut self, level: usize) -> HwpxResult<()> {
        if level >= 10 {
            return Err(invalid("list counter level exceeds ten"));
        }
        for i in 0..level {
            if !self.used[i] {
                self.values[i] = u64::from(self.starts[i].max(1));
                self.used[i] = true;
            }
        }
        self.values[level] = if self.used[level] {
            self.values[level].checked_add(1).ok_or_else(|| invalid("list counter overflow"))?
        } else {
            u64::from(self.starts[level].max(1))
        };
        self.used[level] = true;
        for used in &mut self.used[level + 1..] {
            *used = false;
        }
        Ok(())
    }
    pub fn references(&self, text: &str) -> Vec<(usize, u64)> {
        tokens(text).into_iter().map(|(i, _)| (i, self.values[i])).collect()
    }
    pub fn segment_starts(&self, level: usize) -> Option<[u32; 10]> {
        let mut starts = self.starts;
        for (i, start) in starts.iter_mut().enumerate().take(level + 1) {
            if self.used[i] {
                *start = u32::try_from(self.values[i]).ok()?;
            }
        }
        Some(starts)
    }
}
pub(in crate::encoder) fn tokens(text: &str) -> Vec<(usize, std::ops::Range<usize>)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'^' && (b'1'..=b'9').contains(&b[i + 1]) {
            let start = i;
            let mut n = usize::from(b[i + 1] - b'0');
            i += 2;
            if n == 1 && b.get(i) == Some(&b'0') {
                n = 10;
                i += 1;
            }
            out.push((n - 1, start..i));
        } else {
            i += 1;
        }
    }
    out
}
