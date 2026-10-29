// Default comparison exercises the shared production statements with no configuration.
#[cfg(feature = "resource-experiment")]
use hwpforge_core::resource_experiment::*;
use hwpforge_smithy_md::MdDecoder;

// 이것을 실패시키는 것: Local 누적, Measured 검사 지연, 완료 Q 과금 생략.
#[cfg(feature = "resource-experiment")]
#[test]
fn note_timing_scope_and_table() {
    for input in
        ["x[^a] y[^a]\n\n[^a]: abcd\n", "| a | b |\n|---|---|\n| x[^a] | y[^a] |\n\n[^a]: abcd\n"]
    {
        for timing in [Timing::Measured, Timing::Post] {
            for scope in [Scope::Local, Scope::Document] {
                // 이것을 실패시키는 것: 허용 경계의 >를 >=로 바꾸기.
                set_experiment(Experiment { timing, scope, budget: 4 });
                assert!(MdDecoder::decode_with_default("x[^a]\n\n[^a]: abcd\n").is_ok());
                assert_eq!(snapshot().q, 4);
                assert_eq!(snapshot().first_reject_q, None);
                clear_experiment();
                set_experiment(Experiment { timing, scope, budget: 6 });
                let result = MdDecoder::decode_with_default(input);
                assert_eq!(result.is_err(), scope == Scope::Document);
                let q = if timing == Timing::Measured && scope == Scope::Document { 4 } else { 8 };
                assert_eq!(snapshot().q, q);
                assert_eq!(snapshot().first_reject_q, (scope == Scope::Document).then_some(q));
                clear_experiment();
            }
        }
    }
}

// 이것을 실패시키는 것: 실험 경로에서 각주 복제 또는 검증 결과 변경, 예산을 1바이트로 낮추기.
#[test]
fn normal_note_equivalence() {
    let mut values = Vec::new();
    for input in [
        "hello[^a] again[^a]\n\n[^a]: ASCII text\n",
        "| note |\n|---|\n| ref[^a] |\n\n[^a]: ASCII text\n",
    ] {
        let decode = || {
            let d = MdDecoder::decode_with_default(input).unwrap();
            serde_json::json!({"validation":format!("{:?}",d.document.clone().validate().map(|_| ())),"document":d.document,"styles":d.style_registry,"warnings":format!("{:?}",d.warnings)})
        };
        let expected = decode();
        #[cfg(feature = "resource-experiment")]
        for timing in [Timing::Measured, Timing::Post] {
            for scope in [Scope::Local, Scope::Document] {
                set_experiment(Experiment { timing, scope, budget: 8 * 1024 * 1024 });
                assert_eq!(decode(), expected);
                clear_experiment();
            }
        }
        values.push(expected);
    }
    if let Ok(path) = std::env::var("RESOURCE_MD_BASELINE") {
        let value = serde_json::to_value(values).unwrap();
        if !cfg!(feature = "resource-experiment") {
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        #[cfg(feature = "resource-experiment")]
        assert_eq!(
            value,
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap()
        );
    }
}

// 이것을 실패시키는 것: Local 비용을 0으로, Measured Local을 Post로 변경.
#[cfg(feature = "resource-experiment")]
#[test]
fn oversized_note() {
    for timing in [Timing::Measured, Timing::Post] {
        for scope in [Scope::Local, Scope::Document] {
            set_experiment(Experiment { timing, scope, budget: 4 });
            assert!(MdDecoder::decode_with_default("x[^a]\n\n[^a]: abcdefgh\n").is_err());
            let q = if timing == Timing::Measured { 0 } else { 8 };
            assert_eq!(snapshot().q, q);
            assert_eq!(snapshot().first_reject_q, Some(q));
            clear_experiment();
        }
    }
}
