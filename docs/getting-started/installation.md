# 설치

HwpForge는 순수 Rust로 작성된 라이브러리입니다. 별도의 시스템 의존성 없이 `Cargo.toml`에 추가하는 것만으로 사용할 수 있습니다.

## 최소 지원 Rust 버전 (MSRV)

**Rust 1.88 이상**이 필요합니다. 현재 버전을 확인하려면:

```bash
rustc --version
```

버전이 낮다면 `rustup`으로 업데이트합니다:

```bash
rustup update stable
```

## 의존성 추가

`Cargo.toml`의 `[dependencies]` 섹션에 추가합니다:

```toml
[dependencies]
hwpforge = "0.1"
```

기본 설치에는 HWPX 인코더/디코더가 포함됩니다.

## Feature Flags

HwpForge는 필요한 기능만 선택적으로 활성화할 수 있습니다.

| Feature | 기본 포함 | 설명                                      |
| ------- | --------- | ----------------------------------------- |
| `hwpx`  | 예        | HWPX 인코더/디코더 (ZIP + XML, KS X 6101) |
| `md`    | 아니오    | Markdown(GFM) ↔ HWPX 변환                 |
| `full`  | 아니오    | 모든 기능 활성화                          |

### HWPX만 사용 (기본)

```toml
[dependencies]
hwpforge = "0.1"
```

### Markdown 변환 포함

```toml
[dependencies]
hwpforge = { version = "0.1", features = ["md"] }
```

### 모든 기능 활성화

```toml
[dependencies]
hwpforge = { version = "0.1", features = ["full"] }
```

## 빌드 확인

의존성을 추가한 후 빌드가 정상적으로 되는지 확인합니다:

```bash
cargo build
```

다음과 같이 컴파일이 성공하면 설치가 완료된 것입니다:

```
Compiling hwpforge v0.1.0
 Finished `dev` profile [unoptimized + debuginfo] target(s) in ...
```

## 다음 단계

설치가 완료되었습니다. [빠른 시작](./quickstart.md)으로 이동하여 첫 번째 HWPX 문서를 생성해 보세요.
