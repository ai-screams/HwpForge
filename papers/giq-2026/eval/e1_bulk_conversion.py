#!/usr/bin/env python3
"""
실험 1: DDI 변환 — 5,411건 정부 문서를 Markdown으로 변환.

HWPX → Markdown (본선)
HWP5 → HWPX → Markdown (레거시)

Usage:
  python e1_bulk_conversion.py                    # 전체 실행
  python e1_bulk_conversion.py --dry-run          # 집계만
  python e1_bulk_conversion.py --ministry 행안부   # 특정 부처만
"""

import argparse
import json
import os
import subprocess
import time
from datetime import datetime
from pathlib import Path

# ── 설정 ──────────────────────────────────────────────────────────
PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent.parent  # HwpForge/
PAPERS_ROOT = Path(__file__).resolve().parent.parent                 # .docs/papers/

HWPFORGE_BIN = PROJECT_ROOT / "target" / "release" / "hwpforge"
BULK_DIR = PAPERS_ROOT / "ref" / "hwp" / "corpus" / "bulk"
RESULTS_DIR = PAPERS_ROOT / "plan" / "results"

MINISTRIES = [
    "행안부", "산업부", "국방부", "기재부", "외교부",
    "법무부", "중기부", "금융위", "문체부", "교육부",
]

DELAY_PER_FILE = 0.05  # 로컬 변환이므로 짧은 대기


def run_hwpforge(args: list[str], timeout: int = 120) -> tuple[bool, str]:
    """HwpForge CLI 실행. (성공 여부, 출력/에러)"""
    cmd = [str(HWPFORGE_BIN)] + args
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        if result.returncode == 0:
            return True, result.stdout.strip()
        else:
            return False, result.stderr.strip()
    except subprocess.TimeoutExpired:
        return False, "TIMEOUT"
    except Exception as e:
        return False, str(e)


def convert_hwpx_to_md(hwpx_path: Path, out_dir: Path) -> dict:
    """HWPX → Markdown 변환."""
    success, msg = run_hwpforge(["to-md", str(hwpx_path), "-o", str(out_dir)])
    return {
        "file": hwpx_path.name,
        "format": "hwpx",
        "path": "hwpx→md",
        "success": success,
        "message": msg[:200] if msg else "",
    }


def convert_hwp5_to_md(hwp_path: Path, out_dir: Path) -> dict:
    """HWP5 → HWPX → Markdown 변환 (2단계)."""
    # Step 1: HWP5 → HWPX
    hwpx_out = out_dir / (hwp_path.stem + ".hwpx")
    success1, msg1 = run_hwpforge(["convert-hwp5", str(hwp_path), "-o", str(hwpx_out)], timeout=300)

    if not success1:
        return {
            "file": hwp_path.name,
            "format": "hwp5",
            "path": "hwp5→hwpx",
            "success": False,
            "message": msg1[:200],
            "warnings": 0,
        }

    # 경고 수 추출
    warnings = 0
    import re
    w_match = re.search(r'(\d+) warnings?', msg1)
    if w_match:
        warnings = int(w_match.group(1))

    # Step 2: HWPX → Markdown
    success2, msg2 = run_hwpforge(["to-md", str(hwpx_out), "-o", str(out_dir)])

    return {
        "file": hwp_path.name,
        "format": "hwp5",
        "path": "hwp5→hwpx→md",
        "success": success1 and success2,
        "hwpx_success": success1,
        "md_success": success2,
        "warnings": warnings,
        "message": (msg1 + " | " + msg2)[:200] if not (success1 and success2) else "",
    }


def process_ministry(ministry: str, dry_run: bool = False) -> dict:
    """부처별 변환 실행."""
    ministry_dir = BULK_DIR / f"보도자료_{ministry}"
    if not ministry_dir.exists():
        print(f"  {ministry}: 폴더 없음")
        return {"ministry": ministry, "error": "폴더 없음"}

    hwpx_files = sorted(ministry_dir.glob("*.hwpx"))
    hwp_files = sorted(ministry_dir.glob("*.hwp"))

    print(f"  {ministry}: HWPX {len(hwpx_files)}건 + HWP5 {len(hwp_files)}건")

    if dry_run:
        return {
            "ministry": ministry,
            "hwpx_count": len(hwpx_files),
            "hwp5_count": len(hwp_files),
            "total": len(hwpx_files) + len(hwp_files),
        }

    # 출력 디렉토리
    out_dir = RESULTS_DIR / "e1" / ministry
    out_dir.mkdir(parents=True, exist_ok=True)

    results = []

    # HWPX → Markdown
    for i, f in enumerate(hwpx_files):
        r = convert_hwpx_to_md(f, out_dir)
        results.append(r)
        if (i + 1) % 50 == 0:
            ok = sum(1 for x in results if x["success"])
            print(f"    HWPX {i+1}/{len(hwpx_files)} (성공 {ok})")
        time.sleep(DELAY_PER_FILE)

    # HWP5 → HWPX → Markdown
    for i, f in enumerate(hwp_files):
        r = convert_hwp5_to_md(f, out_dir)
        results.append(r)
        if (i + 1) % 50 == 0:
            hwp5_results = [x for x in results if x["format"] == "hwp5"]
            ok = sum(1 for x in hwp5_results if x["success"])
            print(f"    HWP5 {i+1}/{len(hwp_files)} (성공 {ok})")
        time.sleep(DELAY_PER_FILE)

    # 집계
    hwpx_results = [r for r in results if r["format"] == "hwpx"]
    hwp5_results = [r for r in results if r["format"] == "hwp5"]

    hwpx_ok = sum(1 for r in hwpx_results if r["success"])
    hwp5_ok = sum(1 for r in hwp5_results if r["success"])
    hwp5_warnings = [r.get("warnings", 0) for r in hwp5_results if r.get("warnings", 0) > 0]
    avg_warnings = sum(hwp5_warnings) / len(hwp5_warnings) if hwp5_warnings else 0

    summary = {
        "ministry": ministry,
        "hwpx_count": len(hwpx_files),
        "hwpx_success": hwpx_ok,
        "hwpx_rate": round(hwpx_ok / len(hwpx_files) * 100, 1) if hwpx_files else 0,
        "hwp5_count": len(hwp_files),
        "hwp5_success": hwp5_ok,
        "hwp5_rate": round(hwp5_ok / len(hwp_files) * 100, 1) if hwp_files else 0,
        "total": len(results),
        "total_success": hwpx_ok + hwp5_ok,
        "total_rate": round((hwpx_ok + hwp5_ok) / len(results) * 100, 1) if results else 0,
        "avg_warnings": round(avg_warnings, 1),
        "results": results,
    }

    print(f"    완료: HWPX {hwpx_ok}/{len(hwpx_files)} ({summary['hwpx_rate']}%), "
          f"HWP5 {hwp5_ok}/{len(hwp_files)} ({summary['hwp5_rate']}%), "
          f"경고 평균 {avg_warnings:.1f}")

    return summary


def main():
    parser = argparse.ArgumentParser(description="실험 1: DDI 벌크 변환")
    parser.add_argument("--ministry", type=str, help="특정 부처만 실행")
    parser.add_argument("--dry-run", action="store_true", help="집계만 (변환 안 함)")
    args = parser.parse_args()

    print(f"실험 1: DDI 벌크 변환")
    print(f"HwpForge: {HWPFORGE_BIN}")
    print(f"코퍼스:   {BULK_DIR}")
    print(f"결과:     {RESULTS_DIR / 'e1'}")
    if args.dry_run:
        print("모드:     DRY RUN")
    print()

    # HwpForge 확인
    if not args.dry_run:
        ok, ver = run_hwpforge(["--version"])
        if not ok:
            print(f"ERROR: hwpforge 실행 실패 — {ver}")
            return
        print(f"Version:  {ver}")

    RESULTS_DIR.mkdir(parents=True, exist_ok=True)

    ministries = [args.ministry] if args.ministry else MINISTRIES
    all_summaries = []

    start_time = datetime.now()

    for ministry in ministries:
        summary = process_ministry(ministry, dry_run=args.dry_run)
        all_summaries.append(summary)

    elapsed = (datetime.now() - start_time).total_seconds()

    # 전체 결과 저장
    report_path = RESULTS_DIR / "e1_report.json"
    with open(report_path, "w", encoding="utf-8") as f:
        # results 필드는 너무 크니 별도 저장
        for s in all_summaries:
            detail_path = RESULTS_DIR / "e1" / f"{s['ministry']}_detail.json"
            if "results" in s:
                detail_path.parent.mkdir(parents=True, exist_ok=True)
                with open(detail_path, "w", encoding="utf-8") as df:
                    json.dump(s.pop("results"), df, ensure_ascii=False, indent=2)
        json.dump(all_summaries, f, ensure_ascii=False, indent=2)

    # 요약 출력
    print(f"\n{'='*60}")
    print(f"실험 1 결과 요약")
    print(f"{'='*60}")
    print(f"{'부처':>6} | {'전체':>5} | {'HWPX':>5} | {'성공':>5} | {'비율':>6} | {'HWP5':>5} | {'성공':>5} | {'비율':>6} | {'경고':>5}")
    print(f"{'-'*6}-+-{'-'*5}-+-{'-'*5}-+-{'-'*5}-+-{'-'*6}-+-{'-'*5}-+-{'-'*5}-+-{'-'*6}-+-{'-'*5}")

    total_all = total_ok = total_hwpx = total_hwpx_ok = total_hwp5 = total_hwp5_ok = 0
    for s in all_summaries:
        if "error" in s:
            continue
        hx = s.get("hwpx_count", 0)
        hxo = s.get("hwpx_success", 0)
        h5 = s.get("hwp5_count", 0)
        h5o = s.get("hwp5_success", 0)
        hxr = s.get("hwpx_rate", 0)
        h5r = s.get("hwp5_rate", 0)
        aw = s.get("avg_warnings", 0)
        total_all += hx + h5
        total_ok += hxo + h5o
        total_hwpx += hx
        total_hwpx_ok += hxo
        total_hwp5 += h5
        total_hwp5_ok += h5o
        print(f"{s['ministry']:>6} | {hx+h5:>5} | {hx:>5} | {hxo:>5} | {hxr:>5.1f}% | {h5:>5} | {h5o:>5} | {h5r:>5.1f}% | {aw:>5.1f}")

    overall_rate = round(total_ok / total_all * 100, 1) if total_all else 0
    print(f"{'-'*6}-+-{'-'*5}-+-{'-'*5}-+-{'-'*5}-+-{'-'*6}-+-{'-'*5}-+-{'-'*5}-+-{'-'*6}-+-{'-'*5}")
    print(f"{'합계':>6} | {total_all:>5} | {total_hwpx:>5} | {total_hwpx_ok:>5} | {round(total_hwpx_ok/total_hwpx*100,1) if total_hwpx else 0:>5.1f}% | {total_hwp5:>5} | {total_hwp5_ok:>5} | {round(total_hwp5_ok/total_hwp5*100,1) if total_hwp5 else 0:>5.1f}% |")

    print(f"\n소요 시간: {elapsed:.0f}초 ({elapsed/60:.1f}분)")
    print(f"결과 저장: {report_path}")


if __name__ == "__main__":
    main()
