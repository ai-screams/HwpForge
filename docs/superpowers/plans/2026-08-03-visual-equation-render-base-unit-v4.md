# Visual Equation Render Base Unit V4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 시각 수식 sidecar v4가 부모 도형의 `scaMatrix.e5`를 수식 글꼴 크기에 중복 적용하지 않고 원본 `baseUnit`을 canonical 렌더 기준으로 제공하게 한다.

**Architecture:** `HwpxVisualEquationGeometry`는 raw/display geometry와 scale을 그대로 유지하되 `display_base_unit`을 제거하고 `render_base_unit`을 추가한다. `render_base_unit`은 양수인 equation `baseUnit`을 우선하고, 없거나 0이면 양수인 raw equation height를 사용한다.

**Tech Stack:** Rust 2021, serde/serde_json, Cargo workspace integration tests

## Global Constraints

- sidecar `schema_version`은 4이다.
- raw/display geometry, raw/display position, scale, translation은 유지한다.
- `display_base_unit`은 직렬화 결과에서 제거한다.
- `render_base_unit`은 raw `baseUnit`, 없으면 raw equation height와 같다.
- 실제 Q524와 Q448 wire 값을 회귀 fixture로 검증한다.

---

### Task 1: Schema v4 RED contract

**Files:**

- Modify: `crates/hwpforge-smithy-hwpx/tests/visual_equations.rs`
- Modify: `crates/hwpforge-bindings-cli/tests/cli_integration.rs`

**Interfaces:**

- Consumes: `HwpxDecoder::decode_with_report`, CLI `to-md --json`
- Produces: schema v4와 `render_base_unit` 공개 JSON 계약을 고정하는 테스트

- [x] **Step 1: 실제 Q524 fixture가 `render_base_unit=1100`이고 `display_base_unit`이 없음을 요구한다.**
- [x] **Step 2: 실제 Q448 fixture가 e5와 무관하게 `render_base_unit=900`임을 요구한다.**
- [x] **Step 3: baseUnit이 없을 때 raw equation height fallback을 요구한다.**
- [x] **Step 4: targeted Cargo tests를 실행해 schema v3/display_base_unit 때문에 예상대로 실패하는지 확인한다.**

### Task 2: Minimal schema v4 implementation

**Files:**

- Modify: `crates/hwpforge-smithy-hwpx/src/decoder/visual_equations.rs`

**Interfaces:**

- Consumes: `HxEquation.base_unit`, `HxEquation.sz.height`
- Produces: `HwpxVisualEquationGeometry.render_base_unit: Option<u32>`

- [x] **Step 1: schema version 상수를 4로 올린다.**
- [x] **Step 2: `display_base_unit`과 scale helper를 제거한다.**
- [x] **Step 3: raw baseUnit 우선, raw equation height fallback으로 `render_base_unit`을 계산한다.**
- [x] **Step 4: targeted tests가 통과하는지 확인한다.**

### Task 3: Corpus and workspace verification

**Files:**

- Verify only: actual M1-02 projection and generated visual-equations JSON

**Interfaces:**

- Consumes: HwpForge CLI `to-md`, 실제 Q448/Q524 HWPX projection
- Produces: actual corpus `render_base_unit` assertions and workspace verification evidence

- [x] **Step 1: `cargo fmt --check`와 `cargo clippy --workspace --all-targets --all-features -- -D warnings`를 실행한다.**
- [x] **Step 2: `cargo test --workspace`를 실행한다.**
- [x] **Step 3: actual corpus sidecar에서 Q448=900, Q524=1100, schema=4, `display_base_unit` 부재를 검증한다.**
- [x] **Step 4: diff를 기능 범위로 검토하고 한국어 Conventional Commit으로 커밋한다.**
