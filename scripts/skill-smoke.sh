#!/usr/bin/env bash
# =============================================================================
# Skill smoke test — verifies that every command/workflow documented in
# .claude/skills/hwpforge/ actually works against the current CLI.
#
# This is the "make ci" for the skill: it catches documentation drift (a flag
# that changed, a workflow that no longer behaves as written) automatically.
# Run via `make skill-test`.
# =============================================================================
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0
FAIL=0
fail() { echo "  ✗ FAIL: $*"; FAIL=$((FAIL + 1)); }
pass() { echo "  ✓ $*"; PASS=$((PASS + 1)); }

# --- assertions -------------------------------------------------------------
assert_ok()       { if "$@" >/dev/null 2>&1; then pass "$*"; else fail "$* (expected exit 0)"; fi; }
assert_fail_grep() { # cmd... -- pattern : expect NON-zero exit AND stderr/out contains pattern
  local pat="${!#}"; set -- "${@:1:$(($#-2))}"
  local out; out="$("$@" 2>&1)"; local rc=$?
  if [[ $rc -ne 0 ]] && grep -qF "$pat" <<<"$out"; then pass "(expected failure) $* -> $pat"
  else fail "$* (expected nonzero exit containing '$pat'; got rc=$rc)"; fi
}
assert_file()      { if [[ -s "$1" ]]; then pass "produced $1"; else fail "missing/empty $1"; fi; }
assert_grep()      { if grep -qF "$2" "$1"; then pass "$1 contains '$2'"; else fail "$1 missing '$2'"; fi; }

echo "== Building CLI =="
cargo build -q -p hwpforge-bindings-cli || { echo "build failed"; exit 1; }
BIN="$ROOT/target/debug/hwpforge"
[[ -x "$BIN" ]] || { echo "binary not found at $BIN"; exit 1; }
cd "$WORK"

echo "== templates / schema =="
assert_ok "$BIN" templates list
assert_ok "$BIN" templates list --json
for p in default modern classic latest; do assert_ok "$BIN" templates show "$p"; done
for t in document exported-document exported-section; do assert_ok "$BIN" schema "$t"; done

echo "== convert (Markdown -> HWPX) + presets =="
cat > tpl.md <<'MD'
# 국가연구개발 과제 제안서

## 1. 연구개발 목표

(여기에 연구개발 목표를 작성하시오)

## 2. 연구비 내역

| 항목 | 1년차 | 2년차 |
| --- | --- | --- |
| 인건비 | (작성) | (작성) |
MD
assert_ok "$BIN" convert tpl.md -o tpl.hwpx
assert_file tpl.hwpx
# Documented reality: convert --preset resolves only `default` today; the other catalogued
# presets (modern/classic/latest) return UNKNOWN_PRESET. Lock both facts.
assert_ok "$BIN" convert tpl.md -o p_default.hwpx --preset default
for p in modern classic latest; do
  assert_fail_grep "$BIN" convert tpl.md -o "p_$p.hwpx" --preset "$p" -- "UNKNOWN_PRESET"
done
printf '# stdin\n\n본문.\n' | "$BIN" convert - -o stdin.hwpx >/dev/null 2>&1 && assert_file stdin.hwpx || fail "stdin convert"

echo "== inspect / to-md / to-json =="
assert_ok "$BIN" inspect tpl.hwpx
assert_ok "$BIN" inspect tpl.hwpx --json
assert_ok "$BIN" to-md tpl.hwpx -o tpl.md.out
assert_file tpl.md.out
assert_ok "$BIN" to-json tpl.hwpx -o full.json
assert_ok "$BIN" to-json tpl.hwpx --section 0 -o sec.json
assert_file sec.json
# documented contract: to-json requires -o (no stdout export)
assert_fail_grep "$BIN" to-json tpl.hwpx -- "output"

echo "== Recipe A: patch (text-only) fills body + POSITIONAL table cells =="
# Mirrors template-fill.md: body via text map, table via row-label positional fill
# (distinct values per column — a text map would write the same value to every cell).
python3 - <<'PY'
import json
d = json.load(open("sec.json"))
BODY = {"(여기에 연구개발 목표를 작성하시오)": "본 과제는 X를 목표로 한다."}
BUDGET = {"인건비": ["120,000", "130,000"]}  # label -> [1년차, 2년차]
def cell_text(cell):
    for cp in cell.get("paragraphs", []):
        for r in cp.get("runs", []):
            if "Text" in r.get("content", {}): return r["content"]["Text"]
    return ""
def set_cell(cell, v):
    for cp in cell.get("paragraphs", []):
        for r in cp.get("runs", []):
            if "Text" in r.get("content", {}): r["content"]["Text"] = v; return
for p in d["section"]["paragraphs"]:
    for r in p.get("runs", []):
        c = r.get("content", {})
        if c.get("Text") in BODY: c["Text"] = BODY[c["Text"]]
        t = c.get("Table")
        if t:
            for row in t["rows"]:
                cells = row["cells"]; cols = BUDGET.get(cell_text(cells[0]))
                if cols:
                    for i, v in enumerate(cols, start=1):
                        if i < len(cells): set_cell(cells[i], v)
json.dump(d, open("sec.json", "w"), ensure_ascii=False)
PY
assert_ok "$BIN" patch tpl.hwpx --section 0 sec.json -o A.hwpx
assert_grep <("$BIN" inspect A.hwpx 2>/dev/null) "1 tables"   # table preserved
"$BIN" to-md A.hwpx -o A.md >/dev/null 2>&1
assert_grep A.md "본 과제는 X를 목표로 한다."
assert_grep A.md "120,000"   # positional fill put DISTINCT values in the two columns
assert_grep A.md "130,000"

echo "== patch rejects structural change (negative test) =="
python3 - <<'PY'
import json
d = json.load(open("sec.json"))
ps = d["section"]["paragraphs"]; ps.append(json.loads(json.dumps(ps[-1])))
json.dump(d, open("sec_struct.json", "w"), ensure_ascii=False)
PY
assert_fail_grep "$BIN" patch tpl.hwpx --section 0 sec_struct.json -o bad.hwpx -- "structural change"

echo "== Recipe B: from-json --base adds a paragraph, preserves table =="
before=$("$BIN" inspect tpl.hwpx --json 2>/dev/null | python3 -c 'import sys,json;d=json.load(sys.stdin);print(sum(len(s.get("paragraphs",[])) if isinstance(s,dict) else 0 for s in [0]) or "?")' 2>/dev/null)
python3 - <<'PY'
import json
d = json.load(open("full.json"))
paras = d["document"]["sections"][0]["paragraphs"]
ref = paras[-1]
new = {"runs": [{"content": {"Text": "리빌드로 추가한 새 문단."},
                 "char_shape_id": ref["runs"][0]["char_shape_id"]}],
       "para_shape_id": ref["para_shape_id"], "column_break": False, "page_break": False}
if "style_id" in ref: new["style_id"] = ref["style_id"]
paras.append(new)
json.dump(d, open("full.json", "w"), ensure_ascii=False)
print("paras_after", len(paras))
PY
assert_ok "$BIN" from-json full.json -o B.hwpx --base tpl.hwpx
assert_grep <("$BIN" inspect B.hwpx 2>/dev/null) "1 tables"   # table survived rebuild
"$BIN" to-md B.hwpx -o B.md >/dev/null 2>&1
assert_grep B.md "리빌드로 추가한 새 문단."

echo "== convert-hwp5 (.hwp -> .hwpx), if a fixture exists =="
HWP_FIXTURE="$(find "$ROOT/tests/fixtures" "$ROOT/crates" -name '*.hwp' 2>/dev/null | head -1)"
if [[ -n "$HWP_FIXTURE" ]]; then
  assert_ok "$BIN" convert-hwp5 "$HWP_FIXTURE" -o from_hwp5.hwpx
  assert_file from_hwp5.hwpx
  # W4: layout-cache carry — 산출물에 linesegarray 가 실려야 한다.
  assert_ok "$BIN" convert-hwp5 "$HWP_FIXTURE" -o from_hwp5_carry.hwpx --carry-layout-cache
  assert_file from_hwp5_carry.hwpx
  if unzip -p from_hwp5_carry.hwpx Contents/section0.xml 2>/dev/null | grep -qF "<hp:linesegarray>"; then
    pass "carry output contains linesegarray"
  else
    fail "carry output missing linesegarray"
  fi
else
  echo "  (skipped — no .hwp fixture found)"
fi

echo "== to-pdf (W6a — 콘텐츠 스니핑 / fail-closed / 렌더는 폰트 있을 때만) =="
printf 'not a container' > garbage.hwpx
assert_fail_grep "$BIN" to-pdf garbage.hwpx -- "UNRECOGNIZED_FORMAT"
PDF_FIXTURE="$ROOT/tests/fixtures/pdf-rules/rules-headerfooter.hwpx"
HANCOM_TTF="/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF"
if [[ -f "$PDF_FIXTURE" && -d "$HANCOM_TTF" ]]; then
  assert_ok "$BIN" to-pdf "$PDF_FIXTURE" -o smoke.pdf --discovery hancom
  assert_file smoke.pdf
  "$BIN" --json to-pdf "$PDF_FIXTURE" -o smoke2.pdf --discovery hancom > topdf.json 2>/dev/null
  assert_grep topdf.json '"detected_format":"hwpx"'
  assert_grep topdf.json '"warning_counts"'
else
  echo "  (렌더 검사 skipped — 한컴 폰트 번들 또는 fixture 없음)"
fi

echo ""
echo "== Skill smoke summary: $PASS passed, $FAIL failed =="
[[ $FAIL -eq 0 ]]
