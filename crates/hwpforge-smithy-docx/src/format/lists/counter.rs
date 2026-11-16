use crate::error::Result;
use crate::DocxError;

#[derive(Clone)]
pub(super) struct CounterState {
    pub values: [u64; 10],
    pub used: [bool; 10],
    pub starts: [u32; 10],
}
impl CounterState {
    pub fn new(starts: [u32; 10]) -> Self {
        Self { values: [0; 10], used: [false; 10], starts }
    }
    pub fn advance_word(
        &mut self,
        level: usize,
        reset: Option<u32>,
        restarts: &[Option<u32>; 10],
        part: &str,
    ) -> Result<()> {
        // 이것을 실패시키는 것: 미사용 상위의 기본 시작값 또는 사용 처리를 생략.
        for i in 0..level {
            if !self.used[i] {
                self.values[i] = u64::from(self.starts[i]);
                self.used[i] = true;
            }
        }
        self.values[level] = if let Some(value) = reset {
            u64::from(value)
        } else if self.used[level] {
            self.values[level]
                .checked_add(1)
                .ok_or_else(|| DocxError::invalid(part, "numPr", "source numbering overflow"))?
        } else {
            u64::from(self.starts[level])
        };
        self.used[level] = true;
        // 이것을 실패시키는 것: 0을 기본 재시작으로 취급하거나 유효 임계값 대신 bool로 축약.
        for (child, restart) in restarts.iter().enumerate().skip(level + 1) {
            let threshold = match restart {
                Some(0) => 0,
                Some(value) if *value <= child as u32 => *value as usize,
                _ => child,
            };
            if level < threshold {
                self.used[child] = false;
            }
        }
        Ok(())
    }
    pub fn advance_core(&mut self, level: usize, part: &str) -> Result<()> {
        // 출력 정의는 모든 하위를 자기 시작값으로 다시 시작한다.
        for i in 0..level {
            if !self.used[i] {
                self.values[i] = u64::from(self.starts[i].max(1));
                self.used[i] = true;
            }
        }
        self.values[level] = if self.used[level] {
            self.values[level]
                .checked_add(1)
                .ok_or_else(|| DocxError::invalid(part, "numPr", "output numbering overflow"))?
        } else {
            u64::from(self.starts[level].max(1))
        };
        self.used[level] = true;
        self.used[level + 1..].fill(false);
        Ok(())
    }
    pub fn segment_starts(
        values: [u64; 10],
        seeds: [u32; 10],
        level: usize,
        part: &str,
    ) -> Result<[u32; 10]> {
        let mut starts = [1; 10];
        for (i, start) in starts.iter_mut().enumerate() {
            // 이것을 실패시키는 것: 현재 상위를 다음 값으로 설정하거나 대기 하위 override를 버림.
            *start = if i <= level {
                u32::try_from(values[i])
                    .map_err(|_| DocxError::invalid(part, "numPr", "numbering start exceeds u32"))?
            } else {
                seeds[i]
            }
            .max(1);
        }
        Ok(starts)
    }
}
