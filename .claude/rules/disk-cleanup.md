# 빌드 산출물 디스크 정리 규칙

`target/` 가 134GB 까지 자라 디스크 잔여 170MB 사고가 났다 (2026-08-18 —
그 전엔 incremental 94GB 사고). 세션 중 아래 임계값을 넘으면 정리한다.

## 임계값 (하나라도 해당하면 정리 실행)

- 디스크 잔여 `< 20GB` (`df -h /`) — **즉시**
- `target/debug/incremental > 30GB` 또는 `target/ > 80GB` (`du -sh`) —
  큰 빌드 작업(전 워크스페이스 빌드·coverage·장기 세션) 시작 전에 확인

## 삭제 순서 (전부 재생성 가능한 산출물)

1. `target/llvm-cov-target` (coverage 산출물 — 항상 안전)
2. `target/debug/incremental` (최대 덩어리 — 단 **cargo 빌드 진행 중엔
   보류**, 진행 빌드를 깨뜨림. 빌드 종료 직후 삭제)
3. standalone 워크스페이스 산출물: `fuzz/target` ·
   `.docs/papers/EAAI/eval/oracle-rs/target`

## 보존 (삭제 금지)

- `target/debug/deps` — warm 의존성 캐시. 지우면 cold 재빌드 15분+.
- `fuzz/corpus` · `.docs/papers` — 재생성 불가 자산.

## 실행 주의

- `rm` 은 대화형 alias — 스크립트 삭제는 `/bin/rm -rf` 또는 `rm -f`.
- APFS purgeable 반환 때문에 삭제 직후 `df` 가 예상보다 크게 회복될 수
  있다 — 판단은 삭제 후 실측으로.
