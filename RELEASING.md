# Releasing HwpForge

**워크스페이스 릴리스**는 **[release-plz](https://release-plz.dev/)** 가 소유한다. 사람이 직접 버전을
올리거나 `v*` 태그를 찍거나 `cargo publish` 하지 **않는다**. 사람이 하는 일은 단 하나:
**release-plz가 만든 "Release PR"을 리뷰하고 머지**하는 것.

**예외는 하나뿐이다.** Python 전용 긴급 수정에 붙이는 `py-v*` 태그는 사람이 직접 만든다 (§9.3). 접두사가 다르고 Rust 크레이트 버전을 건드리지 않으므로 release-plz 와 충돌하지 않는다. 이 예외를 워크스페이스 `v*` 태그로 일반화하지 말 것 — `v*` 는 여전히 release-plz 만 찍는다.

> 설정 위치: `.github/workflows/release-plz.yml` (자동화) · `release-plz.toml` (정책) ·
> `.github/workflows/npm-publish.yml` (MCP npm 배포) · `.github/workflows/pages.yml` (문서 배포).

---

## 1. 전체 흐름

```mermaid
flowchart TD
    A["feature PR 머지<br/>(버전 안 올림 · conventional commit)"] --> B["push to main"]
    B --> C["release-plz.yml 실행<br/>(release-plz-release + release-plz-pr 병렬)"]
    C --> E["release-plz release-pr<br/>다음 Release PR 생성/갱신<br/>(버전 bump + CHANGELOG)"]
    E --> F["사람: Release PR 리뷰 & 머지"]
    F --> G["push to main (Release PR)"]
    G --> H["release-plz release<br/>crates.io publish + git 태그"]
    H --> I["umbrella hwpforge<br/>GitHub Release + v{version} 태그"]
    I --> J["npm-publish.yml<br/>hwpforge-mcp 5타깃 npm 배포"]
    I --> K["pages.yml<br/>mdBook 문서 배포"]
```

**두 단계로 나뉘는 게 핵심이다.** 평소 feature PR을 머지하면 release-plz가 _Release PR을
열어두기만_ 한다(아직 릴리스 아님). 그 **Release PR을 머지하는 순간**에야 실제 publish·태그·
GitHub Release가 일어난다.

---

## 2. 개발자가 할 일

1. **conventional commit** 으로 작업한다 (아래 §3). feature PR에서는 **버전을 만지지 않는다.**
2. PR을 main에 머지한다.
3. release-plz가 자동으로 **Release PR**(라벨 `release`)을 열거나 갱신한다.
   - 누적된 커밋으로 다음 버전을 계산하고 (`semver_check = true` → cargo-semver-checks로
     breaking 여부 판정), `CHANGELOG.md` 를 갱신한다.
4. 릴리스할 준비가 되면 **Release PR을 리뷰하고 머지**한다.
5. 나머지(crates.io publish, 태그, GitHub Release, npm, 문서 배포)는 전부 자동.

> ⚠️ **버전·태그를 손으로 만들지 말 것.** `0.6.0`/per-crate 태그는 모두 release-plz 산출물이다.
> 손으로 찍으면 release-plz 상태와 어긋나 자기비교·중복 publish 등 사고가 난다.

---

## 3. 커밋 규칙 (릴리스·CHANGELOG를 결정)

릴리스를 트리거하는 타입 (`release-plz.toml` `release_commits`):
`feat` · `fix` · `perf` · `refactor` (+ 임의의 `type!:` breaking).

CHANGELOG 그룹 매핑 (`commit_parsers`):

| 타입                                | CHANGELOG 섹션 | 릴리스 트리거 |
| ----------------------------------- | -------------- | ------------- |
| `feat`                              | Added          | ✅            |
| `fix`                               | Fixed          | ✅            |
| `perf`                              | Performance    | ✅            |
| `refactor`                          | Changed        | ✅            |
| `doc`                               | Documentation  | ❌ (그룹만)   |
| `style`·`test`·`chore`·`ci`·`build` | (skip)         | ❌            |

**Breaking change 표기** — 둘 중 하나로 _명시_ 해야 release-plz가 메이저급 bump를 잡는다:

- 제목에 `!`: `feat(core)!: ...`, `refactor(foundation)!: ...`
- 또는 footer: `BREAKING CHANGE: ...`

> 비표준 타입으로 breaking을 낼 때(예: `docs!:`)도 **`type!:` 형태를 쓰는 게 안전**하다
> (`release-plz.toml` 주석 참고). breaking을 안 적으면 0.x에서 patch로 잘못 bump될 수 있다.

---

## 4. 무엇이 어디로 배포되나

| 크레이트                | crates.io | git 태그       | 비고                                                          |
| ----------------------- | --------- | -------------- | ------------------------------------------------------------- |
| `hwpforge` (umbrella)   | ✅        | `v{version}`   | **유일하게 GitHub Release 생성** → npm/pages 트리거           |
| `hwpforge-foundation`   | ✅        | `…-v{version}` |                                                               |
| `hwpforge-core`         | ✅        | `…-v{version}` |                                                               |
| `hwpforge-blueprint`    | ✅        | `…-v{version}` |                                                               |
| `hwpforge-smithy-hwpx`  | ✅        | `…-v{version}` |                                                               |
| `hwpforge-smithy-md`    | ✅        | `…-v{version}` |                                                               |
| `hwpforge-bindings-mcp` | ✅        | `…-v{version}` | npm `@hwpforge/mcp` 바이너리는 npm-publish.yml가 별도 배포    |
| `hwpforge-smithy-hwp5`  | ❌        | ❌             | `release=false, publish=false`                                |
| `hwpforge-bindings-cli` | ❌        | ❌             | `release=false, publish=false`                                |
| `hwpforge-bindings-py`  | ❌        | ❌             | `release=false, publish=false` — 배포는 PyPI (§9)             |
| `hwpforge-smithy-pdf`   | ❌        | ❌             | Cargo.toml `publish = false`, `release-plz.toml` 에 항목 없음 |
| `hwpforge-convert`      | ❌        | ❌             | Cargo.toml `publish = false`, `release-plz.toml` 에 항목 없음 |

- **npm**: umbrella의 GitHub Release `published` → `npm-publish.yml` 가 `hwpforge-mcp` 5타깃
  바이너리 + 플랫폼 패키지 + base `@hwpforge/mcp`(optionalDependencies) 배포.
- **PyPI**: 같은 Release `published` → `pypi-publish.yml` 가 `hwpforge` wheel 5개 + sdist 배포
  (§9). `hwpforge-bindings-py` 크레이트 자체는 crates.io 에 올라가지 않는다 — Python 배포
  채널은 PyPI 하나뿐이다.
- **문서**: 실제 릴리스가 생겼을 때만(`releases_created == true`) `pages.yml` 가 mdBook 배포.

> 표 마지막 두 행은 `release-plz.toml` 에 항목이 없어 설정만으로는 태그 여부를 단정할 수 없었는데,
> `0.16.5`·`0.16.6` 두 릴리스의 태그 목록(`git ls-remote --tags origin | grep 'v0.16.6$'`)에
> 둘 다 없고 umbrella `v{version}` + 위 여섯 크레이트 태그만 있었다 — Cargo.toml `publish = false`
> 인 크레이트는 release-plz 가 태그도 만들지 않는다.

---

## 5. 0.x SemVer 규칙

`1.0.0` 이전에는 **마이너 자리가 메이저 역할**이다.

- breaking change → **마이너** bump (`0.6.x → 0.7.0`)
- 호환 추가/수정 → **패치** bump (`0.6.0 → 0.6.1`)

release-plz가 cargo-semver-checks로 이를 자동 판정하므로, breaking을 커밋에 제대로
표기(§3)하기만 하면 버전은 알아서 맞춰진다.

> **SemVer 검사를 ci.yml에 standalone 게이트로 다시 넣지 말 것.** release-plz가 이미
> 소유한다. feature PR은 버전을 안 올리는 모델이라, "HEAD vs 최신 태그" 게이트는 breaking
> feature PR마다 영원히 빨강이 된다 (PR #78에서 이 이유로 제거함).

---

## 6. 사전 조건 (시크릿)

| 시크릿                       | 용도                                              |
| ---------------------------- | ------------------------------------------------- |
| `APP_ID` + `APP_PRIVATE_KEY` | release-plz용 GitHub App 토큰 (PR 생성·태그 push) |
| `CARGO_REGISTRY_TOKEN`       | crates.io publish                                 |
| npm 토큰 (`npm-publish.yml`) | `@hwpforge/*` npm 배포                            |

---

## 7. 다음 릴리스 때 알아둘 것 (체크리스트)

- [ ] **버전/태그를 손대지 않는다.** Release PR 머지만 한다.
- [ ] **CHANGELOG의 한글(CJK) 표.** 편집 후 dprint pre-commit이 거부하면
      `dprint fmt CHANGELOG.md` 수동 실행 → 재-stage (CLAUDE.md Tooling Gotchas).
- [ ] **breaking은 반드시 `type!:` 로 표기.** 안 하면 0.x에서 patch로 잘못 bump.
- [ ] **로컬에서 태그 기반 검증 시 `git fetch --tags` 먼저.** 로컬 클론에 최신 태그가
      없으면 잘못된 baseline으로 거짓 통과한다 (PR #78에서 겪은 함정).
- [ ] **release 전 `make ci-full` + `make py-all` 통과 확인** (release-plz.yml 은 preflight 없이 곧바로 release job 을 돌리므로, 로컬에서 먼저 막는 게 유일한 사전 방어선 — CI 다이어트 P2). `make ci` 는 `ci-fast` 별칭이라 coverage 와 MSRV 가 빠지고, `make ci-full` 도 그 둘만 더한다 — CI 의 HWP5 Audit Gate·Docs Build·Python·Workflow Lint 는 여전히 로컬 타깃에 없으므로, 그 레인들은 머지 큐 실행 결과로 확인한다.
- [ ] umbrella만 GitHub Release를 만든다 — npm/pages는 거기에 매달려 있다. umbrella가
      bump되지 않으면 npm·문서 배포도 안 일어난다는 점을 기억.

---

## 8. 운영 함정 (실사고 기반 — CLAUDE.md 에서 이관)

- **Merge queue 활성** — `gh pr merge --squash`(특히 `--delete-branch`) 거부됨. GraphQL `enqueuePullRequest(input:{pullRequestId})` mutation 으로 큐에 넣을 것 (`mergeStateStatus=CLEAN` 이후에만 성공 — BLOCKED/UNSTABLE 중엔 대기). 큐가 PR당 CI 재실행 후 자동 머지(머지 방식은 큐 설정 소유). 큐 상태 = `repository.mergeQueue(branch:"main").entries`.
- **inter-crate 의존성은 `version = "0"` 유지 — 정확 핀으로 "고치지" 말 것.** 통합버전(`version.workspace = true`) 워크스페이스에서 release-plz 는 커밋 없는 베이스 crate 를 못 올려, 정확 핀이면 breaking bump 시 `failed to select a version` 으로 Release PR 생성이 죽음 (PR #94; `version_group`·`release_always` 는 무효 — memory `release-plz-unified-version-workspace.md`).
- **breaking/릴리스 트리거 커밋의 2대 필수 조건** (0.16.0 3막 사고): (1) subject 는 `type(scope)!:` (**`type!(scope):` 는 release_commits regex 불일치로 통째 무시**) (2) **대상 크레이트의 파일을 실제로 변경해야** 함 — 커밋→패키지 귀속은 변경 파일 경로 기반이라 빈 커밋·publish=false 크레이트만 건드린 PR 은 발화하지 않는다.
- **릴리스 완주 판정**: GitHub Release 는 release-plz 실행 **도중** 먼저 게시되고, 그 이벤트가 npm-publish 와 pypi-publish 를 **함께** 트리거한다 → release-plz·npm-publish·pypi-publish **셋 다 success** + 세 레지스트리 실측까지 확인해야 완료: sparse index(`index.crates.io/hw/pf/<crate>`) · npm(`npm view @hwpforge/mcp version`) · PyPI(`curl -s https://pypi.org/pypi/hwpforge/json | jq '.info.version, (.urls[].filename)'` — 버전이 맞고 파일이 wheel 5 + sdist 인지). **Release PR 생성 여부도 실측**: release-plz 로그의 `release_pr_output` 이 `{"prs":[]}` 면 미발화 (0.16.0 사고 — 2회 미발화 후 발견).
- **publish 검증**: crates.io API 는 샌드박스에서 막힐 수 있음 → sparse index `index.crates.io/hw/pf/<crate>` 로 확인.
- **release-plz 디버깅은 로컬 프리빌트로 재현** (CI 머지 사이클로 추측 금지): `gh release download release-plz-v0.3.159 --repo release-plz/release-plz` + 깨끗한 clone 에서 `release-plz update`. `{{ release_link }}` 는 로컬 렌더 실패 → 임시 제거 후 실험. (`release-plz-v0.3.159` 는 **CLI**(`release-plz/release-plz`) 릴리스 태그이며, `.github/workflows/release-plz.yml` 이 실제로 고정하는 `release-plz/action@…v0.5.131` 과는 버전 계열이 다르다 — action 이 내부적으로 vendor 하는 CLI 버전은 별개이므로, 재현 시 `gh release list --repo release-plz/release-plz --limit 5` 로 최신 CLI 태그를 다시 조회할 것.)
- **npm 토큰**: granular 토큰 90일 만료(npm 은 인증 실패를 **E404 로 위장**), 재발급 시 **"Bypass 2FA" 필수**(없으면 E403). ⚠️ 현 토큰 **~2026-10-10 재만료** — 영구 해결은 npm Trusted Publishing(OIDC) 전환.

---

## 9. Python 배포 (PyPI)

Rust 크레이트·npm 과 달리 Python wheel 은 release-plz 가 만들지 않는다. `.github/workflows/pypi-publish.yml` 이 별도로 돌고, 버전은 여전히 release-plz 가 계산한 워크스페이스 버전을 따른다. 아래 절차는 그 워크플로를 사람이 어떻게 부르는지를 적는다.

### 9.1 트리거와 대상

| 이벤트                               | 태그 출처                          | 게시 대상                           |
| ------------------------------------ | ---------------------------------- | ----------------------------------- |
| `release: published` (umbrella `v*`) | `release.tag_name`                 | **PyPI**                            |
| `workflow_dispatch`                  | "Use workflow from" 에서 고른 태그 | 입력 `target` (기본값 **TestPyPI**) |

**게시 대상에 GitHub Release 는 없다.** release-plz 가 만드는 워크스페이스 Release 는 발행 후 에셋을 붙일 수 없는 불변 객체라, wheel·sdist 를 그쪽에 복제하는 잡을 두지 않는다 — 특정 파일이 필요하면 PyPI 의 [files 페이지](https://pypi.org/project/hwpforge/#files)에서 받는다(`pip` 이 보는 것과 같은 바이트·sha256).

**태그를 푸시하는 것만으로는 아무 일도 일어나지 않는다.** `py-v*` push 트리거는 제거했다 — 리허설에 쓸 태그를 origin 에 올려야 "Use workflow from" 목록에 뜨는데, 그 준비 동작이 곧 프로덕션 게시가 되어 버리고(그리고 그 파일명을 영구히 태워 이후 릴리스 업로드를 hash 불일치로 깨뜨리고) 만다. 프로덕션에 닿는 길은 릴리스 이벤트와 명시적 수동 실행 둘뿐이고, 기본값은 TestPyPI 다.

수동 실행에는 태그 입력란이 없다. **Actions → PyPI Publish → Run workflow → "Use workflow from" 에서 `Tags` 를 고르고 태그를 선택**한 뒤 `target` 만 정한다. 브랜치를 고르면 `Resolve › Tag` 가 거부한다. 그 드롭다운에 태그가 보이려면 **그 태그의 트리에 `pypi-publish.yml` 이 있어야** 한다 — 이 워크플로가 main 에 들어가기 전에 찍힌 태그로는 수동 실행을 할 수 없다.

### 9.2 태그 문법

워크스페이스 릴리스 태그는 `vX.Y.Z` 하나뿐이고 Cargo 워크스페이스 버전과 **정확히** 같아야 한다. Python 전용 태그는 네 가지뿐이다.

| 형태                | 예                   | 언제                            |
| ------------------- | -------------------- | ------------------------------- |
| `py-vX.Y.Z`         | `py-v0.16.4`         | 같은 버전을 그대로 다시 게시    |
| `py-vX.Y.Z.postM`   | `py-v0.16.4.post1`   | 같은 소스를 다시 빌드 (M ≥ 1)   |
| `py-vX.Y.Z.N`       | `py-v0.16.4.1`       | Python 층만 고친 릴리스 (N ≥ 1) |
| `py-vX.Y.Z.N.postM` | `py-v0.16.4.1.post2` | 그 릴리스의 재빌드              |

epoch·pre-release·dev·local 세그먼트와 다섯 번째 release 성분은 거부된다. PEP 440 정규화 결과가 `py-v` 를 뗀 문자열과 **글자 그대로** 같아야 하므로 `py-v0.16.04` 도 거부된다. 앞 세 성분은 항상 현재 Cargo 버전이어야 하고, Rust 크레이트 버전은 이 경로에서 절대 바뀌지 않는다. `post` 는 PEP 440 이 "소프트웨어에 영향 없는 정정" 으로 한정하므로 **코드 변경에는 쓰지 않는다** — 코드가 바뀌면 `.N` 이다.

규칙의 소유자는 `.github/scripts/resolve_release_tag.py` 이고, 같은 파일의 `--self-check` 가 수용·거부 표를 계정 없이 재현한다.

```console
uv run --no-project --with packaging python .github/scripts/resolve_release_tag.py --self-check
```

### 9.3 Python 전용 태그를 붙이는 절차

1. 고칠 내용을 main 에 머지한다 (평시 Python 변경은 다음 워크스페이스 릴리스에 그냥 실려 나가므로, 이 절차는 **긴급 수정**용이다).
2. 그 커밋에 태그를 붙여 푸시한다 — `git tag py-v0.16.4.1 <commit>` 후 `git push origin py-v0.16.4.1`. 이 푸시는 아무것도 실행하지 않는다. 태그를 "Use workflow from" 목록에 띄우는 것이 전부다.
3. **Actions → PyPI Publish → Run workflow → "Use workflow from" 에서 그 태그를 고르고 `target: pypi`** 로 실행한다. TestPyPI 로 먼저 한 번 돌려 보고 싶으면 같은 태그에 `target: testpypi` 로 실행한 뒤 다시 `pypi` 로 실행하면 된다 — 두 대상은 서로 다른 environment 와 index 를 쓴다.
4. 워크플로는 **태그 ref 위에서** 돌고 모든 잡이 같은 커밋(`github.sha`)을 체크아웃한다 — 체크아웃할 ref 가 입력에서 오지 않으므로 기본 브랜치와 공유되는 캐시를 오염시킬 경로가 없다. 같은 이유로 이 워크플로에는 캐시 액션이 하나도 없다 (릴리스 빌드는 cold 가 정상이다). 게시 직전에 태그가 여전히 그 커밋을 가리키는지 다시 확인한다. `pyproject.toml` 의 `dynamic = ["version"]` 은 러너의 일회용 체크아웃에서만 정적 버전으로 바뀌고 빌드 뒤 원본이 복원된다 (`git diff --exit-code` 로 증명). 저장소에는 아무것도 커밋되지 않는다.

release-plz 의 `git_tag_name = "v{{ version }}"` 과 접두사가 달라 충돌하지 않는다.

### 9.4 검증된 것과 남은 것

이 절은 게시 경로에서 **무엇이 실제로 확인됐는지**를 적는다. 아래 "릴리스마다 도는 점검표"만 반복 작업이고, 나머지는 기록이다.

**2026-09-18 TestPyPI 리허설에서 실측**(전부 `workflow_dispatch` + `target=testpypi`):

- 네 가지 태그 형태(`py-v0.16.4` · `py-v0.16.4.post1` · `py-v0.16.4.1` · `py-v0.16.4.1.post2`) 각각이 `Resolve › Tag` 를 통과하고 TestPyPI 에 6개 파일(wheel 5 + sdist)을 올렸다. wheel 파일명·`METADATA`·`PKG-INFO` 버전이 태그와 일치.
- 거부되어야 할 태그(`py-v0.16.4.0` 등)와 브랜치 선택은 같은 자리에서 실패했다.
- 새 가상환경에서 wheel 설치·import 성공. sdist 강제 설치가 소스에서 1분 08초에 빌드됐다.
- 같은 run 의 게시 잡 재실행이 6개 전부 `already exists, skipping` 으로 통과.
- 이미 게시된 태그를 **새로 dispatch** 하면 재빌드된 바이트가 달라 `--check-url` 이 거부한다(exit 2) — `Local file and index file do not match for hwpforge-…-macosx_10_12_x86_64.whl`. 고장이 아니라 **안전 속성**이다: 이미 올라간 파일을 덮어쓰지 않는다. 멱등성은 오직 이 검사에서 온다(PyPI 는 파일명 재사용을 영구히 거부한다).

**프로덕션 첫 업로드**: `0.16.5` 에서 PyPI `hwpforge` 에 wheel 5 + sdist 가 올라갔다 (`.claude/guides/status.md`).

**아직 실측하지 않은 것**: 토큰으로 진짜 3/6 부분 게시 상태를 만든 뒤의 복구. 절차는 — 아티팩트를 내려받아 `uv publish --trusted-publishing never --publish-url https://test.pypi.org/legacy/ --check-url https://test.pypi.org/simple/ <파일 3개>` 로 셋만 올리고(토큰 사용), 그다음 **Actions → 그 run → "Re-run failed jobs"** 로 `Publish › TestPyPI` 만 다시 돌려 로그에 **skip 3 · upload 3** 이 찍히는지 본다. 아티팩트 보관은 14일이므로 그 안에 해야 한다. 새로 dispatch 해서는 안 된다(위 `--check-url` 거부).

#### 릴리스마다 도는 점검표

- [ ] 레지스트리 실측 — `curl -s https://pypi.org/pypi/hwpforge/json | jq '.info.version, (.urls[].filename)'` 로 버전과 파일 6개(wheel 5 + sdist)를 확인한다. TestPyPI 는 같은 명령의 `https://test.pypi.org/pypi/hwpforge/json`.
- [ ] 설치 확인 (wheel 경로) — 새 환경에서 설치하고 import 한다:

```console
uv venv /tmp/hf-check
uv pip install --python /tmp/hf-check/bin/python --index-url https://pypi.org/simple/ "hwpforge==<ver>"
uv run --python /tmp/hf-check/bin/python python -c "import hwpforge, sys; print(hwpforge.__version__, sys.platform)"
```

- [ ] sdist 가 실제로 컴파일되는지 — wheel 이 있으면 설치 해석기가 그것을 고르므로 명시적으로 막아야 한다 (Rust 1.92 + maturin 필요):

```console
uv venv /tmp/hf-sdist
uv pip install --python /tmp/hf-sdist/bin/python --no-binary hwpforge \
  --index-url https://pypi.org/simple/ "hwpforge==<ver>"
```

TestPyPI 에 올린 것을 같은 방법으로 확인할 때는 인덱스를 둘 다 봐야 한다 — 빌드 의존성(maturin)은 **PyPI** 에서 와야 하는데 TestPyPI 에는 maturin 0.7.9 뿐이라, `--index-url` 을 TestPyPI 로만 두면 소스 빌드가 깨진다. 그래서 기본 인덱스는 PyPI 로 두고 `--extra-index-url https://test.pypi.org/simple/ --index-strategy unsafe-best-match` 를 더한다. wheel 경로는 의존성이 없으므로 `--index-url` 하나면 충분하다.

### 9.5 Release PR 과 Python 배포의 관계

**Release PR 을 막는 Python 쪽 조건은 없다.** `pypi-publish.yml` 은 main 에 있고, §9.6 선행 작업도 끝나 있으며, 첫 wheel 은 `0.16.5` 에서 나갔다. 평소처럼 §8 의 머지 큐 규칙만 지키면 된다.

지켜야 할 제약은 하나 남는다: **`release: published` 이벤트는 재생할 수 없다.** 어떤 릴리스에서 PyPI 게시가 실패하면 그 이벤트를 다시 발생시킬 수 없으므로 손으로 복구해야 하는데, 복구법은 **얼마나 올라갔는지에 따라 갈린다**:

- **하나도 안 올라갔으면** — §9.1 의 `workflow_dispatch` 로 그 태그를 골라 `target: pypi` 로 돌린다.
- **일부만 올라갔으면** — 새로 dispatch 하면 안 된다. 재빌드한 바이트가 달라 `--check-url` 이 이미 올라간 파일에서 거부한다(§9.4). **Actions → 그 파일들을 만든 run → 게시 잡만 재실행** 해야 한다. 아티팩트 보관은 14일이므로 그 안에 해야 하고, 지났으면 남은 파일을 `.N` 버전으로 다시 내보내는 수밖에 없다(PyPI 는 파일명 재사용을 영구히 거부한다).

### 9.6 사용자(관리자) 선행 작업

워크플로는 이 표가 채워지기 전에는 **조용히 건너뛰지 않고 실패한다** — `--trusted-publishing always` 는 토큰으로 되돌아가지 않고 그 자리에서 죽는다. 바꿔 말해 게시가 성공했다는 것 자체가 그 대상의 표가 채워졌다는 증거다: TestPyPI 는 2026-09-18 리허설이, PyPI 는 `0.16.5` 업로드가 그렇게 증명했다. 아래는 계정을 새로 만들거나 조직이 바뀔 때 다시 보는 표다.

| 무엇                          | 값                                                                                                       |
| ----------------------------- | -------------------------------------------------------------------------------------------------------- |
| PyPI 계정 + 2FA               | 이름 `hwpforge` 는 예약되지 않는다 — pending publisher 는 선점 장치가 아니므로 첫 업로드를 미루지 않는다 |
| TestPyPI 계정 + 2FA           | 리허설용, 별도 계정                                                                                      |
| PyPI pending publisher        | owner `ai-screams` · repository `HwpForge` · workflow `pypi-publish.yml` · environment `pypi`            |
| TestPyPI pending publisher    | 같은 값에 environment `testpypi`                                                                         |
| GitHub environment `pypi`     | 같은 이름으로 생성. 필요하면 reviewer 를 붙여 수동 승인 게이트로 쓴다                                    |
| GitHub environment `testpypi` | 같은 이름으로 생성, reviewer 없이                                                                        |

증빙은 `gh api repos/ai-screams/HwpForge/environments` 출력을 `.docs/prs/` 에 남긴다. 토큰은 쓰지 않는다 — OIDC 뿐이다.

### 9.7 이용자에게 알릴 것

정확 핀(`==X.Y.Z`)을 쓰는 이용자는 `py-vX.Y.Z.N` 로 나가는 Python 전용 수정을 받지 못한다. 문서와 안내에서는 `\~=X.Y.Z` 를 권한다.
