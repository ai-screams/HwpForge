//! MCP Resource definitions for HwpForge.
//!
//! Exposes style template descriptions as readable resources so AI agents
//! can understand available styles before generating documents.

use rmcp::model::*;
use rmcp::ErrorData as McpError;

/// Template resource metadata.
struct TemplateResource {
    uri: &'static str,
    name: &'static str,
    description: &'static str,
}

const TEMPLATES: &[TemplateResource] = &[
    TemplateResource {
        uri: "hwpforge://templates/default",
        name: "Default Template",
        description: "기본 스타일 (함초롬돋움)",
    },
    TemplateResource {
        uri: "hwpforge://templates/modern",
        name: "Modern Template",
        description: "깔끔한 현대적 스타일 (맑은 고딕)",
    },
    TemplateResource {
        uri: "hwpforge://templates/classic",
        name: "Classic Template",
        description: "전통적 문서 스타일 (바탕)",
    },
    TemplateResource {
        uri: "hwpforge://templates/latest",
        name: "Latest Template",
        description: "최신 한컴 스타일 (함초롬바탕)",
    },
];

const DEFAULT_YAML: &str = "\
# HwpForge Default Template
# 기본 문서 스타일

name: default
description: \"HwpForge 기본 스타일 — 함초롬돋움 기반\"

fonts:
  primary: \"함초롬돋움\"
  fallback: \"함초롬돋움\"
  # 현재 프리셋은 글꼴을 변경합니다.
  # 페이지 여백/글자 크기 커스터마이징은 향후 지원 예정입니다.

best_for:
  - \"일반 문서\"
  - \"기본 변환\"

usage: |
  hwpforge_convert({ markdown: \"content.md\", output_path: \"out.hwpx\", preset: \"default\" })
";

const MODERN_YAML: &str = "\
# HwpForge Modern Template
# 깔끔한 현대적 문서 스타일

name: modern
description: \"깔끔한 현대적 스타일 — 고딕체 기반\"

fonts:
  primary: \"맑은 고딕\"
  fallback: \"함초롬돋움\"
  # 현재 프리셋은 글꼴을 변경합니다.
  # 페이지 여백/글자 크기 커스터마이징은 향후 지원 예정입니다.

best_for:
  - \"프레젠테이션 보조 자료\"
  - \"기술 문서\"
  - \"현대적 보고서\"
  - \"스타트업/IT 기업 제안서\"

usage: |
  hwpforge_convert({ markdown: \"content.md\", output_path: \"out.hwpx\", preset: \"modern\" })
  hwpforge_restyle({ file_path: \"doc.hwpx\", preset: \"modern\", output_path: \"doc_modern.hwpx\" })
";

const CLASSIC_YAML: &str = "\
# HwpForge Classic Template
# 전통적 문서 스타일

name: classic
description: \"전통적 문서 스타일 — 명조체 기반\"

fonts:
  primary: \"바탕\"
  fallback: \"함초롬바탕\"
  # 현재 프리셋은 글꼴을 변경합니다.
  # 페이지 여백/글자 크기 커스터마이징은 향후 지원 예정입니다.

best_for:
  - \"공문서\"
  - \"학술 논문\"
  - \"전통적 보고서\"
  - \"정부 제안서\"

usage: |
  hwpforge_convert({ markdown: \"content.md\", output_path: \"out.hwpx\", preset: \"classic\" })
  hwpforge_restyle({ file_path: \"doc.hwpx\", preset: \"classic\", output_path: \"doc_classic.hwpx\" })
";

const LATEST_YAML: &str = "\
# HwpForge Latest Template
# 최신 한컴 스타일

name: latest
description: \"최신 한컴오피스 스타일 — 함초롬바탕 기반\"

fonts:
  primary: \"함초롬바탕\"
  fallback: \"함초롬돋움\"
  # 현재 프리셋은 글꼴을 변경합니다.
  # 페이지 여백/글자 크기 커스터마이징은 향후 지원 예정입니다.

best_for:
  - \"한글(한컴오피스) 기본 문서\"
  - \"일반 보고서\"
  - \"사내 문서\"

usage: |
  hwpforge_convert({ markdown: \"content.md\", output_path: \"out.hwpx\", preset: \"latest\" })
  hwpforge_restyle({ file_path: \"doc.hwpx\", preset: \"latest\", output_path: \"doc_latest.hwpx\" })
";

/// List all available template resources.
pub fn list_resources() -> Result<ListResourcesResult, McpError> {
    let resources = TEMPLATES
        .iter()
        .map(|t| {
            RawResource {
                uri: t.uri.into(),
                name: t.name.into(),
                description: Some(t.description.into()),
                mime_type: Some("application/x-yaml".into()),
                title: None,
                size: None,
                icons: None,
                meta: None,
            }
            .no_annotation()
        })
        .collect();

    Ok(ListResourcesResult { resources, next_cursor: None, meta: None })
}

/// Read a specific template resource by URI.
pub fn read_resource(uri: &str) -> Result<ReadResourceResult, McpError> {
    let content = match uri {
        "hwpforge://templates/default" => DEFAULT_YAML,
        "hwpforge://templates/modern" => MODERN_YAML,
        "hwpforge://templates/classic" => CLASSIC_YAML,
        "hwpforge://templates/latest" => LATEST_YAML,
        _ => return Err(McpError::resource_not_found(format!("Unknown resource: {uri}"), None)),
    };

    Ok(ReadResourceResult { contents: vec![ResourceContents::text(content, uri)] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_resources_returns_four() {
        let result = list_resources().unwrap();
        assert_eq!(result.resources.len(), 4);
    }

    #[test]
    fn list_resources_has_correct_uris() {
        let result = list_resources().unwrap();
        let uris: Vec<&str> = result.resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"hwpforge://templates/default"));
        assert!(uris.contains(&"hwpforge://templates/modern"));
        assert!(uris.contains(&"hwpforge://templates/classic"));
        assert!(uris.contains(&"hwpforge://templates/latest"));
    }

    #[test]
    fn list_resources_has_yaml_mime_type() {
        let result = list_resources().unwrap();
        for r in &result.resources {
            assert_eq!(r.mime_type.as_deref(), Some("application/x-yaml"));
        }
    }

    #[test]
    fn read_resource_modern() {
        let result = read_resource("hwpforge://templates/modern").unwrap();
        assert_eq!(result.contents.len(), 1);
        if let ResourceContents::TextResourceContents { text, .. } = &result.contents[0] {
            assert!(text.contains("맑은 고딕"));
            assert!(text.contains("modern"));
        } else {
            panic!("Expected TextResourceContents");
        }
    }

    #[test]
    fn read_resource_classic() {
        let result = read_resource("hwpforge://templates/classic").unwrap();
        if let ResourceContents::TextResourceContents { text, .. } = &result.contents[0] {
            assert!(text.contains("바탕"));
        } else {
            panic!("Expected TextResourceContents");
        }
    }

    #[test]
    fn read_resource_latest() {
        let result = read_resource("hwpforge://templates/latest").unwrap();
        if let ResourceContents::TextResourceContents { text, .. } = &result.contents[0] {
            assert!(text.contains("함초롬바탕"));
        } else {
            panic!("Expected TextResourceContents");
        }
    }

    #[test]
    fn read_resource_default() {
        let result = read_resource("hwpforge://templates/default").unwrap();
        if let ResourceContents::TextResourceContents { text, .. } = &result.contents[0] {
            assert!(text.contains("함초롬돋움"));
            assert!(text.contains("default"));
        } else {
            panic!("Expected TextResourceContents");
        }
    }

    #[test]
    fn read_resource_unknown() {
        let err = read_resource("hwpforge://templates/unknown").unwrap_err();
        assert!(err.message.contains("Unknown resource"));
    }

    #[test]
    fn templates_match_builtin_presets() {
        let presets = hwpforge_smithy_hwpx::presets::builtin_presets();
        let resource_names: Vec<&str> = TEMPLATES
            .iter()
            .map(|t| t.uri.strip_prefix("hwpforge://templates/").unwrap())
            .collect();
        let preset_names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            resource_names, preset_names,
            "TEMPLATES URIs must match builtin_presets() names"
        );
    }
}
