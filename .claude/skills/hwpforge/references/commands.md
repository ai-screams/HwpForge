# CLI Commands (key forms)

Run `hwpforge <command> --help` for every flag. Add `--json` for machine-readable output (on success to stdout, on failure to stderr). Edits always write to a new `-o` file.

```bash
# create / convert — presets: default, modern, classic, latest
hwpforge convert in.md -o out.hwpx --preset default
echo "# 제목" | hwpforge convert - -o out.hwpx       # stdin has no base dir: images only as data: URIs (50 MB cap)
hwpforge convert-hwp5 old.hwp -o new.hwpx
hwpforge convert-hwp5 old.hwp -o new.pdfsrc.hwpx --carry-layout-cache   # for to-pdf only, not for 한컴 re-open

# read
hwpforge outline doc.hwpx --json                    # headings, tables (ordinal, rows, cols, at), fields, bookmarks
hwpforge read doc.hwpx --section 0 --paras 1..3     # paragraph range (inclusive)
hwpforge read doc.hwpx --table 0                    # one table as a grid
hwpforge read doc.hwpx --field 과제명               # one named field
hwpforge inspect doc.hwpx --styles                  # per-section counts; drop --styles for counts only
hwpforge fields doc.hwpx --json                     # name, hint, current, section, fillable
hwpforge to-md doc.hwpx -o out.md --mode lossy      # modes: styled (default), lossy, lossless; -o ending .md = that file, else a directory

# edit
hwpforge fill doc.hwpx --set 과제명="AI 문서 자동화" --set 기관명="AiScream" -o out.hwpx   # all-or-nothing; repeat --set
hwpforge to-json doc.hwpx --section 0 -o sec.json   # -o is required; omit --section for the whole document
hwpforge patch doc.hwpx --section 0 sec.json -o out.hwpx
hwpforge to-json doc.hwpx -o full.json
hwpforge from-json full.json -o out.hwpx --base doc.hwpx       # rebuild — only under SKILL.md rule 5
hwpforge set-cell doc.hwpx --table 0 --at "1,1" --text "값" -o out.hwpx   # structural-edit.md
hwpforge insert-para doc.hwpx --section 0 --anchor 1 --text "문단" -o out.hwpx
hwpforge delete-para doc.hwpx --section 0 --index 2 -o out.hwpx
hwpforge stamp-plan doc.hwpx --json > plan.json
hwpforge stamp doc.hwpx --map specs.json -o form.hwpx          # stamp.md

# check / render
hwpforge diff doc.hwpx out.hwpx --json
hwpforge diff doc.hwpx out.hwpx --json -o report.json          # also writes the full report (top-level keys, no .diff wrapper)
hwpforge validate doc.hwpx --json                   # exit 0 valid, 1 file error, 2 not HWPX, 3 decodes but invalid
hwpforge to-pdf doc.hwpx -o doc.pdf --discovery platform       # pdf.md

# presets / schemas — schema kinds: document, exported-document, exported-section
hwpforge templates list --json
hwpforge templates show default
hwpforge schema exported-section
```

Diagnostic commands for HWP5 parity checks, not for normal authoring: `audit-hwp5`, `census-hwp5`.

JSON round-trip rules (`patch` replaces the whole section; keep `preservation`, `layout_cache` and cell `addr` as exported; reuse existing style IDs): [editing-workflow.md](editing-workflow.md). Filling a Korean template end to end: [template-fill.md](template-fill.md).
