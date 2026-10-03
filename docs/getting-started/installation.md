# 설치

HwpForge는 순수 Rust로 작성된 라이브러리입니다. 별도의 시스템 의존성 없이 `Cargo.toml`에 추가하는 것만으로 사용할 수 있습니다.

## 최소 지원 Rust 버전 (MSRV)

라이브러리 크레이트(`hwpforge` umbrella와 하위 크레이트 대부분)는 **Rust 1.89 이상이** 필요합니다. 현재 버전을 확인하려면:

```bash
rustc --version
```

버전이 낮다면 `rustup`으로 업데이트합니다:

```bash
rustup update stable
```

PDF 렌더(`krilla`)에 의존하는 `hwpforge-smithy-pdf`, `hwpforge-convert`, `hwpforge-bindings-cli`, `hwpforge-bindings-py`는 각 `Cargo.toml`에 `rust-version = "1.92"`가 지정돼 있어 **Rust 1.92 이상이** 필요합니다. `hwpforge` umbrella만 쓰는 라이브러리 사용자는 1.89면 충분하고, CLI를 소스에서 설치하거나 Python sdist를 빌드할 때만 1.92가 필요합니다.

## 의존성 추가

`Cargo.toml`의 `[dependencies]` 섹션에 추가합니다:

```toml
[dependencies]
hwpforge = "0.16"
```

기본 설치에는 HWPX 인코더/디코더가 포함됩니다.

## Feature Flags

HwpForge는 필요한 기능만 선택적으로 활성화할 수 있습니다.

| Feature    | 기본 포함 | 설명                                                                                                    |
| ---------- | --------- | ------------------------------------------------------------------------------------------------------- |
| `hwpx`     | 예        | HWPX 인코더/디코더 (ZIP + XML, KS X 6101)                                                               |
| `md`       | 아니오    | Markdown(GFM) ↔ HWPX 변환                                                                               |
| `ops-hwpx` | 아니오    | HWPX 전용 표면(검사·교환·읽기·편집·diff·stamp·스타일)이 공유하는 연산 계층. `hwpx`를 함께 켭니다        |
| `ops-md`   | 아니오    | 연산 계층에 Markdown 연산(`convert_md`, `to_md`)을 더합니다. `ops-hwpx`와 `md`를 함께 켭니다            |
| `ops`      | 아니오    | 연산 계층 전체의 별칭(`ops-md`)                                                                         |
| `schemars` | 아니오    | `ops`가 소유한 wire DTO와 교환 DTO에 `JsonSchema` derive를 더합니다. HWPX 코덱을 따라 켜지지는 않습니다 |
| `full`     | 아니오    | `hwpx` + `md` + `ops`를 켭니다(`schemars`는 포함하지 않음)                                              |

### HWPX만 사용 (기본)

```toml
[dependencies]
hwpforge = "0.16"
```

### Markdown 변환 포함

```toml
[dependencies]
hwpforge = { version = "0.16", features = ["md"] }
```

### 편집 연산 계층 포함

CLI·MCP·Python이 공유하는 연산(`hwpforge::ops`)을 라이브러리에서 직접 쓰려면 `ops`를 켭니다.

```toml
[dependencies]
hwpforge = { version = "0.16", features = ["ops"] }
```

### 모든 기능 활성화

```toml
[dependencies]
hwpforge = { version = "0.16", features = ["full"] }
```

## 빌드 확인

의존성을 추가한 후 빌드가 정상적으로 되는지 확인합니다:

```bash
cargo build
```

다음과 같이 컴파일이 성공하면 설치가 완료된 것입니다:

```
Compiling hwpforge v0.16.9
 Finished `dev` profile [unoptimized + debuginfo] target(s) in ...
```

## CLI · MCP · Python

Rust 라이브러리 외에 다른 창구도 있습니다. 설치 방법은 각 장에 있습니다.

- **CLI** (`hwpforge`): crates.io에 배포하지 않으므로 git 또는 로컬 경로에서 설치합니다. Rust 1.92 이상이 필요합니다. [CLI 레퍼런스](../reference/cli.md)
- **MCP 서버** (`hwpforge-mcp`): `npx -y @hwpforge/mcp` 또는 `cargo install hwpforge-bindings-mcp`. [MCP 서버 레퍼런스](../reference/mcp.md)
- **Python**: `pip install hwpforge`. [Python 가이드](../guide/python.md)

## 다음 단계

설치가 완료되었습니다. [빠른 시작](./quickstart.md)으로 이동하여 첫 번째 HWPX 문서를 생성해 보세요.
