//! `generate_proposal` prompt — Korean government proposal generation workflow.

use rmcp::model::*;
use rmcp::ErrorData as McpError;

/// Generate the proposal prompt messages.
pub fn get_prompt(
    arguments: &serde_json::Map<String, serde_json::Value>,
) -> Result<GetPromptResult, McpError> {
    let topic = arguments
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::invalid_params("Required argument 'topic' is missing", None))?;

    let organization =
        arguments.get("organization").and_then(|v| v.as_str()).unwrap_or("(기관명 미지정)");

    let deadline = arguments.get("deadline").and_then(|v| v.as_str()).unwrap_or("(기한 미지정)");

    let text = format!(
        r#"한국 정부 제안서를 작성해주세요.

## 기본 정보
- 주제: {topic}
- 제안 기관: {organization}
- 제출 기한: {deadline}

## 표준 목차 구조
1. 표지
2. 목차
3. 사업 이해 (배경, 현황 분석, 문제점)
4. 수행 방안 (추진 전략, 세부 계획, 산출물)
5. 수행 조직 (조직도, 투입 인력)
6. 관리 방안 (일정, 품질, 위험)
7. 참고 자료

## 작성 규칙
- Markdown으로 작성 (GFM 호환)
- H1(#)은 표지 제목에만, H2(##)는 장, H3(###)은 절
- 표는 GFM 테이블 문법 사용
- `---`(수평선)으로 장 구분 (페이지 분리)
- 완성 후 hwpforge_convert로 HWPX 변환

## 변환 명령
마크다운 작성 완료 후:
hwpforge_convert({{ markdown: "<작성한_마크다운>", is_file: false, output_path: "proposal.hwpx", preset: "default" }})"#
    );

    Ok(GetPromptResult {
        description: Some("한국 정부 RFP 제안서 작성 워크플로우".into()),
        messages: vec![PromptMessage::new_text(PromptMessageRole::User, text)],
    })
}
