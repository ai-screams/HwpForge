//! `convert_and_review` prompt — JSON round-trip editing workflow guide.

use rmcp::model::*;
use rmcp::ErrorData as McpError;

/// Generate the convert-and-review prompt messages.
pub fn get_prompt(
    arguments: &serde_json::Map<String, serde_json::Value>,
) -> Result<GetPromptResult, McpError> {
    let file_path = arguments.get("file_path").and_then(|v| v.as_str()).ok_or_else(|| {
        McpError::invalid_params("Required argument 'file_path' is missing", None)
    })?;

    let edit_instructions = arguments
        .get("edit_instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("(편집 지침 없음 — 구조 확인 후 필요한 수정 진행)");

    let text = format!(
        r#"기존 HWPX 문서를 JSON round-trip 방식으로 편집해주세요.

## 대상 파일
- 파일 경로: {file_path}
- 편집 지침: {edit_instructions}

## 워크플로우 (5단계)

### Step 1: 구조 파악
hwpforge_inspect({{ file_path: "{file_path}" }})
→ 섹션 수, 문단 수, 표/이미지/차트 현황 확인

### Step 2: 대상 섹션 추출
hwpforge_to_json({{ file_path: "{file_path}", section: 0 }})
→ 편집할 섹션을 JSON으로 추출 (section 번호는 Step 1 결과 참고)

### Step 3: JSON 편집
→ 추출된 JSON에서 필요한 수정 수행
→ 문단 텍스트 변경, 추가, 삭제 등
→ 편집 완료 후 JSON을 파일로 저장 (예: edited.json)

### Step 4: 변경 적용
hwpforge_patch({{ base_path: "{file_path}", section: 0, section_json_path: "edited.json", output_path: "result.hwpx" }})
→ 편집된 JSON을 원본에 적용하여 새 HWPX 생성

### Step 5: 결과 검증
hwpforge_validate({{ file_path: "result.hwpx" }})
→ 생성된 문서의 구조/무결성 확인

## 주의사항
- JSON 편집 시 구조(section_index, styles)는 유지
- 이미지 바이너리는 JSON에 포함되지 않으므로 base_path에서 상속됨
- 큰 변경은 섹션 단위로 분할 편집 권장"#
    );

    Ok(GetPromptResult {
        description: Some("HWPX 문서 JSON round-trip 편집 워크플로우".into()),
        messages: vec![PromptMessage::new_text(PromptMessageRole::User, text)],
    })
}
