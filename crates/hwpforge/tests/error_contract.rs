//! Error contract 0.18: the shared manifest, the generation-site collector
//! and the two coverage comparisons.
//!
//! - manifest ↔ production generation sites: every `CodecStage::<X>`
//!   literal and every audited constructor in production code is registered,
//!   and nothing registered is gone;
//! - manifest ↔ enum inventory: registered variants exist, and every variant
//!   of an in-scope enum (each `Codec` wrapper × every stage) is registered.
//!
//! Expected codes stay literal in the manifest; nothing here derives them.
#![cfg(feature = "ops-hwpx")]

use std::collections::BTreeSet;
use std::fmt::Debug;

use hwpforge::hwpx::{CodecFailure, CodecStage};

#[path = "support/error_contract.rs"]
mod error_contract;
#[path = "support/error_inventory.rs"]
mod error_inventory;

use error_contract::{Manifest, STAGES};
use error_inventory::{Inventory, SiteKey};

/// Enums whose every variant the manifest must register. A wave adds an
/// enum here in the change that registers its cases; until then the
/// inventory comparison only checks that registered variants exist.
const CONTRACT_INVENTORY_SCOPE: &[&str] = &[
    "FillError",
    "PatchError",
    "SectionWorkflowError",
    "CellEditError",
    "StructuralEditError",
    "StamperError",
];

/// Files the design names as generation-site homes; the scan must reach
/// each of them and visit its fns, so an empty result cannot come from a
/// scan that never ran.
const REQUIRED_SITE_FILES: &[&str] = &[
    "crates/hwpforge-smithy-hwpx/src/cell_edit.rs",
    "crates/hwpforge-smithy-hwpx/src/fill.rs",
    "crates/hwpforge-smithy-hwpx/src/layout_carry.rs",
    "crates/hwpforge-smithy-hwpx/src/patch.rs",
    "crates/hwpforge-smithy-hwpx/src/section_workflow.rs",
    "crates/hwpforge-smithy-hwpx/src/stamp/stamper.rs",
    "crates/hwpforge-smithy-hwpx/src/structural.rs",
    "crates/hwpforge/src/ops/mod.rs",
];

// ── the two comparisons (pure, so synthetic inputs can test them) ──

fn registered_sites(manifest: &Manifest) -> Vec<SiteKey> {
    manifest
        .cases
        .iter()
        .flat_map(|case| &case.sites)
        .map(|site| {
            let (kind, name) = match (&site.stage, &site.variant) {
                (Some(stage), _) => ("stage", stage.clone()),
                (None, Some(variant)) => ("variant", variant.clone()),
                (None, None) => ("none", String::new()),
            };
            SiteKey {
                file: site.file.clone(),
                function: site.function.clone(),
                kind: kind.to_owned(),
                name,
                occurrence: site.occurrence,
            }
        })
        .collect()
}

/// Problems between registered and collected generation-site keys.
fn compare_sites(registered: &[SiteKey], collected: &[SiteKey]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut registered_set = BTreeSet::new();
    for key in registered {
        if !registered_set.insert(key) {
            problems.push(format!("registered twice: {key:?}"));
        }
    }
    let collected_set: BTreeSet<&SiteKey> = collected.iter().collect();
    for key in collected_set.difference(&registered_set) {
        problems.push(format!("unregistered generation site: {key:?}"));
    }
    for key in registered_set.difference(&collected_set) {
        problems.push(format!("registered site not in the code: {key:?}"));
    }
    problems
}

/// Problems between the manifest's variant references and the inventory.
fn compare_inventory(
    refs: &[(String, String, Option<String>)],
    inventory: &Inventory,
    scope: &[&str],
) -> Vec<String> {
    let mut problems = Vec::new();
    let variants_of =
        |name: &str| inventory.enums.iter().find(|e| e.name == name).map(|e| &e.variants);
    for (enum_name, variant, _) in refs {
        let exists = variants_of(enum_name).is_some_and(|vs| vs.iter().any(|v| &v.name == variant));
        if !exists {
            problems.push(format!("registered {enum_name}::{variant} is not in the inventory"));
        }
    }
    for enum_name in scope {
        let Some(variants) = variants_of(enum_name) else {
            problems.push(format!("scope enum {enum_name} is not in the inventory"));
            continue;
        };
        for variant in variants {
            let stages: BTreeSet<Option<&str>> = refs
                .iter()
                .filter(|(e, v, _)| e == enum_name && *v == variant.name)
                .map(|(_, _, stage)| stage.as_deref())
                .collect();
            if stages.is_empty() {
                problems.push(format!("{enum_name}::{} is not registered", variant.name));
            } else if variant.name == "Codec" {
                for stage in STAGES {
                    if !stages.contains(&Some(*stage)) {
                        problems.push(format!("{enum_name}::Codec lacks stage {stage}"));
                    }
                }
            }
        }
    }
    problems
}

/// Audited `Enum::Variant` entries the enum inventory does not have.
///
/// Kept apart from the case inventory scope: a misspelt entry would
/// otherwise keep its real constructor out of the site scan unnoticed. The
/// list therefore holds only variants that exist; the expected 0.18 list
/// (five `Invariant`, two `ProtectedFieldRange`, `CellEditError::NestingTooDeep`)
/// is added entry by entry by the wave that introduces each variant.
fn missing_audited_variants(audited: &[String], inventory: &Inventory) -> Vec<String> {
    audited
        .iter()
        .filter(|entry| {
            let Some((enum_name, variant)) = entry.split_once("::") else { return true };
            !inventory
                .enums
                .iter()
                .any(|e| e.name == enum_name && e.variants.iter().any(|v| v.name == variant))
        })
        .map(|entry| format!("audited variant {entry} is not in the enum inventory"))
        .collect()
}

/// The audit scope of 0.18: the variants whose constructors are generation
/// sites once they exist.
fn is_audit_target(enum_name: &str, variant: &str) -> bool {
    match variant {
        "Invariant" => matches!(
            enum_name,
            "FillError" | "PatchError" | "StructuralEditError" | "StamperError" | "CellEditError"
        ),
        "ProtectedFieldRange" => matches!(enum_name, "PatchError" | "StructuralEditError"),
        "NestingTooDeep" => enum_name == "CellEditError",
        _ => false,
    }
}

/// Audit-scope variants that exist in the inventory but are missing from
/// the audited list — the reverse of [`missing_audited_variants`]. Without
/// it, a wave that adds a variant but forgets to list it would leave its
/// constructors out of the site scan unnoticed. Variants that do not exist
/// yet are not required.
fn unlisted_audit_targets(audited: &[String], inventory: &Inventory) -> Vec<String> {
    inventory
        .enums
        .iter()
        .flat_map(|e| e.variants.iter().map(move |v| format!("{}::{}", e.name, v.name)))
        .filter(|entry| {
            let (enum_name, variant) = entry.split_once("::").expect("Enum::Variant");
            is_audit_target(enum_name, variant) && !audited.contains(entry)
        })
        .map(|entry| format!("{entry} exists but is not in audited_variants"))
        .collect()
}

// ── the real manifest against the real code ──────────────────────

#[test]
fn manifest_is_consistent() {
    let manifest = error_contract::load(&error_inventory::workspace_root());
    assert_eq!(error_contract::check(&manifest), Vec::<String>::new());
}

#[test]
fn manifest_sites_match_the_production_generation_sites() {
    // 이것을 실패시키는 것: A5 layout carry의 최종 ZIP 작성 stage를 Verification으로 변경.
    let manifest = error_contract::load(&error_inventory::workspace_root());
    let scan = error_inventory::collect_sites(&manifest.audited_variants);

    // Anti-vacuity: the scan reached every required file and visited its fns.
    // This is what fails when the scan scope is emptied.
    for required in REQUIRED_SITE_FILES {
        let fns = scan.files.iter().find(|(file, _)| file == required).map(|(_, fns)| *fns);
        assert!(fns.is_some_and(|n| n > 0), "{required} not scanned (fns: {fns:?})");
    }

    let registered = registered_sites(&manifest);
    assert_eq!(
        registered.len(),
        scan.sites.len(),
        "registered {} · collected {}",
        registered.len(),
        scan.sites.len()
    );
    assert_eq!(compare_sites(&registered, &scan.sites), Vec::<String>::new());
}

#[test]
fn manifest_inventory_refs_match_the_enum_inventory() {
    // 이것을 실패시키는 것: A4 audited_variants에서 CellEditError::NestingTooDeep 삭제.
    let manifest = error_contract::load(&error_inventory::workspace_root());
    let refs: Vec<(String, String, Option<String>)> = manifest
        .cases
        .iter()
        .flat_map(|case| &case.inventory)
        .map(|r| (r.enum_name.clone(), r.variant.clone(), r.stage.clone()))
        .collect();
    let inventory = error_inventory::collect();
    assert_eq!(
        compare_inventory(&refs, &inventory, CONTRACT_INVENTORY_SCOPE),
        Vec::<String>::new()
    );
    assert_eq!(
        missing_audited_variants(&manifest.audited_variants, &inventory),
        Vec::<String>::new()
    );
    assert_eq!(
        unlisted_audit_targets(&manifest.audited_variants, &inventory),
        Vec::<String>::new()
    );
}

#[test]
fn deleting_a_w2_workflow_reference_is_reported() {
    // 이것을 실패시키는 것: CONTRACT_INVENTORY_SCOPE 에서 SectionWorkflowError 를 빼기.
    // The real manifest with one reference removed: the scope check must name it.
    let manifest = error_contract::load(&error_inventory::workspace_root());
    let refs: Vec<(String, String, Option<String>)> = manifest
        .cases
        .iter()
        .flat_map(|case| &case.inventory)
        .map(|r| (r.enum_name.clone(), r.variant.clone(), r.stage.clone()))
        .filter(|(e, v, _)| !(e == "SectionWorkflowError" && v == "PreservingPatch"))
        .collect();
    let inventory = error_inventory::collect();
    assert_eq!(
        compare_inventory(&refs, &inventory, CONTRACT_INVENTORY_SCOPE),
        vec!["SectionWorkflowError::PreservingPatch is not registered".to_owned()]
    );
}

#[test]
fn deleting_w3_inventory_references_is_reported() {
    // 이것을 실패시키는 것: A1–A3 scope에서 각 W3 enum 삭제, A6 삭제 필터의 &&를 ||로 변경.
    let manifest = error_contract::load(&error_inventory::workspace_root());
    let inventory = error_inventory::collect();
    for (name, variant) in [
        ("CellEditError", "NestingTooDeep"),
        ("StructuralEditError", "ProtectedFieldRange"),
        ("StamperError", "Invariant"),
    ] {
        let refs = manifest
            .cases
            .iter()
            .flat_map(|case| &case.inventory)
            .filter(|r| !(r.enum_name == name && r.variant == variant))
            .map(|r| (r.enum_name.clone(), r.variant.clone(), r.stage.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            compare_inventory(&refs, &inventory, CONTRACT_INVENTORY_SCOPE),
            vec![format!("{name}::{variant} is not registered")]
        );
    }
}

// ── value types ──────────────────────────────────────────────────

#[test]
fn codec_stage_is_the_closed_set_the_manifest_names() {
    // No wildcard: a sixth stage, or `#[non_exhaustive]` on the enum, stops
    // this from compiling. 이것을 실패시키는 것: CodecStage 에 `#[non_exhaustive]`.
    fn name(stage: CodecStage) -> &'static str {
        match stage {
            CodecStage::Decode => "Decode",
            CodecStage::Admission => "Admission",
            CodecStage::Validation => "Validation",
            CodecStage::Encode => "Encode",
            CodecStage::Verification => "Verification",
        }
    }
    let all = [
        CodecStage::Decode,
        CodecStage::Admission,
        CodecStage::Validation,
        CodecStage::Encode,
        CodecStage::Verification,
    ];
    // 이것을 실패시키는 것: STAGES 에서 이름 하나 삭제.
    assert_eq!(all.map(name).as_slice(), STAGES);
}

#[test]
fn codec_failure_keeps_clone_and_eq() {
    // The editor error enums derive Clone + Eq and carry this payload.
    // 이것을 실패시키는 것: CodecFailure 의 Clone 또는 Eq derive 삭제(컴파일 실패).
    fn bounds<T: Clone + Eq + Debug>() {}
    bounds::<CodecFailure>();
    bounds::<CodecStage>();
}

// ── manifest integrity (synthetic) ───────────────────────────────

fn check_text(text: &str) -> Vec<String> {
    error_contract::check(&error_contract::parse(text).expect("parses"))
}

#[test]
fn check_rejects_a_duplicate_case_id() {
    // 이것을 실패시키는 것: check() 의 case id 중복 검사 삭제.
    let problems = check_text(
        r#"{"audited_variants": [], "cases": [
            {"id": "a", "expected_code": "DECODE_FAILED", "surfaces": {}},
            {"id": "a", "expected_code": "DECODE_FAILED", "surfaces": {}}]}"#,
    );
    assert_eq!(problems, ["case id a used twice"]);
}

#[test]
fn check_rejects_a_duplicate_audited_variant() {
    // 이것을 실패시키는 것: check() 의 audited_variants 중복 검사 삭제.
    let problems = check_text(r#"{"audited_variants": ["A::B", "A::B"], "cases": []}"#);
    assert_eq!(problems, ["audited variant A::B listed twice"]);
}

#[test]
fn check_rejects_an_unknown_inventory_stage() {
    // 정상 다섯 stage 에 오타 stage 를 더해도 잡힌다.
    // 이것을 실패시키는 것: check() 의 inventory stage 대조 삭제.
    let refs: Vec<String> = STAGES
        .iter()
        .chain(&["Decod"])
        .map(|stage| {
            format!(r#"{{"enum": "CellEditError", "variant": "Codec", "stage": "{stage}"}}"#)
        })
        .collect();
    let problems = check_text(&format!(
        r#"{{"audited_variants": [], "cases": [{{"id": "a", "expected_code": "X",
            "surfaces": {{}}, "inventory": [{}]}}]}}"#,
        refs.join(",")
    ));
    assert_eq!(
        problems,
        [r#"case a: inventory CellEditError::Codec has unknown stage Some("Decod")"#]
    );
}

#[test]
fn check_requires_a_reason_for_na_and_unreachable_and_an_operation_otherwise() {
    // 이것을 실패시키는 것: Na|Unreachable → reason, Fixture|Injected →
    // operation 갈래 중 하나를 다른 쪽으로 바꾸거나 빈 문자열 검사를 삭제.
    let problems = check_text(
        r#"{"audited_variants": [], "cases": [{"id": "a", "expected_code": "X",
            "surfaces": {
              "rust":   {"mode": "injected", "operation": "fill"},
              "cli":    {"mode": "na", "operation": "fill"},
              "mcp":    {"mode": "unreachable", "reason": "  "},
              "python": {"mode": "fixture", "reason": "why"}}}]}"#,
    );
    assert_eq!(
        problems,
        [
            "case a: cli surface in mode Na lacks its operation or reason",
            "case a: mcp surface in mode Unreachable lacks its operation or reason",
            "case a: python surface in mode Fixture lacks its operation or reason",
        ]
    );
}

#[test]
fn check_requires_exactly_one_known_stage_or_audited_variant_per_site() {
    // 이것을 실패시키는 것: STAGES 소속 검사 또는 audited 소속 검사 삭제,
    // 혹은 (Some, Some)/(None, None) 을 허용하도록 패턴 변경.
    let site = |function: &str, fields: &str| {
        format!(r#"{{"file": "f.rs", "function": "{function}", {fields} "occurrence": 1}}"#)
    };
    let sites = [
        site("ok_stage", r#""stage": "Decode","#),
        site("ok_variant", r#""variant": "P::Invariant","#),
        site("unknown_stage", r#""stage": "Internal","#),
        site("both", r#""stage": "Decode", "variant": "P::Invariant","#),
        site("neither", ""),
        site("unaudited", r#""variant": "P::Other","#),
        site("other_enum", r#""variant": "Q::Invariant","#),
    ]
    .join(",");
    let text = format!(
        r#"{{"audited_variants": ["P::Invariant"], "cases": [{{"id": "a",
            "expected_code": "X", "surfaces": {{}}, "sites": [{sites}]}}]}}"#
    );
    let problems = check_text(&text);
    let flagged: Vec<&str> = problems
        .iter()
        .map(|p| p.split("f.rs::").nth(1).unwrap().split(' ').next().unwrap())
        .collect();
    assert_eq!(flagged, ["unknown_stage", "both", "neither", "unaudited", "other_enum"]);
}

#[test]
fn parse_rejects_unknown_surfaces_modes_and_fields() {
    // 이것을 실패시키는 것: Surfaces·Case 의 `deny_unknown_fields` 삭제.
    let base = |surfaces: &str, extra: &str| {
        format!(
            r#"{{"audited_variants": [], "cases": [{{"id": "a", "expected_code": "X",
                "surfaces": {surfaces}{extra}}}]}}"#
        )
    };
    assert!(error_contract::parse(&base("{}", "")).is_ok());
    assert!(error_contract::parse(&base(r#"{"web": {"mode": "na", "reason": "r"}}"#, "")).is_err());
    assert!(error_contract::parse(&base(r#"{"cli": {"mode": "maybe"}}"#, "")).is_err());
    assert!(error_contract::parse(&base("{}", r#", "design_rows": ["K04"]"#)).is_err());
}

// ── collector (synthetic sources) ────────────────────────────────

fn audited() -> Vec<String> {
    ["PatchError::Invariant", "StamperError::Invariant"].map(String::from).to_vec()
}

fn keys(source: &str) -> Vec<(String, String, String, u32)> {
    let (sites, _) = error_inventory::sites_in_source("f.rs", source, &audited())
        .unwrap_or_else(|p| panic!("{p:#?}"));
    sites.into_iter().map(|s| (s.function, s.kind, s.name, s.occurrence)).collect()
}

fn key(function: &str, kind: &str, name: &str, occurrence: u32) -> (String, String, String, u32) {
    (function.to_owned(), kind.to_owned(), name.to_owned(), occurrence)
}

const SAMPLE: &str = r#"
fn decode(bytes: &[u8]) -> Result<(), PatchError> {
    let a = read(bytes).map_err(|e| fail(CodecStage::Decode, e))?;
    match a.stage {
        CodecStage::Decode => {}
        _ => {}
    }
    if a.stage == CodecStage::Encode || CodecStage::Admission != a.stage {
        return Err(PatchError::Invariant { detail: a.into() });
    }
    let b = parse(a).map_err(PatchError::Invariant)?;
    let c = PatchError::Invariant(CodecStage::Validation);
    let failure = CodecFailure { stage: CodecStage::Decode, code: c, detail: d };
    Ok(())
}
impl StamperError {
    fn invariant(detail: String) -> Self {
        Self::Invariant { detail }
    }
}
"#;

#[test]
fn collector_counts_repeats_closures_fn_values_and_self_but_not_patterns_or_comparisons() {
    // match 팔의 패턴 경로는 syn 이 `Expr` 로 방문하지 않으므로 세지 않는다
    // (별도 조건 없음, 이 표본이 그 사실을 고정한다).
    // 이것을 실패시키는 것: 비교 피연산자 제외 삭제(Encode·Admission 추가),
    // Expr::Path 갈래 삭제(map_err 값 누락),
    // Self 해석 삭제(invariant 누락), Struct 갈래 삭제(Invariant 1 누락),
    // Call 갈래가 callee 를 다시 방문(Invariant 중복).
    assert_eq!(
        keys(SAMPLE),
        [
            key("decode", "stage", "Decode", 1),
            key("decode", "variant", "PatchError::Invariant", 1),
            key("decode", "variant", "PatchError::Invariant", 2),
            key("decode", "variant", "PatchError::Invariant", 3),
            key("decode", "stage", "Validation", 1),
            key("decode", "stage", "Decode", 2),
            key("StamperError::invariant", "variant", "StamperError::Invariant", 1),
        ]
    );
}

#[test]
fn trait_impls_of_one_self_type_get_distinct_keys() {
    // 반례. 이것을 실패시키는 것: 키에서 trait 경로(제네릭 포함)를
    // 빼는 변경 — 두 키가 겹쳐 중복 오류가 난다.
    let source = r"
        impl From<StampError> for StamperError {
            fn from(e: StampError) -> Self { wrap(CodecStage::Encode, e) }
        }
        impl From<CellStampError> for StamperError {
            fn from(e: CellStampError) -> Self { wrap(CodecStage::Encode, e) }
        }
    ";
    assert_eq!(
        keys(source),
        [
            key("StamperError as From<StampError>::from", "stage", "Encode", 1),
            key("StamperError as From<CellStampError>::from", "stage", "Encode", 1),
        ]
    );
}

#[test]
fn nested_fns_get_their_own_key_and_ordinal() {
    // 이것을 실패시키는 것: in_fn 에서 순번 카운터를 비우지 않음(inner 가 2 부터)
    // 또는 복원하지 않음(outer 의 두 번째가 3).
    let source = r"
        fn outer() {
            let _ = CodecStage::Decode;
            fn inner() { let _ = (CodecStage::Decode, CodecStage::Decode); }
            let _ = CodecStage::Decode;
        }
    ";
    assert_eq!(
        keys(source),
        [
            key("outer", "stage", "Decode", 1),
            key("outer::inner", "stage", "Decode", 1),
            key("outer::inner", "stage", "Decode", 2),
            key("outer", "stage", "Decode", 2),
        ]
    );
}

#[test]
fn test_only_code_is_not_scanned() {
    // 이것을 실패시키는 것: is_test_only 의 `#[test]` 또는 `#[cfg(test)]` 판정 삭제,
    // 또는 visit_item_mod·visit_item_fn·visit_item_impl·visit_impl_item_fn 의 건너뛰기 삭제.
    let source = r"
        #[cfg(test)]
        mod tests { fn helper() { let _ = CodecStage::Decode; } }
        #[test]
        fn case() { let _ = CodecStage::Encode; }
        #[cfg(test)]
        impl Probe { fn helper() { let _ = CodecStage::Admission; } }
        impl Probe {
            #[cfg(test)]
            fn probe() { let _ = CodecStage::Decode; }
            fn live() { let _ = CodecStage::Validation; }
        }
        fn live() { let _ = CodecStage::Verification; }
    ";
    assert_eq!(
        keys(source),
        [key("Probe::live", "stage", "Validation", 1), key("live", "stage", "Verification", 1)]
    );
}

#[test]
fn collector_fails_on_duplicate_keys_macros_and_sites_outside_fns() {
    let problems = |source: &str| {
        error_inventory::sites_in_source("f.rs", source, &audited()).expect_err("must fail")
    };
    // 이것을 실패시키는 것: sites_in_source 의 중복 키 검사 삭제.
    let duplicate = problems(
        r"
        impl Codec { fn run() { let _ = CodecStage::Decode; } }
        impl Codec { fn run() { let _ = CodecStage::Decode; } }
        ",
    );
    assert_eq!(duplicate.len(), 1, "{duplicate:?}");
    assert!(duplicate[0].starts_with("duplicate generation-site key"), "{duplicate:?}");

    // 이것을 실패시키는 것: visit_macro 의 토큰 검사 삭제, flat_tokens 의 그룹 재귀 삭제.
    let in_macro = problems("fn f() { let v = vec![Err(PatchError::Invariant { detail })]; }");
    assert_eq!(in_macro.len(), 1, "{in_macro:?}");
    assert!(in_macro[0].contains("inside `vec!`"), "{in_macro:?}");

    // 이것을 실패시키는 것: record() 의 fn 밖 검사 삭제.
    let outside = problems("const S: CodecStage = CodecStage::Decode;");
    assert_eq!(outside, ["f.rs: stage Decode outside any fn"]);

    // An unrelated macro is fine.
    assert!(error_inventory::sites_in_source(
        "f.rs",
        r#"fn f() { println!("{}", 1); }"#,
        &audited()
    )
    .is_ok());
}

// ── comparisons (synthetic) ──────────────────────────────────────

fn site_key(function: &str, name: &str, occurrence: u32) -> SiteKey {
    SiteKey {
        file: "f.rs".into(),
        function: function.into(),
        kind: "stage".into(),
        name: name.into(),
        occurrence,
    }
}

#[test]
fn compare_sites_reports_missing_stale_reordered_and_doubled_registrations() {
    // 이것을 실패시키는 것: compare_sites 의 세 검사(중복·미등록·잔존) 중 하나 삭제.
    let collected =
        [site_key("a", "Decode", 1), site_key("a", "Decode", 2), site_key("b", "Encode", 1)];
    assert_eq!(compare_sites(&collected, &collected), Vec::<String>::new());

    let registered = [
        site_key("a", "Decode", 1),
        site_key("a", "Decode", 1),
        site_key("a", "Decode", 3), // wrong ordinal
        site_key("gone", "Encode", 1),
    ];
    let problems = compare_sites(&registered, &collected);
    assert_eq!(problems.len(), 5, "{problems:#?}");
    assert_eq!(problems.iter().filter(|p| p.starts_with("registered twice")).count(), 1);
    assert_eq!(problems.iter().filter(|p| p.starts_with("unregistered")).count(), 2);
    assert_eq!(problems.iter().filter(|p| p.starts_with("registered site not")).count(), 2);
}

fn synthetic_inventory() -> Inventory {
    let inventory = r#"{"enums": [{"krate": "k", "file": "f.rs", "name": "EditError",
        "visibility": "pub", "non_exhaustive": true, "wrapped": true, "variants": [
          {"name": "Codec", "shape": "tuple", "cfg": [], "discriminant": null},
          {"name": "Invariant", "shape": "struct", "cfg": [], "discriminant": null}]}],
        "unlisted_public_error_or_warning_enums": []}"#;
    serde_json::from_str(inventory).expect("inventory")
}

fn inventory_ref(variant: &str, stage: Option<&str>) -> (String, String, Option<String>) {
    ("EditError".to_owned(), variant.to_owned(), stage.map(str::to_owned))
}

#[test]
fn compare_inventory_reports_missing_variants_missing_stages_and_retired_refs() {
    let inventory = synthetic_inventory();
    let mut full: Vec<_> = STAGES.iter().map(|s| inventory_ref("Codec", Some(s))).collect();
    full.push(inventory_ref("Invariant", None));
    assert_eq!(compare_inventory(&full, &inventory, &["EditError"]), Vec::<String>::new());

    // 이것을 실패시키는 것: scope 순회의 미등록 variant 검사 삭제.
    let without_invariant = &full[..STAGES.len()];
    assert_eq!(
        compare_inventory(without_invariant, &inventory, &["EditError"]),
        ["EditError::Invariant is not registered"]
    );

    // 이것을 실패시키는 것: Codec × stage 검사 삭제.
    let without_verification: Vec<_> =
        full.iter().filter(|r| r.2.as_deref() != Some("Verification")).cloned().collect();
    assert_eq!(
        compare_inventory(&without_verification, &inventory, &["EditError"]),
        ["EditError::Codec lacks stage Verification"]
    );

    // 이것을 실패시키는 것: 등록 variant 존재 검사 삭제 (퇴역 variant 잔존).
    let mut retired = full.clone();
    retired.push(inventory_ref("Retired", None));
    assert_eq!(
        compare_inventory(&retired, &inventory, &["EditError"]),
        ["registered EditError::Retired is not in the inventory"]
    );

    // 이것을 실패시키는 것: scope enum 이 inventory 에 없을 때의 보고 삭제.
    assert_eq!(
        compare_inventory(&[], &inventory, &["GoneError"]),
        ["scope enum GoneError is not in the inventory"]
    );

    // Out of scope, only existence is checked.
    assert_eq!(compare_inventory(&[], &inventory, &[]), Vec::<String>::new());
}

#[test]
fn production_module_tree_leaves_out_test_modules() {
    // 이것을 실패시키는 것: collect_out_of_line_mods 의 `skip_test` 건너뛰기 삭제.
    let dir = std::env::temp_dir().join(format!("hwpforge-site-tree-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("lib.rs"), "mod live;\n#[cfg(test)]\nmod tests;\n").expect("lib.rs");
    std::fs::write(dir.join("live.rs"), "").expect("live.rs");
    std::fs::write(dir.join("tests.rs"), "").expect("tests.rs");
    let names = |files: Vec<std::path::PathBuf>| -> Vec<String> {
        files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect()
    };
    let production = names(error_inventory::production_module_tree(&dir.join("lib.rs")));
    let all = names(error_inventory::module_tree(&dir.join("lib.rs")));
    std::fs::remove_dir_all(&dir).expect("clean up");
    assert_eq!(production, ["lib.rs", "live.rs"]);
    assert_eq!(all, ["lib.rs", "live.rs", "tests.rs"]);
}

#[test]
fn audited_variants_must_exist_in_the_enum_inventory() {
    // 오타 반례(`Invarient`). 이것을 실패시키는 것: missing_audited_variants 의
    // 존재 대조를 항상 통과로 바꿈.
    let inventory = synthetic_inventory();
    let audited =
        ["EditError::Invariant", "EditError::Invarient", "OtherError::Invariant"].map(String::from);
    assert_eq!(
        missing_audited_variants(&audited, &inventory),
        [
            "audited variant EditError::Invarient is not in the enum inventory",
            "audited variant OtherError::Invariant is not in the enum inventory",
        ]
    );
}

#[test]
fn compared_operands_are_skipped_under_parens_and_references() {
    // 중첩 비교: 괄호·중첩 괄호·`&` 아래의 비교 피연산자는 생성점이
    // 아니다. 비교식 안의 호출 인자로 실제 생성되는 stage 는 센다.
    // 이것을 실패시키는 것: is_compared_path 의 Paren 갈래 삭제(Decode·Admission
    // 추가) 또는 Reference 갈래 삭제(Encode 추가), 비교식 피연산자를 통째로
    // 건너뜀(Verification 누락).
    let source = r"
        fn f(stage: CodecStage) -> bool {
            stage == (CodecStage::Decode)
                || &stage != &CodecStage::Encode
                || stage == ((CodecStage::Admission))
                || stage == stage_of(CodecStage::Verification)
        }
    ";
    assert_eq!(keys(source), [key("f", "stage", "Verification", 1)]);
}

#[test]
fn self_resolves_before_the_stage_check() {
    // 타입 해석 반례. 이것을 실패시키는 것: classify_pair 에서 `Self` 해석을
    // CodecStage 판정 뒤로 옮김.
    let source = r"impl CodecStage { fn input() -> Self { Self::Decode } }";
    assert_eq!(keys(source), [key("CodecStage::input", "stage", "Decode", 1)]);
}

#[test]
fn audit_targets_in_the_inventory_must_be_listed() {
    // 누락 등록 반례: variant 는 추가했지만 감사 목록 등록을 빠뜨림.
    // 이것을 실패시키는 것: unlisted_audit_targets 의 목록 대조 삭제(항상 빈 결과).
    let inventory: Inventory = serde_json::from_str(
        r#"{"enums": [{"krate": "k", "file": "f.rs", "name": "FillError",
            "visibility": "pub", "non_exhaustive": false, "wrapped": true, "variants": [
              {"name": "Workflow", "shape": "struct", "cfg": [], "discriminant": null},
              {"name": "Invariant", "shape": "struct", "cfg": [], "discriminant": null}]}],
            "unlisted_public_error_or_warning_enums": []}"#,
    )
    .expect("inventory");
    assert_eq!(
        unlisted_audit_targets(&[], &inventory),
        ["FillError::Invariant exists but is not in audited_variants"]
    );
    assert_eq!(
        unlisted_audit_targets(&["FillError::Invariant".to_owned()], &inventory),
        Vec::<String>::new()
    );
}
