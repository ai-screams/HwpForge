#!/usr/bin/env python3
"""PDF 오버레이 비교기 (에픽 §7 상설 회귀 검출기 — W5 시각 게이트에서 승격).

두 PDF 를 래스터화해 잉크를 겹친다: **빨강 = ours, 파랑 = reference(한컴),
일치 = 검정**. 어긋난 잉크는 색 번짐으로 즉시 드러난다.

주의: y 축 비교에 pdftotext em-box 를 쓰지 말 것 — FontDescriptor ascent
기재 차이(krilla=typo vs Quartz=hhea)로 추출 박스만 다르게 나온다 (실측
2026-08-10). 잉크 수준 비교는 이 래스터 오버레이 또는 콘텐트 스트림 Tm
해석으로 한다.

의존: ghostscript(`gs`) + Pillow. 사용:
  python3 scripts/pdf_overlay_diff.py ours.pdf hancom.pdf -o out_dir [--dpi 150] [--pages 1 2]
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    from PIL import Image, ImageChops
except ImportError:  # pragma: no cover
    print("Pillow 필요: pip install Pillow", file=sys.stderr)
    sys.exit(2)


def rasterize(pdf: Path, out_dir: Path, tag: str, dpi: int) -> list[Path]:
    try:
        subprocess.run(
            [
                "gs", "-dNOPAUSE", "-dBATCH", "-dQUIET", "-sDEVICE=pnggray", f"-r{dpi}",
                f"-sOutputFile={out_dir}/{tag}-%d.png", str(pdf),
            ],
            check=True,
        )
    except FileNotFoundError:
        print("ghostscript(`gs`) 필요: brew install ghostscript", file=sys.stderr)
        sys.exit(2)
    return sorted(out_dir.glob(f"{tag}-*.png"), key=lambda p: int(p.stem.rsplit("-", 1)[1]))


def overlay(ours: Path, reference: Path, out: Path) -> float:
    """겹침 PNG 를 쓰고, 불일치 잉크 비율(0.0=완전 일치)을 반환한다."""
    o = Image.open(ours).convert("L")
    h = Image.open(reference).convert("L")
    w, ht = min(o.width, h.width), min(o.height, h.height)
    o, h = o.crop((0, 0, w, ht)), h.crop((0, 0, w, ht))
    oi, hi = ImageChops.invert(o), ImageChops.invert(h)  # 잉크 = 밝음
    r = ImageChops.invert(hi)
    g = ImageChops.invert(ImageChops.lighter(oi, hi))
    b = ImageChops.invert(oi)
    Image.merge("RGB", (r, g, b)).save(out)
    # 불일치 지표: 한쪽에만 있는 잉크 픽셀 / 전체 잉크 픽셀 (임계 128).
    diff = ImageChops.difference(oi, hi).point(lambda v: 255 if v >= 128 else 0)
    union = ImageChops.lighter(oi, hi).point(lambda v: 255 if v >= 128 else 0)
    diff_px = sum(1 for v in diff.getdata() if v)
    union_px = sum(1 for v in union.getdata() if v)
    return diff_px / union_px if union_px else 0.0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("ours", type=Path)
    ap.add_argument("reference", type=Path)
    ap.add_argument("-o", "--out", type=Path, required=True)
    ap.add_argument("--dpi", type=int, default=150)
    ap.add_argument("--pages", type=int, nargs="*", help="1-기반 쪽 번호 (기본 전체)")
    ap.add_argument("--max-diff", type=float, default=1.0, help="불일치 비율 상한 (초과 시 exit 1)")
    args = ap.parse_args()

    args.out.mkdir(parents=True, exist_ok=True)
    worst = 0.0
    page_mismatch = False
    with tempfile.TemporaryDirectory() as td:
        tdp = Path(td)
        ours_pages = rasterize(args.ours, tdp, "ours", args.dpi)
        ref_pages = rasterize(args.reference, tdp, "ref", args.dpi)
        n = min(len(ours_pages), len(ref_pages))
        if len(ours_pages) != len(ref_pages):
            # 쪽수 = 유일한 구조 신호 — 잉크 비율/임계와 무관하게 무조건 실패.
            print(f"쪽수 불일치: ours {len(ours_pages)} vs reference {len(ref_pages)}")
            page_mismatch = True
        pages = args.pages or range(1, n + 1)
        for p in pages:
            if p < 1 or p > n:
                continue
            out_png = args.out / f"p{p}-overlay.png"
            ratio = overlay(ours_pages[p - 1], ref_pages[p - 1], out_png)
            worst = max(worst, ratio)
            print(f"p{p}: 불일치 잉크 {ratio:.4f} → {out_png}")
    print(f"worst = {worst:.4f} (상한 {args.max_diff})" + (" · 쪽수 불일치" if page_mismatch else ""))
    return 1 if page_mismatch or worst > args.max_diff else 0


if __name__ == "__main__":
    sys.exit(main())
