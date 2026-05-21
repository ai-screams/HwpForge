#!/usr/bin/env python3
"""HWP5 audit fidelity gate.

Compares the current run of `cargo run --example audit_batch` against the
committed baseline at `.audit/hwp5_baseline.json`. Exits with code 1 when a
regression is detected.

Regression rules:
- Any baseline category that grows in count → regression.
- Any new category that appears in the current run → regression.
- Any new fixture failure → regression.

Categories that shrink or disappear are accepted (this is the intended
direction of travel for Phase 11). The gate prints a one-line summary plus a
detailed diff on regression so PR authors see exactly which categories drifted.

Usage:
    python3 scripts/audit_hwp5_gate.py \\
        --baseline .audit/hwp5_baseline.json \\
        --current  /tmp/hwp5_audit_current.json
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def load_report(path: Path) -> dict:
    if not path.exists():
        sys.stderr.write(f"audit gate: missing report at {path}\n")
        sys.exit(2)
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        sys.stderr.write(f"audit gate: invalid JSON at {path}: {exc}\n")
        sys.exit(2)


def diff_categories(baseline: dict, current: dict) -> list[str]:
    regressions: list[str] = []
    base_cats = baseline.get("categories", {})
    cur_cats = current.get("categories", {})

    for name, stats in cur_cats.items():
        cur_count = stats.get("count", 0)
        base_count = base_cats.get(name, {}).get("count", 0)
        if name not in base_cats:
            regressions.append(
                f"new category: {name} (count={cur_count})"
            )
        elif cur_count > base_count:
            regressions.append(
                f"category {name}: {base_count} → {cur_count} (+{cur_count - base_count})"
            )

    base_failures = {f["fixture"] for f in baseline.get("failures", [])}
    cur_failures = current.get("failures", [])
    for failure in cur_failures:
        if failure["fixture"] not in base_failures:
            regressions.append(
                f"new failure: {failure['fixture']} — {failure['error']}"
            )

    return regressions


def summarize_improvements(baseline: dict, current: dict) -> list[str]:
    improvements: list[str] = []
    base_cats = baseline.get("categories", {})
    cur_cats = current.get("categories", {})

    for name, stats in base_cats.items():
        base_count = stats.get("count", 0)
        cur_count = cur_cats.get(name, {}).get("count", 0)
        if cur_count < base_count:
            improvements.append(
                f"category {name}: {base_count} → {cur_count} (-{base_count - cur_count})"
            )

    return improvements


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--baseline",
        type=Path,
        required=True,
        help="Path to committed baseline JSON",
    )
    parser.add_argument(
        "--current",
        type=Path,
        required=True,
        help="Path to current audit_batch output JSON",
    )
    args = parser.parse_args()

    baseline = load_report(args.baseline)
    current = load_report(args.current)

    regressions = diff_categories(baseline, current)
    improvements = summarize_improvements(baseline, current)

    base_total = baseline.get("total_warnings", 0)
    cur_total = current.get("total_warnings", 0)
    print(
        f"HWP5 audit gate: baseline_total={base_total} current_total={cur_total} "
        f"(Δ={cur_total - base_total:+d})"
    )

    if improvements:
        print("Improvements:")
        for line in improvements:
            print(f"  ✓ {line}")

    if regressions:
        print("Regressions detected:")
        for line in regressions:
            print(f"  ✗ {line}")
        print(
            "\nIf this regression is intentional, regenerate the baseline with:\n"
            "  make audit-hwp5-baseline"
        )
        return 1

    print("No regressions detected.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
