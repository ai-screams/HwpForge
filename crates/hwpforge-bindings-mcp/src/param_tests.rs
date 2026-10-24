//! Router-level WM-A fixtures; expected codes come only from the shared manifest.
use super::*;
use rmcp::ServiceExt;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn call(name: &str, arguments: Value) -> Result<CallToolResult, McpError> {
    let (client, server_io) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server_io);
    let service = tokio::spawn(async move {
        HwpForgeServer::new()
            .serve(crate::bounded_transport(sr, sw, crate::MAX_LINE))
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let (cr, mut cw) = tokio::io::split(client);
    let mut reader = BufReader::new(cr).lines();
    cw.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"WM-A\",\"version\":\"0\"}}}\n").await.unwrap();
    reader.next_line().await.unwrap().unwrap();
    cw.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let request = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":arguments}});
    cw.write_all(format!("{request}\n").as_bytes()).await.unwrap();
    let response = read_call_response(&mut reader).await;
    cw.shutdown().await.unwrap();
    service.await.unwrap();
    match response.get("error") {
        Some(error) => Err(serde_json::from_value(error.clone()).unwrap()),
        None => Ok(serde_json::from_value(response["result"].clone()).unwrap()),
    }
}

async fn read_call_response(
    reader: &mut tokio::io::Lines<impl tokio::io::AsyncBufRead + Unpin>,
) -> Value {
    loop {
        let response: Value =
            serde_json::from_str(&reader.next_line().await.unwrap().unwrap()).unwrap();
        if response["id"] == 2 {
            return response;
        }
    }
}

#[tokio::test]
// 이것을 실패시키는 것: 첫 줄을 응답으로 반환하거나 id == 2 조건을 반전.
async fn wm_a_call_response_skips_notifications_and_other_ids() {
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[]}}\n",
    );
    let response = read_call_response(&mut BufReader::new(input.as_bytes()).lines()).await;
    assert_eq!(response, json!({"jsonrpc":"2.0","id":2,"result":{"content":[]}}));
}

#[tokio::test]
// 이것을 실패시키는 것: 알 수 없는 도구를 Ok 도구 오류로 바꾸거나 프로토콜 코드를 변경.
async fn wm_a_unknown_tool_stays_protocol_error() {
    let error = call("hwpforge_unknown_tool", json!({})).await.unwrap_err();
    assert_eq!(error.code.0, -32602);
}

#[test]
// 이것을 실패시키는 것: 도구의 CheckedParams를 우회하면서 manifest case를 제거,
// 또는 invalid_params fixture 키만 제거. 실제 라우터가 도구 목록의 정본이다.
fn wm_a_param_case_sets_cover_router() {
    use std::collections::BTreeSet;

    let tools: BTreeSet<_> = HwpForgeServer::tool_router()
        .list_all()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    let manifest: Value =
        serde_json::from_str(include_str!("../../hwpforge/tests/data/error_contract_018.json"))
            .unwrap();
    let cases: BTreeSet<_> = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["id"].as_str().unwrap().starts_with("params."))
        .map(|case| {
            format!(
                "hwpforge_{}",
                case["surfaces"]["mcp"]["operation"].as_str().unwrap().replace('-', "_")
            )
        })
        .collect();
    let fixtures: Value =
        serde_json::from_str(include_str!("../tests/data/invalid_params.json")).unwrap();
    let inputs: BTreeSet<_> = fixtures
        .as_object()
        .unwrap()
        .keys()
        .map(|id| format!("hwpforge_{}", id.strip_prefix("params.").unwrap().replace('-', "_")))
        .collect();
    assert_eq!(tools, cases, "every routed tool needs a params.* manifest case");
    assert_eq!(tools, inputs, "every routed tool needs invalid parameter inputs");
}

// 이것을 실패시키는 것: 각 도구 CheckedParams 우회, 경로 삭제·최상위 '.' 유지,
// 누락 메시지 삭제, INVALID_PARAMS 코드 변경, 성공 봉투 반환, manifest fixture 미소비.
// 부재 입력보다 인자 오류가 먼저이고 출력 sentinel도 그대로여야 한다.
pub(crate) async fn assert_invalid_case(case: &Value) -> ToolErrorInfo {
    let fixtures: Value =
        serde_json::from_str(include_str!("../tests/data/invalid_params.json")).unwrap();
    let id = case["id"].as_str().unwrap();
    assert_eq!(case["surfaces"]["mcp"]["mode"], "fixture");
    let name = format!(
        "hwpforge_{}",
        case["surfaces"]["mcp"]["operation"].as_str().unwrap().replace('-', "_")
    );
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("missing.hwpx");
    let output = dir.path().join("existing.hwpx");
    std::fs::write(&output, b"do not overwrite").unwrap();
    let mut last = None;
    for fixture in fixtures[id]["invalid"].as_array().expect("every params case has inputs") {
        let mut args = fixture["arguments"].clone();
        for value in args.as_object_mut().unwrap().values_mut() {
            match value.as_str() {
                Some("INPUT") => *value = json!(input),
                Some("OUTPUT") => *value = json!(output),
                _ => {}
            }
        }
        let result = call(&name, args).await.unwrap();
        assert_eq!(result.is_error, Some(true), "{id}");
        let wire: Value = serde_json::to_value(result).unwrap();
        let error: Value = serde_json::from_str(wire["content"][0]["text"].as_str().unwrap())
            .unwrap_or_else(|e| panic!("{id}: expected structured error: {e}; {wire}"));
        assert_eq!(error["code"], case["expected_code"], "{id}");
        assert!(
            error["message"].as_str().unwrap().starts_with(fixture["message"].as_str().unwrap()),
            "{id}: {error}"
        );
        assert_eq!(error.as_object().unwrap().len(), 3, "no public path or extra envelope");
        assert_eq!(std::fs::read(&output).unwrap(), b"do not overwrite");
        last = Some(ToolErrorInfo {
            code: error["code"].as_str().unwrap().into(),
            message: error["message"].as_str().unwrap().into(),
            hint: error["hint"].as_str().unwrap().into(),
        });
    }
    last.unwrap()
}

#[test]
// 이것을 실패시키는 것: CheckedParams의 json_schema를 빈 schema로 변경.
fn wm_a_router_schemas_preserved() {
    let actual: std::collections::BTreeMap<_, _> = HwpForgeServer::tool_router()
        .list_all()
        .into_iter()
        .map(|tool| (tool.name.to_string(), Value::Object((*tool.input_schema).clone())))
        .collect();
    let mut expected: std::collections::BTreeMap<String, Value> =
        serde_json::from_str(include_str!("../tests/data/tool_input_schemas.json")).unwrap();
    // Reuse the two existing wire baselines instead of keeping second copies.
    expected.extend([
        (
            "hwpforge_stamp".into(),
            serde_json::from_str(include_str!("../tests/data/stamp_request_schema.json")).unwrap(),
        ),
        (
            "hwpforge_set_cell".into(),
            serde_json::from_str(include_str!("../tests/data/set_cell_request_schema.json"))
                .unwrap(),
        ),
    ]);
    assert_eq!(actual, expected);
}

#[tokio::test]
// 이것을 실패시키는 것: CheckedParams가 정상 Request까지 Err로 바꿈, convert의 is_file 기본값 false.
// 모든 도구의 성공 추출 갈래를 통과하며, 이후 도메인 오류는 기존 코드여야 한다.
async fn wm_a_valid_params_reach_tools() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../tests/data/invalid_params.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (id, fixture) in fixtures.as_object().unwrap() {
        let name = format!("hwpforge_{}", id.strip_prefix("params.").unwrap().replace('-', "_"));
        let mut args = fixture["valid"].clone();
        for value in args.as_object_mut().unwrap().values_mut() {
            match value.as_str() {
                Some("INPUT") => *value = json!(dir.path().join("missing.hwpx")),
                Some("OUTPUT") => *value = json!(dir.path().join("output.hwpx")),
                _ => {}
            }
        }
        let result = call(&name, args).await.unwrap();
        let wire = serde_json::to_value(result).unwrap();
        let data: Value =
            serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_ne!(data["code"], "INVALID_PARAMS", "{id}: {data}");
        assert!(
            !dir.path().join("output.hwpx").exists(),
            "{id}: input must be read, not rendered as inline Markdown"
        );
    }
}

#[test]
// 이것을 실패시키는 것: schema_name/schema_id/inline_schema 위임 변경, preset 기본값 변경.
fn wm_a_schema_identity_and_serde_defaults() {
    fn same_identity<T: JsonSchema>() {
        assert_eq!(CheckedParams::<T>::schema_name(), T::schema_name());
        assert_eq!(CheckedParams::<T>::schema_id(), T::schema_id());
        assert_eq!(CheckedParams::<T>::inline_schema(), T::inline_schema());
    }
    same_identity::<ConvertRequest>();
    same_identity::<String>();
    let CheckedParams(request) = serde_json::from_value::<CheckedParams<ConvertRequest>>(json!({
        "markdown": "input.md", "output_path": "output.hwpx", "unknown": true
    }))
    .unwrap();
    let request = request.unwrap();
    assert!(request.is_file);
    assert_eq!(request.preset, "default");
}

#[test]
// 이것을 실패시키는 것: 인자 오류 반환보다 먼저 spawn_blocking을 기다리는 변경.
// 유일한 blocking worker를 점유해 두면 19개 도구의 잘못된 인자는 여전히 즉시 끝나야 한다.
fn wm_a_invalid_params_do_not_start_workers() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (started, ready) = tokio::sync::oneshot::channel();
        let worker = tokio::task::spawn_blocking(move || {
            started.send(()).unwrap();
            let _ = wait.recv();
        });
        ready.await.unwrap();
        let manifest: Value =
            serde_json::from_str(include_str!("../../hwpforge/tests/data/error_contract_018.json"))
                .unwrap();
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["id"].as_str().unwrap().starts_with("params."))
        {
            tokio::time::timeout(std::time::Duration::from_secs(2), assert_invalid_case(case))
                .await
                .expect("invalid params waited for the blocking pool");
        }
        // Dropping this sender on assertion failure also releases the worker.
        drop(release);
        worker.await.unwrap();
    });
}
