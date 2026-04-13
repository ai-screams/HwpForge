#!/usr/bin/env python3
"""
Step 4: 코퍼스 HWPX에서 E2용 4가지 조건 입력을 자동 생성.

조건:
  A: Raw XML (HWPX unzip → section*.xml 추출)
  B: Plain text (Markdown에서 마크업 strip)
  C: Markdown (hwpforge to-md)
  D: JSON (hwpforge to-json)

Usage:
  python input_preparer.py                    # 전체 코퍼스
  python input_preparer.py --file "파일.hwpx" # 단일 파일
"""

import argparse
import json
import re
import subprocess
import zipfile
from pathlib import Path

from config import CORPUS_HWPX, EXISTING_HWPX, HWPFORGE_BIN, INPUT_DIR


def run_hwpforge(args: list[str]) -> subprocess.CompletedProcess:
    """HwpForge CLI 실행."""
    cmd = [str(HWPFORGE_BIN)] + args
    return subprocess.run(cmd, capture_output=True, text=True, timeout=120)


def prepare_condition_a(hwpx_path: Path, out_dir: Path) -> Path:
    """조건 A: Raw XML — HWPX 내부 section XML 추출."""
    out_file = out_dir / "A_raw_xml.txt"
    xml_parts = []

    with zipfile.ZipFile(hwpx_path, "r") as zf:
        section_files = sorted(
            [n for n in zf.namelist() if "section" in n.lower() and n.endswith(".xml")]
        )
        if not section_files:
            # fallback: Contents/ 하위 모든 XML
            section_files = sorted(
                [n for n in zf.namelist() if n.startswith("Contents/") and n.endswith(".xml")]
            )

        for sf in section_files:
            xml_parts.append(f"<!-- {sf} -->\n")
            xml_parts.append(zf.read(sf).decode("utf-8", errors="replace"))
            xml_parts.append("\n\n")

    content = "".join(xml_parts)
    out_file.write_text(content, encoding="utf-8")
    print(f"  A: {len(content):,} chars → {out_file.name}")
    return out_file


def prepare_condition_b(markdown_path: Path, out_dir: Path) -> Path:
    """조건 B: Plain text — Markdown에서 마크업 구문 제거."""
    md_text = markdown_path.read_text(encoding="utf-8")

    # Strip markdown syntax
    text = md_text
    text = re.sub(r"^#{1,6}\s+", "", text, flags=re.MULTILINE)  # headings
    text = re.sub(r"\*\*(.+?)\*\*", r"\1", text)  # bold
    text = re.sub(r"\*(.+?)\*", r"\1", text)  # italic
    text = re.sub(r"!\[.*?\]\(.*?\)", "", text)  # images
    text = re.sub(r"\[(.+?)\]\(.*?\)", r"\1", text)  # links
    text = re.sub(r"^[\-\*]\s+", "", text, flags=re.MULTILINE)  # list markers
    text = re.sub(r"^\d+\.\s+", "", text, flags=re.MULTILINE)  # numbered lists
    text = re.sub(r"^>\s+", "", text, flags=re.MULTILINE)  # blockquotes
    text = re.sub(r"`(.+?)`", r"\1", text)  # inline code
    text = re.sub(r"^---+$", "", text, flags=re.MULTILINE)  # horizontal rules
    # Strip HTML table tags but keep cell content
    text = re.sub(r"</?table>", "", text)
    text = re.sub(r"</?tbody>", "", text)
    text = re.sub(r"</?thead>", "", text)
    text = re.sub(r"</?tr>", "\n", text)
    text = re.sub(r"</?t[dh][^>]*>", " ", text)
    text = re.sub(r"<br\s*/?>", "\n", text)
    text = re.sub(r"</?strong>", "", text)
    text = re.sub(r"</?em>", "", text)
    text = re.sub(r"<img[^>]*>", "", text)
    # Markdown table separators
    text = re.sub(r"^\|[\s\-:|]+\|$", "", text, flags=re.MULTILINE)
    text = re.sub(r"\|", " ", text)
    # Clean up whitespace
    text = re.sub(r"\n{3,}", "\n\n", text)
    text = text.strip()

    out_file = out_dir / "B_plain_text.txt"
    out_file.write_text(text, encoding="utf-8")
    print(f"  B: {len(text):,} chars → {out_file.name}")
    return out_file


def prepare_condition_c(hwpx_path: Path, out_dir: Path) -> Path:
    """조건 C: Markdown — hwpforge to-md."""
    md_dir = out_dir / "_md_temp"
    md_dir.mkdir(exist_ok=True)

    result = run_hwpforge(["to-md", str(hwpx_path), "-o", str(md_dir)])
    if result.returncode != 0:
        print(f"  C: ERROR — {result.stderr.strip()}")
        return None

    # Find generated .md file
    md_files = list(md_dir.glob("*.md"))
    if not md_files:
        print("  C: ERROR — no .md file generated")
        return None

    md_content = md_files[0].read_text(encoding="utf-8")
    out_file = out_dir / "C_markdown.md"
    out_file.write_text(md_content, encoding="utf-8")
    print(f"  C: {len(md_content):,} chars → {out_file.name}")
    return out_file


def prepare_condition_d(hwpx_path: Path, out_dir: Path) -> Path:
    """조건 D: JSON — hwpforge to-json."""
    out_file = out_dir / "D_json.json"

    result = run_hwpforge(["to-json", str(hwpx_path), "-o", str(out_file)])
    if result.returncode != 0:
        print(f"  D: ERROR — {result.stderr.strip()}")
        return None

    size = out_file.stat().st_size
    print(f"  D: {size:,} bytes → {out_file.name}")
    return out_file


def prepare_one(hwpx_path: Path, base_out_dir: Path) -> dict:
    """하나의 HWPX 파일에 대해 4가지 조건 입력 생성."""
    doc_name = hwpx_path.stem
    out_dir = base_out_dir / doc_name
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"\n{'='*60}")
    print(f"Processing: {hwpx_path.name}")
    print(f"Output:     {out_dir}")
    print(f"{'='*60}")

    results = {"document": doc_name, "source": str(hwpx_path)}

    # C first (Markdown needed for B)
    c_path = prepare_condition_c(hwpx_path, out_dir)
    results["C_markdown"] = str(c_path) if c_path else None

    # A: Raw XML
    a_path = prepare_condition_a(hwpx_path, out_dir)
    results["A_raw_xml"] = str(a_path) if a_path else None

    # B: Plain text (from C's markdown)
    if c_path:
        b_path = prepare_condition_b(c_path, out_dir)
        results["B_plain_text"] = str(b_path) if b_path else None
    else:
        print("  B: SKIPPED (no markdown available)")
        results["B_plain_text"] = None

    # D: JSON
    d_path = prepare_condition_d(hwpx_path, out_dir)
    results["D_json"] = str(d_path) if d_path else None

    return results


def main():
    parser = argparse.ArgumentParser(description="E2 입력 조건 생성기")
    parser.add_argument("--file", type=str, help="단일 HWPX 파일 경로")
    parser.add_argument("--output", type=str, default=str(INPUT_DIR), help="출력 디렉토리")
    args = parser.parse_args()

    out_dir = Path(args.output)
    out_dir.mkdir(parents=True, exist_ok=True)

    if args.file:
        hwpx_files = [Path(args.file)]
    else:
        # 코퍼스 전체
        hwpx_files = sorted(CORPUS_HWPX.glob("*.hwpx"))
        if EXISTING_HWPX.exists():
            hwpx_files.append(EXISTING_HWPX)

    print(f"HwpForge: {HWPFORGE_BIN}")
    print(f"Files:    {len(hwpx_files)}")
    print(f"Output:   {out_dir}")

    # Verify hwpforge binary
    check = subprocess.run([str(HWPFORGE_BIN), "--version"], capture_output=True, text=True)
    if check.returncode != 0:
        print(f"ERROR: hwpforge not found at {HWPFORGE_BIN}")
        return
    print(f"Version:  {check.stdout.strip()}")

    all_results = []
    for hwpx_path in hwpx_files:
        result = prepare_one(hwpx_path, out_dir)
        all_results.append(result)

    # Save manifest
    manifest_path = out_dir / "manifest.json"
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(all_results, f, ensure_ascii=False, indent=2)

    print(f"\n{'='*60}")
    print(f"Done: {len(all_results)} documents processed")
    print(f"Manifest: {manifest_path}")

    # Summary
    success = sum(1 for r in all_results if all(r.get(k) for k in ["A_raw_xml", "B_plain_text", "C_markdown", "D_json"]))
    print(f"All 4 conditions OK: {success}/{len(all_results)}")


if __name__ == "__main__":
    main()
