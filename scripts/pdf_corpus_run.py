#!/usr/bin/env python3
"""PDF corpus 검증 러너 (W6b — 커밋된 파라미터화 러너, 일회성 스크립트 금지).

corpus 의 각 문서를 `hwpforge to-pdf` 로 **프로세스 격리** 실행하고,
per-doc 타임아웃·RSS 실측·경고 버킷을 manifest(JSONL) 로 남긴다.
corpus 자체는 `.docs` 내부 자산(미커밋)이라 경로를 인자로 받는다.

판정 계층 (Codex 게이트 2 반영 — "panic 0" 단독은 빈 게이트):
  ok            : exit 0 — PDF 산출 (경고 버킷·문단 스킵은 별도 지표)
  fail_closed   : exit 2 + 구조화 오류 JSON (convert/decode/render 거부)
  crash         : 그 외 exit / 시그널 — **게이트 위반**
  timeout       : per-doc 예산 초과 — **게이트 위반**

사용:
  python3 scripts/pdf_corpus_run.py --corpus <dir> --out <dir> \
      [--bin target/debug/hwpforge] [--timeout 60] [--sample 100] [--seed 42] \
      [-- extra to-pdf args...]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import random
import resource
import subprocess
import sys
import time
from pathlib import Path


def git_commit(repo: Path) -> str:
    try:
        out = subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "HEAD"],
            capture_output=True, text=True, timeout=10, check=True,
        )
        return out.stdout.strip()
    except Exception:
        return "unknown"


def sha256_head(path: Path, cap: int = 1 << 20) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        h.update(f.read(cap))
    return h.hexdigest()[:16]


def last_json(text: str) -> dict:
    try:
        return json.loads(text.strip().splitlines()[-1]) if text.strip() else {}
    except (json.JSONDecodeError, IndexError):
        return {}


def classify(exit_code: int, stdout: str, stderr: str, timed_out: bool) -> tuple[str, str | None]:
    """(outcome, error_code). CLI 는 json 모드 오류를 **stderr** 로 낸다."""
    if timed_out:
        return "timeout", None
    if exit_code == 0:
        return "ok", None
    if exit_code < 0:
        return "crash", f"signal {-exit_code}"
    code = last_json(stderr).get("code") or last_json(stdout).get("code")
    if exit_code in (1, 2) and code:
        return "fail_closed", code
    return "crash", f"exit {exit_code} without structured error"


def run_one(bin_path: Path, doc: Path, out_pdf: Path, timeout: float, extra: list[str]) -> dict:
    cmd = [str(bin_path), "--json", "to-pdf", str(doc), "-o", str(out_pdf), *extra]
    before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    start = time.monotonic()
    timed_out = False
    stderr = ""
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        exit_code, stdout, stderr = proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired as e:
        timed_out = True
        exit_code = -999
        stdout = (e.stdout or b"").decode("utf-8", "replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
    duration = time.monotonic() - start
    # ru_maxrss 는 자식 프로세스 최대값의 고수위 — 문서별 근사치 (macOS bytes, Linux KB).
    peak_rss = max(resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss, before)

    outcome, error_code = classify(exit_code, stdout, stderr, timed_out)
    record: dict = {
        "input": str(doc),
        "sha256_head": sha256_head(doc),
        "size_bytes": doc.stat().st_size,
        "outcome": outcome,
        "error_code": error_code,
        "duration_s": round(duration, 3),
        "peak_rss_highwater": peak_rss,
    }
    if outcome == "ok":
        try:
            payload = json.loads(stdout.strip().splitlines()[-1])
            record["warning_counts"] = payload.get("warning_counts", {})
            codes: dict[str, int] = {}
            for w in payload.get("warnings", []):
                key = f"{w.get('stage')}:{w.get('code')}"
                codes[key] = codes.get(key, 0) + 1
            record["warning_codes"] = codes
        except (json.JSONDecodeError, IndexError):
            record["outcome"] = "crash"
            record["error_code"] = "ok exit without parseable JSON"
    return record


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--corpus", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--bin", type=Path, default=Path("target/debug/hwpforge"))
    ap.add_argument("--timeout", type=float, default=60.0)
    ap.add_argument("--sample", type=int, default=0, help="0 = 전수")
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("extra", nargs="*", help="추가 to-pdf 인자 (-- 뒤)")
    args = ap.parse_args()

    docs = sorted(args.corpus.rglob("*.hwp"))
    if not docs:
        print(f"no .hwp under {args.corpus}", file=sys.stderr)
        return 2
    total_manifested = len(docs)
    if args.sample and args.sample < len(docs):
        # 층화: 크기 상위 10% 는 반드시 포함 + 나머지는 고정 seed 무작위.
        by_size = sorted(docs, key=lambda p: p.stat().st_size, reverse=True)
        top = by_size[: max(1, args.sample // 10)]
        rest_pool = [d for d in docs if d not in set(top)]
        rng = random.Random(args.seed)
        rest = rng.sample(rest_pool, min(args.sample - len(top), len(rest_pool)))
        docs = sorted(set(top) | set(rest))

    args.out.mkdir(parents=True, exist_ok=True)
    pdf_dir = args.out / "pdf"
    pdf_dir.mkdir(exist_ok=True)
    manifest_path = args.out / "manifest.jsonl"

    records = []
    with open(manifest_path, "w", encoding="utf-8") as mf:
        for i, doc in enumerate(docs, 1):
            out_pdf = pdf_dir / f"{doc.stem}-{sha256_head(doc)[:8]}.pdf"
            rec = run_one(args.bin, doc, out_pdf, args.timeout, args.extra)
            records.append(rec)
            mf.write(json.dumps(rec, ensure_ascii=False) + "\n")
            if i % 50 == 0 or i == len(docs):
                print(f"[{i}/{len(docs)}] ok={sum(r['outcome'] == 'ok' for r in records)}", flush=True)

    outcomes: dict[str, int] = {}
    error_codes: dict[str, int] = {}
    warning_codes: dict[str, int] = {}
    for r in records:
        outcomes[r["outcome"]] = outcomes.get(r["outcome"], 0) + 1
        if r["error_code"]:
            error_codes[r["error_code"]] = error_codes.get(r["error_code"], 0) + 1
        for k, v in r.get("warning_codes", {}).items():
            warning_codes[k] = warning_codes.get(k, 0) + v

    summary = {
        "commit": git_commit(Path(__file__).resolve().parent.parent),
        "os": platform.platform(),
        "bin": str(args.bin),
        "extra_args": args.extra,
        "timeout_s": args.timeout,
        "corpus": str(args.corpus),
        "total_manifested": total_manifested,
        "executed": len(records),
        "outcomes": outcomes,
        "gate_violations": outcomes.get("crash", 0) + outcomes.get("timeout", 0),
        "success_rate": round(outcomes.get("ok", 0) / len(records), 4) if records else 0.0,
        "error_codes": dict(sorted(error_codes.items(), key=lambda kv: -kv[1])),
        "warning_codes": dict(sorted(warning_codes.items(), key=lambda kv: -kv[1])),
    }
    summary_path = args.out / "summary.json"
    summary_path.write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 1 if summary["gate_violations"] else 0


if __name__ == "__main__":
    sys.exit(main())
