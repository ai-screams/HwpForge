# 커밋·훅·테스트 실행 함정 (Tooling Gotchas)

> 이 파일은 CLAUDE.md 로딩 맵에서 필요 시 로드된다 (자동 로드 아님).

- **dprint + 한글(CJK) 마크다운 표**: 한글이 든 `.md` 표(예: `HWP5_WIRE_SPEC.md`, `CHANGELOG.md`)를 편집하면 dprint pre-commit 훅이 거부함(CJK 글자 폭 재계산으로 표 정렬 불일치 판단). `dprint fmt <파일>` 수동 실행 → 재-stage → 재커밋.
- **`cargo nextest run -p <crate> <filter>`** 의 필터는 정규식이 아니라 **부분일치(substring)** — `'a|b'` 는 아무것도 안 잡음. 공통 substring 하나(예: `warns`)로 필터하거나 따로 실행.
- 용량 큰 이미지 임베드 fixture(~MB)는 리뷰 산출물 영역 `examples/hwp5_review/`(미추적 — `git add -A` 주의. **디렉터리 통째 gitignore 금지**: tracked 리뷰 샘플 39개가 있어 cargo/release-plz 가 "committed and in .gitignore" 로 실패, PR #92)에만 두고, 회귀 방지는 **단위 테스트로 잠금**(수 MB fixture를 커밋하지 말 것).
- **pre-commit `cargo fmt` 훅**: 스테이지된 Rust(특히 테스트의 다줄 배열/`assert!`)를 재포맷하며 커밋을 **거부**함 → `cargo fmt` 수동 실행 → 재-`git add` → 재커밋 (dprint 표와 동일 패턴).
- **pre-commit/pre-push 훅이 workspace clippy(+`make ci`)를 돌림** → 다파일 커밋·push는 **2분+**, cold/contended 빌드 땐 **20분+** 까지 감. 항상 `run_in_background`로 commit/push 후 폴링.
- **docs-only 커밋/push 는 빠름**: pre-commit clippy·fmt 는 staged 에 Rust 파일이 있을 때만 실행(`no files to check` skip) — "훅 2분+" 은 Rust 변경 시에만 해당 (백그라운드 실행 원칙은 동일).
- **`git push` 를 파이프에 연결 금지**(`| tail` 등) — pre-push 훅의 대량 테스트 출력이 `BlockingIOError [Errno 35]` 로 push 자체를 죽임. run_in_background(파일 리다이렉트)로 실행하고, 성공 판정은 `git ls-remote --heads origin <branch> | grep -q .` 로.
- pre-push `cargo deny` 가 RustSec advisory DB fetch 네트워크 오류로 간헐 실패 → 재시도로 해결.
- **전체 `cargo nextest run --workspace`는 cold 빌드 시 15분+** (foreground 한계 초과) → 변경 영향 크레이트만 `-p <crate>`로 돌리고 byte-중립 게이트만 골라 검증. **테스트 실행 중 소스 편집 금지**(rebuild 유발로 더 느려짐).
- **nextest 통합 테스트 파일 필터**: substring 은 테스트 _이름_ 만 매칭 (파일명 안 잡힘) — 파일 단위는 `-E 'binary(<파일명>)'`.
- **commit 출력도 `| tail` 로 자르지 말 것** — 실패한 훅 라인·exit code 가 사라져 "커밋됐다" 오판. 파일 리다이렉트 후 grep (push 파이프 금지 규칙과 동일 계열).
- **커밋 전 touched 크레이트만 `cargo clippy --all-targets -- -D warnings` 사전 점검** — 훅 거부 1회 = 2분+ 재사이클 (nextest/build 는 clippy lint 를 안 잡음).
- `rm` 은 대화형 alias — stale `.git/index.lock`(0바이트·git 프로세스 없음 확인 후) 등 스크립트 삭제는 `rm -f`.
- 대용량 정리: `target/`(수백 GB 가능)·`fuzz/target`·`.docs/papers/EAAI/eval/oracle-rs/target` 은 재생성 가능 빌드 산출물. `.docs/papers`(corpus·논문)·`fuzz/corpus` 는 자산 — 삭제 금지. **디스크 고갈 시 우선 삭제 = `target/debug/incremental`(94GB 실사고)·`target/llvm-cov-target`** — `target/debug/deps`(warm 의존성 캐시)는 보존해 cold 재빌드를 피한다.
- pre-commit 은 **미스테이지 변경을 stash 하고 staged 트리만 검사** — 다파일 수정에서 하나라도 `git add` 누락하면 staged 트리가 컴파일 실패로 거부됨 (원인이 "숨은 미스테이지 파일"이라 오진하기 쉬움). 커밋 전 `git status --short` 로 관련 파일 전부 staged(`M`) 확인.
- **`git add -u` 는 신규 파일을 안 잡는다** — 신규 모듈 포함 커밋에서 스테이징 후 `git status --short` 의 `??` 잔존 확인 필수. **훅 clippy 는 디스크 파일을 봐서 이 누락을 못 잡음** → "훅 통과 + clean-checkout 컴파일 불가 커밋" 사고 (placement.rs 실사고, reset --mixed 재구성으로 복구).
- **stale `.git/index.lock` 은 반복 사고** — 백그라운드 에이전트/세션이 강제 종료될 때 0바이트 lock 잔존. `ls -la .git/index.lock`(0바이트) + git 프로세스 없음 확인 후 `rm -f`.
- CLI **`convert` 는 Markdown→HWPX 전용** — HWP5 변환은 **`convert-hwp5`**. `convert` 에 .hwp 를 넣으면 "stream did not contain valid UTF-8" (텍스트로 읽음).
- **백그라운드(run_in_background) Bash 는 cwd 가 포그라운드와 다를 수 있음** — 백그라운드 스크립트의 경로는 전부 절대 경로로 (상대 경로 FILE_READ_FAILED 오진 사고).
- 시각 게이트 PDF 대조에서 "달라 보인다" 보고가 오면 먼저 **동일 배율 PNG**(`sips -s format png -Z <px>`)로 재판정 — 뷰어 확대율/스크롤 차이가 흔한 오탐. 정량 판정은 CTM 파서 bbox Δ(≤0.1pt 게이트)가 정본.
- 백그라운드 executor 가 API 단절로 죽으면: **작업 트리 diff 부터 확인** — 변경 0 이면 전체 스펙으로 재시작, 부분 진행이면 SendMessage 로 재개(transcript 컨텍스트 보존). 죽을 때 stale `.git/index.lock` 을 남기는 일이 잦다.
- **zsh 는 미인용 변수를 word-split 하지 않음** — `CMD="node /x.mjs"; $CMD status` 는 전체가 하나의 명령명 (조용한 command-not-found → 루프/조건 오탐). 스크립트에서 명령을 변수에 담지 말고 인라인 전체 경로로 (`for x in $VAR` 미분리와 동계열).
- Bash 작업 디렉터리는 **호출 간 지속** — 앞서 `cd` 한 상태에서 레포-루트 상대 경로(git add 등)를 쓰면 pathspec fatal. 커밋/스테이지 명령은 절대 경로 또는 루트 복귀 후 실행.

## Watch Mode

```bash
bacon         # Auto-run clippy on file changes
bacon test    # Auto-run tests
```

## Documentation & Coverage

```bash
make doc
make cov
```

---
