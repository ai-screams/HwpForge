//! 텍스트 층 — 셰이핑([`shape`])과 정렬 배분([`align`]).
//!
//! lineseg 의 `horzpos/horzsize` 는 정렬을 반영하지 않는다 (W0 실측 —
//! blank-HPC·rules-table 표B). 렌더러가 자연폭을 셰이핑으로 구해 잉여폭을
//! 배분해야 하며, 그 규칙이 이 층에 산다.

pub mod align;
pub mod shape;
