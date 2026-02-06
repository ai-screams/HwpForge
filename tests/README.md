# Tests

HwpForge 테스트 구조

## Structure

```
tests/
├── unit/           # 단위 테스트 (각 크레이트)
├── integration/    # 통합 테스트 (크레이트 간)
└── golden/         # Golden 테스트 (실제 파일)
    ├── hwpx/       # 한글 생성 HWPX 파일
    ├── hwp5/       # 한글 생성 HWP5 파일
    └── expected/   # 예상 결과
```

## Running

```bash
# 전체
cargo nextest run

# 단위만
cargo nextest run --lib

# Golden만
cargo nextest run --test golden
```
