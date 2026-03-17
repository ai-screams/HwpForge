//! `generate_report` prompt — Korean report generation workflow.

use rmcp::model::*;
use rmcp::ErrorData as McpError;

/// Generate the report prompt messages.
pub fn get_prompt(
    arguments: &serde_json::Map<String, serde_json::Value>,
) -> Result<GetPromptResult, McpError> {
    let topic = arguments
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::invalid_params("Required argument 'topic' is missing", None))?;

    let author = arguments.get("author").and_then(|v| v.as_str()).unwrap_or("(저자 미지정)");

    let report_type = arguments.get("report_type").and_then(|v| v.as_str()).unwrap_or("research");

    let type_desc = match report_type {
        "progress" => "진행 보고서 — 현재까지의 진행 상황, 이슈, 향후 계획",
        "analysis" => "분석 보고서 — 데이터 기반 현황 분석, 시사점, 제언",
        _ => "연구 보고서 — 서론, 본론, 결론 형식의 체계적 보고",
    };

    let text = format!(
        r#"보고서를 작성해주세요.

## 기본 정보
- 주제: {topic}
- 저자: {author}
- 보고서 유형: {report_type} ({type_desc})

## 표준 목차 구조
1. 표지 (제목, 저자, 날짜)
2. 목차
3. 서론 / 요약 (Executive Summary)
4. 현황 분석 (배경, 데이터, 현재 상태)
5. 주요 결과 / 발견사항
6. 결론 및 제언
7. 참고 문헌

## 작성 규칙
- Markdown으로 작성 (GFM 호환)
- H1(#)은 문서 제목, H2(##)는 장, H3(###)은 절
- 데이터는 GFM 테이블로 정리
- `---`(수평선)으로 장 구분 (페이지 분리)
- 객관적/분석적 어조 사용
- 완성 후 hwpforge_convert로 HWPX 변환

## 변환 명령
마크다운 작성 완료 후:
hwpforge_convert({{ markdown: "<작성한_마크다운>", is_file: false, output_path: "report.hwpx", preset: "default" }})"#
    );

    Ok(GetPromptResult::new(vec![PromptMessage::new_text(PromptMessageRole::User, text)])
        .with_description("보고서 작성 워크플로우"))
}
