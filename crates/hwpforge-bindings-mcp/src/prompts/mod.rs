//! MCP Prompt definitions for HwpForge.
//!
//! Provides workflow-oriented prompts that guide AI agents through
//! common document creation and editing tasks.

use rmcp::model::*;
use rmcp::ErrorData as McpError;

mod convert_review;
mod proposal;
mod report;

/// List all available prompts.
pub fn list_prompts() -> Result<ListPromptsResult, McpError> {
    Ok(ListPromptsResult {
        prompts: vec![
            Prompt {
                name: "generate_proposal".into(),
                title: Some("정부 제안서 생성".into()),
                description: Some(
                    "한국 정부 RFP 제안서를 마크다운으로 작성하고 HWPX로 변환하는 워크플로우"
                        .into(),
                ),
                arguments: Some(vec![
                    PromptArgument {
                        name: "topic".into(),
                        title: Some("제안서 주제".into()),
                        description: Some("제안서의 주제 또는 사업명".into()),
                        required: Some(true),
                    },
                    PromptArgument {
                        name: "organization".into(),
                        title: Some("제안 기관".into()),
                        description: Some("제안 기관/회사명 (선택)".into()),
                        required: Some(false),
                    },
                    PromptArgument {
                        name: "deadline".into(),
                        title: Some("제출 기한".into()),
                        description: Some("제출 기한 (YYYY-MM-DD, 선택)".into()),
                        required: Some(false),
                    },
                ]),
                icons: None,
                meta: None,
            },
            Prompt {
                name: "generate_report".into(),
                title: Some("보고서 생성".into()),
                description: Some(
                    "연구/진행/분석 보고서를 마크다운으로 작성하고 HWPX로 변환하는 워크플로우"
                        .into(),
                ),
                arguments: Some(vec![
                    PromptArgument {
                        name: "topic".into(),
                        title: Some("보고서 주제".into()),
                        description: Some("보고서의 주제".into()),
                        required: Some(true),
                    },
                    PromptArgument {
                        name: "author".into(),
                        title: Some("저자".into()),
                        description: Some("보고서 저자 (선택)".into()),
                        required: Some(false),
                    },
                    PromptArgument {
                        name: "report_type".into(),
                        title: Some("보고서 유형".into()),
                        description: Some(
                            "보고서 종류: research/progress/analysis (선택, 기본: research)".into(),
                        ),
                        required: Some(false),
                    },
                ]),
                icons: None,
                meta: None,
            },
            Prompt {
                name: "convert_and_review".into(),
                title: Some("문서 편집 워크플로우".into()),
                description: Some(
                    "기존 HWPX 문서를 JSON round-trip으로 편집하는 단계별 가이드".into(),
                ),
                arguments: Some(vec![
                    PromptArgument {
                        name: "file_path".into(),
                        title: Some("HWPX 파일 경로".into()),
                        description: Some("편집할 HWPX 파일의 경로".into()),
                        required: Some(true),
                    },
                    PromptArgument {
                        name: "edit_instructions".into(),
                        title: Some("편집 지침".into()),
                        description: Some("구체적인 편집 지침 (선택)".into()),
                        required: Some(false),
                    },
                ]),
                icons: None,
                meta: None,
            },
        ],
        next_cursor: None,
        meta: None,
    })
}

/// Get a specific prompt by name with arguments.
pub fn get_prompt(name: &str, arguments: Option<&JsonObject>) -> Result<GetPromptResult, McpError> {
    let empty = serde_json::Map::new();
    let args = arguments.unwrap_or(&empty);
    match name {
        "generate_proposal" => proposal::get_prompt(args),
        "generate_report" => report::get_prompt(args),
        "convert_and_review" => convert_review::get_prompt(args),
        _ => Err(McpError::invalid_params(
            format!(
                "Unknown prompt: {name}. Available: generate_proposal, generate_report, convert_and_review"
            ),
            None,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_prompts_returns_three() {
        let result = list_prompts().unwrap();
        assert_eq!(result.prompts.len(), 3);
    }

    #[test]
    fn list_prompts_has_correct_names() {
        let result = list_prompts().unwrap();
        let names: Vec<&str> = result.prompts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["generate_proposal", "generate_report", "convert_and_review"]);
    }

    #[test]
    fn get_prompt_proposal_with_topic() {
        let mut args = serde_json::Map::new();
        args.insert("topic".into(), serde_json::Value::String("AI 시스템".into()));
        let result = get_prompt("generate_proposal", Some(&args)).unwrap();
        assert!(!result.messages.is_empty());
        assert!(result.description.is_some());
    }

    #[test]
    fn get_prompt_proposal_missing_topic() {
        let args = serde_json::Map::new();
        let err = get_prompt("generate_proposal", Some(&args)).unwrap_err();
        assert!(err.message.contains("topic"));
    }

    #[test]
    fn get_prompt_report_with_topic() {
        let mut args = serde_json::Map::new();
        args.insert("topic".into(), serde_json::Value::String("분석 보고서".into()));
        let result = get_prompt("generate_report", Some(&args)).unwrap();
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn get_prompt_convert_review_with_file() {
        let mut args = serde_json::Map::new();
        args.insert("file_path".into(), serde_json::Value::String("/tmp/doc.hwpx".into()));
        let result = get_prompt("convert_and_review", Some(&args)).unwrap();
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn get_prompt_convert_review_missing_file() {
        let args = serde_json::Map::new();
        let err = get_prompt("convert_and_review", Some(&args)).unwrap_err();
        assert!(err.message.contains("file_path"));
    }

    #[test]
    fn get_prompt_unknown() {
        let err = get_prompt("unknown", None).unwrap_err();
        assert!(err.message.contains("Unknown prompt"));
    }

    #[test]
    fn get_prompt_proposal_with_args() {
        let mut args = serde_json::Map::new();
        args.insert("topic".into(), serde_json::Value::String("test".into()));
        let result = get_prompt("generate_proposal", Some(&args)).unwrap();
        assert!(!result.messages.is_empty());
    }
}
