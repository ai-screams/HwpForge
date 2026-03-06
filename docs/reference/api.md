# API 레퍼런스

HwpForge의 전체 공개 API 문서는 rustdoc으로 자동 생성됩니다.

**[API 레퍼런스 열기 (rustdoc) →](../api/hwpforge/index.html)**

## 크레이트 구조

| 크레이트               | 역할                          | rustdoc                                                        |
| ---------------------- | ----------------------------- | -------------------------------------------------------------- |
| `hwpforge`             | 우산 크레이트 (re-export)     | [hwpforge](../api/hwpforge/index.html)                         |
| `hwpforge-foundation`  | 기본 타입 (HwpUnit, Color)    | [hwpforge_foundation](../api/hwpforge_foundation/index.html)   |
| `hwpforge-core`        | 문서 구조 (Document, Section) | [hwpforge_core](../api/hwpforge_core/index.html)               |
| `hwpforge-blueprint`   | 스타일 템플릿 (YAML)          | [hwpforge_blueprint](../api/hwpforge_blueprint/index.html)     |
| `hwpforge-smithy-hwpx` | HWPX 인코더/디코더            | [hwpforge_smithy_hwpx](../api/hwpforge_smithy_hwpx/index.html) |
| `hwpforge-smithy-md`   | Markdown 인코더/디코더        | [hwpforge_smithy_md](../api/hwpforge_smithy_md/index.html)     |

## 로컬에서 보기

```bash
cargo doc --open --no-deps --all-features
```

## crates.io 퍼블리시 후

퍼블리시 이후에는 [docs.rs/hwpforge](https://docs.rs/hwpforge)에서도 확인할 수 있습니다.
