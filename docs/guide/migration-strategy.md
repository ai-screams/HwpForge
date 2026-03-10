# 대규모 HWP 아카이브 마이그레이션

레거시 HWP/HWPX 문서 아카이브를 검색 가능한 Markdown으로 마이그레이션하는 전략을 설명합니다.

## 마이그레이션 파이프라인 개요

```text
┌────────────────┐     ┌──────────────┐     ┌─────────────┐     ┌──────────────┐
│ 1. 스캔        │ ──▶ │ 2. 분류      │ ──▶ │ 3. 변환     │ ──▶ │ 4. 검증      │
│ 파일 목록 수집 │     │ 포맷 감지    │     │ Core → MD   │     │ 무결성 확인  │
│                │     │ HWP5/HWPX    │     │ lossy 모드  │     │ 결과 기록    │
└────────────────┘     └──────────────┘     └─────────────┘     └──────────────┘
```

## 1단계: 파일 스캔 및 포맷 분류

파일 확장자와 매직 바이트로 포맷을 감지합니다.

```rust,no_run
use std::path::{Path, PathBuf};

#[derive(Debug)]
enum DocFormat {
    Hwpx,         // ZIP + XML (PK 시그니처)
    Hwp5,         // OLE2/CFB (D0 CF 시그니처)
    Unknown(String),
}

#[derive(Debug)]
struct ScanResult {
    path: PathBuf,
    format: DocFormat,
    size_bytes: u64,
}

fn scan_archive(dir: &Path) -> Vec<ScanResult> {
    let mut results = Vec::new();

    let walker = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok());

    for entry in walker {
        let path = entry.path();
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext != "hwp" && ext != "hwpx" {
            continue;
        }

        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let format = detect_format(path);

        results.push(ScanResult {
            path: path.to_path_buf(),
            format,
            size_bytes,
        });
    }

    results
}

fn detect_format(path: &Path) -> DocFormat {
    let Ok(bytes) = std::fs::read(path) else {
        return DocFormat::Unknown("읽기 실패".into());
    };

    if bytes.len() < 4 {
        return DocFormat::Unknown("파일이 너무 작음".into());
    }

    // ZIP (HWPX): PK\x03\x04
    if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return DocFormat::Hwpx;
    }

    // OLE2/CFB (HWP5): D0 CF 11 E0
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return DocFormat::Hwp5;
    }

    DocFormat::Unknown(format!("알 수 없는 시그니처: {:02X} {:02X}", bytes[0], bytes[1]))
}
```

## 2단계: 변환 (HWPX → Markdown)

현재 HwpForge는 HWPX 파일의 변환을 지원합니다. HWP5 파일은 사전 변환이 필요합니다.

### HWPX 파일 변환

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::MdEncoder;
use std::path::Path;

#[derive(Debug)]
struct ConvertResult {
    source: String,
    status: ConvertStatus,
    markdown_len: usize,
    title: Option<String>,
}

#[derive(Debug)]
enum ConvertStatus {
    Success,
    DecodeError(String),
    ValidationError(String),
    EncodeError(String),
}

fn convert_hwpx(input: &Path) -> ConvertResult {
    let source = input.display().to_string();

    // 1. 디코딩 (손상 파일 처리)
    let result = match HwpxDecoder::decode_file(input) {
        Ok(r) => r,
        Err(e) => {
            return ConvertResult {
                source,
                status: ConvertStatus::DecodeError(e.to_string()),
                markdown_len: 0,
                title: None,
            };
        }
    };

    let title = result.document.metadata().title.clone();

    // 2. 검증
    let validated = match result.document.validate() {
        Ok(v) => v,
        Err(e) => {
            return ConvertResult {
                source,
                status: ConvertStatus::ValidationError(e.to_string()),
                markdown_len: 0,
                title,
            };
        }
    };

    // 3. Markdown 변환 (RAG/검색용 lossy 모드)
    match MdEncoder::encode_lossy(&validated) {
        Ok(md) => ConvertResult {
            source,
            status: ConvertStatus::Success,
            markdown_len: md.len(),
            title,
        },
        Err(e) => ConvertResult {
            source,
            status: ConvertStatus::EncodeError(e.to_string()),
            markdown_len: 0,
            title,
        },
    }
}
```

### HWP5 파일 사전 처리

레거시 HWP5(`.hwp`) 파일은 현재 직접 변환이 불가능합니다. 다음 전략을 사용합니다:

| 전략               | 설명                                                         | 자동화    |
| ------------------ | ------------------------------------------------------------ | --------- |
| **한글 배치 변환** | 한글 프로그램의 매크로/스크립트로 `.hwp` → `.hwpx` 일괄 변환 | 반자동    |
| **한컴독스 API**   | 한컴독스 클라우드 API로 변환 (유료)                          | 완전 자동 |
| **별도 분류**      | HWP5 파일만 분리하여 v2.0 지원 후 처리                       | 수동      |

```rust,no_run,ignore
// v2.0 이후 — HWP5 직접 변환
// use hwpforge::hwp5::Hwp5Decoder;
//
// let result = Hwp5Decoder::decode_file("legacy.hwp")?;
// let validated = result.document.validate()?;
// let markdown = MdEncoder::encode_lossy(&validated)?;
```

## 3단계: 배치 처리 아키텍처

대규모 아카이브(수천~수만 파일)를 안정적으로 처리하는 패턴입니다.

```rust,no_run
use std::path::{Path, PathBuf};
use std::fs;

use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::MdEncoder;

struct MigrationConfig {
    input_dir: PathBuf,
    output_dir: PathBuf,
    error_dir: PathBuf,
    /// 개별 파일 처리 제한 시간 (초)
    timeout_secs: u64,
    /// 최대 파일 크기 (바이트, 기본 100MB)
    max_file_size: u64,
}

struct MigrationReport {
    total: usize,
    success: usize,
    failed: usize,
    skipped_hwp5: usize,
    skipped_too_large: usize,
    errors: Vec<(String, String)>,
}

fn run_migration(config: &MigrationConfig) -> MigrationReport {
    fs::create_dir_all(&config.output_dir).expect("출력 디렉토리 생성 실패");
    fs::create_dir_all(&config.error_dir).expect("오류 디렉토리 생성 실패");

    let mut report = MigrationReport {
        total: 0, success: 0, failed: 0,
        skipped_hwp5: 0, skipped_too_large: 0,
        errors: Vec::new(),
    };

    let files: Vec<_> = walkdir::WalkDir::new(&config.input_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension()
                .is_some_and(|ext| ext == "hwpx" || ext == "hwp")
        })
        .collect();

    report.total = files.len();
    eprintln!("총 {} 파일 발견", report.total);

    for (i, entry) in files.iter().enumerate() {
        let path = entry.path();
        let rel_path = path.strip_prefix(&config.input_dir).unwrap_or(path);

        // 진행률 표시
        if (i + 1) % 100 == 0 || i + 1 == report.total {
            eprintln!("[{}/{}] 처리 중...", i + 1, report.total);
        }

        // HWP5 건너뛰기
        if path.extension().is_some_and(|ext| ext == "hwp") {
            report.skipped_hwp5 += 1;
            continue;
        }

        // 파일 크기 제한
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size > config.max_file_size {
            report.skipped_too_large += 1;
            continue;
        }

        // 변환 시도
        match convert_single(path, &config.output_dir, rel_path) {
            Ok(_) => report.success += 1,
            Err(e) => {
                report.failed += 1;
                report.errors.push((path.display().to_string(), e.clone()));

                // 실패 파일을 오류 디렉토리에 복사
                let err_dest = config.error_dir.join(rel_path);
                if let Some(parent) = err_dest.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::copy(path, err_dest);
            }
        }
    }

    report
}

fn convert_single(
    input: &Path,
    output_dir: &Path,
    rel_path: &Path,
) -> Result<(), String> {
    let result = HwpxDecoder::decode_file(input)
        .map_err(|e| format!("디코딩 실패: {e}"))?;

    let validated = result.document.validate()
        .map_err(|e| format!("검증 실패: {e}"))?;

    let markdown = MdEncoder::encode_lossy(&validated)
        .map_err(|e| format!("MD 인코딩 실패: {e}"))?;

    // 출력 경로: .hwpx → .md
    let out_name = rel_path.with_extension("md");
    let out_path = output_dir.join(out_name);

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("디렉토리 생성 실패: {e}"))?;
    }

    fs::write(&out_path, &markdown).map_err(|e| format!("파일 쓰기 실패: {e}"))?;

    Ok(())
}
```

## 4단계: 무결성 검증

변환 결과의 품질을 검증합니다.

### 기본 검증 항목

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::MdEncoder;
use std::path::Path;

struct IntegrityCheck {
    source_sections: usize,
    source_paragraphs: usize,
    source_tables: usize,
    markdown_lines: usize,
    markdown_bytes: usize,
    has_title: bool,
}

fn verify_conversion(hwpx_path: &Path, markdown: &str) -> Option<IntegrityCheck> {
    let result = HwpxDecoder::decode_file(hwpx_path).ok()?;
    let doc = &result.document;

    let mut total_paragraphs = 0;
    let mut total_tables = 0;
    for section in doc.sections() {
        total_paragraphs += section.paragraphs.len();
        total_tables += section.content_counts().tables;
    }

    Some(IntegrityCheck {
        source_sections: doc.sections().len(),
        source_paragraphs: total_paragraphs,
        source_tables: total_tables,
        markdown_lines: markdown.lines().count(),
        markdown_bytes: markdown.len(),
        has_title: doc.metadata().title.is_some(),
    })
}
```

### 검증 체크리스트

| 항목            | 방법                                         | 허용 기준      |
| --------------- | -------------------------------------------- | -------------- |
| **텍스트 보존** | 원본 문단 수 vs Markdown 비어있지 않은 줄 수 | 손실 < 10%     |
| **표 구조**     | 원본 표 수 vs Markdown `\|` 테이블 수        | 동일           |
| **메타데이터**  | YAML frontmatter에 title/author 존재         | 원본과 일치    |
| **파일 크기**   | Markdown 바이트 > 0                          | 빈 파일 없음   |
| **인코딩**      | UTF-8 유효성                                 | 깨진 문자 없음 |

## 손상 파일 처리 전략

대규모 아카이브에서는 손상되거나 비표준 파일이 불가피합니다.

### 일반적인 오류 유형과 대응

| 오류              | 원인                               | 대응                           |
| ----------------- | ---------------------------------- | ------------------------------ |
| ZIP 파싱 실패     | 파일 손상, 불완전 다운로드         | 오류 목록에 기록, 원본 보존    |
| XML 파싱 실패     | 비표준 네임스페이스, 잘못된 인코딩 | 오류 목록에 기록, 수동 검토    |
| 검증 실패         | 빈 섹션, 유효하지 않은 인덱스      | 경고 후 계속 진행              |
| OOM (메모리 부족) | 매우 큰 임베디드 이미지            | 파일 크기 제한으로 사전 필터링 |
| 암호화된 파일     | 비밀번호 보호                      | 별도 목록으로 분류             |

### 에러 리포트 생성

```rust,no_run
use std::fs;

struct MigrationReport {
    total: usize,
    success: usize,
    failed: usize,
    skipped_hwp5: usize,
    skipped_too_large: usize,
    errors: Vec<(String, String)>,
}

fn write_report(report: &MigrationReport, path: &str) {
    let mut lines = Vec::new();
    lines.push(format!("# 마이그레이션 리포트\n"));
    lines.push(format!("- 총 파일: {}", report.total));
    lines.push(format!("- 성공: {}", report.success));
    lines.push(format!("- 실패: {}", report.failed));
    lines.push(format!("- HWP5 건너뜀: {}", report.skipped_hwp5));
    lines.push(format!("- 크기 초과: {}", report.skipped_too_large));

    if !report.errors.is_empty() {
        lines.push(format!("\n## 실패 목록\n"));
        lines.push(format!("| 파일 | 오류 |"));
        lines.push(format!("| --- | --- |"));
        for (file, err) in &report.errors {
            lines.push(format!("| `{}` | {} |", file, err));
        }
    }

    fs::write(path, lines.join("\n")).expect("리포트 저장 실패");
}
```

## Lossy vs Lossless 모드 선택

| 목적                | 권장 모드         | 이유                                |
| ------------------- | ----------------- | ----------------------------------- |
| **RAG/검색 인덱싱** | `encode_lossy`    | 표준 GFM, 청크 분할 호환, 토큰 절약 |
| **아카이브 백업**   | `encode_lossless` | 구조 완전 보존, 원본 복원 가능      |
| **하이브리드**      | 둘 다 생성        | lossy는 검색용, lossless는 백업용   |

하이브리드 전략이 이상적입니다:

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::MdEncoder;

let result = HwpxDecoder::decode_file("document.hwpx").unwrap();
let validated = result.document.validate().unwrap();

// 검색/RAG용
let lossy = MdEncoder::encode_lossy(&validated).unwrap();
std::fs::write("output/search/document.md", &lossy).unwrap();

// 아카이브 백업용
let lossless = MdEncoder::encode_lossless(&validated).unwrap();
std::fs::write("output/archive/document.lossless.md", &lossless).unwrap();
```

## 관련 문서

- [HWPX 인코딩/디코딩](./hwpx-codec.md) — 기본 HWPX 처리
- [Markdown에서 HWPX로](./markdown-bridge.md) — Markdown 변환 상세 (RAG 가이드 포함)
- [텍스트 추출](./text-extraction.md) — 구조 보존 텍스트 추출
- [HWP5와 HWPX: 이중 포맷 파이프라인](./format-pipeline.md) — 포맷 비교 및 감지
