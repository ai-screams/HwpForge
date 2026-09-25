---
name: hwpforge-submission-check
description: "Report-only pre-submission check of a Korean 한글 .hwpx before it is submitted or sent (제출 전 점검). Use when the user asks whether a filled form, application or report is ready to submit or send: \"제출해도 돼?\", \"이거 최종본이야?\", \"보내기 전에 확인해 줘\", \"빠진 거 없는지 봐 줘\", \"check before submitting\", \"is this final\", \"ready to send\". Finds unfilled 누름틀, leftover blanks and unchecked boxes, memos left in the file, form guidance text, and leaking document properties (author, last saved by), then gives a verdict. Never edits the file; for fixes, or to check what an edit changed (diff), use the hwpforge skill."
license: MIT OR Apache-2.0
compatibility: "Needs the hwpforge CLI 0.16.6 or later, bash, jq 1.6 or later, awk and grep. The bundled scripts/check.sh runs every check. The hwpforge Python package (0.16.6+) is optional and adds the PDF page count. Works on local files only."
metadata:
  author: ai-screams
  version: "0.3.0"
allowed-tools: "Bash(hwpforge *)"
---

# HwpForge 제출 전 점검

Checks a `.hwpx` the user is about to submit and reports what is missing or should not be sent. For a legacy `.hwp`, convert it first with the main skill (`convert-hwp5`), then check the `.hwpx`.

## Rules

1. **Report only.** Never modify, re-save or replace the input. If the user wants fixes, hand off to the main [hwpforge skill](../hwpforge/SKILL.md) after the report.
2. **Write nothing next to the input.** The script writes only into a new temp dir.
3. **HwpForge surfaces only.** Do not unzip the package or grep its XML; the script runs every check. What it cannot see is listed under 알려진 한계: report it as not checked, do not work around it.
4. **The script's last line decides completeness.** `CHECK COMPLETE` (exit 0) or `CHECK INCOMPLETE: <steps>` (exit 1). On INCOMPLETE the verdict is **점검 불완전 — 확인 필요**; name the steps and the error codes from their `exit=` lines. A PDF render failure is not in the gate; it only leaves the layout unverified.
5. **Plain Korean, no raw JSON.** The user gets the report format at the end, nothing else.

## Step 1: run the script once

```bash
bash <this skill's directory>/scripts/check.sh /path/to/doc.hwpx
```

A second argument sets the `hwpforge` binary if it is not on `PATH`. Run it through `bash` exactly like this. It prints summaries and keeps full outputs in the `scratch:` dir:

| Printed section                                                     | Meaning                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Full data in scratch             |
| ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `<step> exit=N` + codes                                             | exit status and error codes of validate, fields, plan (stamp-plan), inspect, tojson, pdf                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | `<step>.json`, `<step>.err`      |
| `-- validate`, `-- fields`                                          | structure and warning counts; every 누름틀 with `hint` and `current`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | `validate.json`, `fields.json`   |
| `-- metadata`                                                       | every non-empty property; `[risk]` marks a draft/internal/test word, a local path, or a login-style or email id in author or last_saved_by                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | `doc.json`                       |
| `-- paragraphs: N · page-number lines: K · text nodes read: X of Y` | Y = every string in the document body except known non-visible keys (ids, styles, enums, URLs, BinData paths, cross-reference targets), counting each ASCII-prefixed text node of an embedded chart's XML except `<f>`/`<formatCode>`; X = what the walk read. A mismatch (text under a key or XML element the walk does not know, or CDATA in chart XML) fails the gate. K = page-number decoration lines (`s0 page_number: - N -`), not counted in N. Gaps: see 알려진 한계                                                                                                                                                                                                                                                                                                      | `paras.txt`                      |
| `-- local paths`                                                    | local file paths in visible text, and `hidden <key>:` lines for paths in strings the walk does not show (a filled field's hint, URLs). One matcher, also used for properties. Detected: `~/x`, `/Users/x`, `/Volumes/x` (any case), `/home/x`, `/root/x`, `/tmp/x`, `/opt/x`, `/var/x`, `/mnt/x`, `/private/x` (something other than a space must follow the folder), drive letters (`C:\x`, `D:/x`), UNC (`\\server\share`), any `file:` URI (`file:///etc/passwd`, `file://server/share`). Not detected: a bare folder name (`/var/` then a space), protocol-relative `//host/x`, other system folders such as `/etc/x` outside `file:`, and paths inside `scheme://` URIs and `mailto: tel: sms: urn: data: javascript: news:`. Other schemes are not removed (see 알려진 한계) | `localpaths.txt`                 |
| `-- memos: N` + memo lines                                          | memo controls anywhere, empty memos included; text and location                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | `paras.txt`                      |
| `-- boxes`                                                          | `□ ☑ ■ √ ✓` counts over all text and every line holding one                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | `boxes.txt`                      |
| `-- plan`                                                           | stamp-plan candidates per pattern (checkbox split by marker) and every empty cell with its labels and guards                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | `plan.json`, `cells.txt`         |
| `-- guidance`, `-- ※ only`                                          | removal or example instructions; `※` notes without one                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | `guidance.txt`, `notes.txt`      |
| `-- pdf`                                                            | render result, error codes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | `pdf.json`, `pdf.err`, `doc.pdf` |

Text comes from `to-json`: plain text, tab-bearing text (`InlineText`), memo anchor text, and the visible text of every control that has one, shown as `[Kind: …]`: fields, path and date fields, cross-references, hyperlinks, 덧말, equations, and charts (title, categories, series names; the `<t>`/`<v>` text of an embedded chart's XML under any ASCII namespace prefix, entities decoded). A field with no value shows its hint, as HwpForge writes it into the body. A cross-reference shows only its display text, never its target name. A page-number decoration is its own line. Every paragraph is covered: body, tables, captions, headers, text boxes, groups, notes and memos. A location is the JSON path, for example `s0 p2 tbl r21 c0 p0 tbl r0 c1 p0` (section, paragraph, table, row, cell position in the row, cell paragraph, nested table). `tbl` is not numbered; the table number for `read --table N` is the one in `cells.txt` or `hwpforge outline`. Lines are printed in full. If the output was too long to show and was saved instead, read `boxes.txt`, `cells.txt` and `guidance.txt` completely before judging, or the verdict is 점검 불완전.

## Step 2: establish the purpose

Most checks assume the user will submit a **filled** document. If the file looks like an untouched form (dozens of blanks, nothing filled anywhere), first ask, or infer from the request, which it is. A document with only a few open items is not a blank form: list each item.

| Purpose                                                     | What to do                                                                                                                                               |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Submitting a filled application or report ("신청서를 제출") | Apply every rule below. An untouched form is one 확실한 문제 ("빈 서식 그대로") with counts per pattern; memos, properties and layout stay separate rows |
| Distributing the blank form itself                          | Skip the blank, checkbox and 누름틀 rules; check memos, guidance text, properties and layout only                                                        |
| Unclear                                                     | Report 확인 필요 ("빈 서식을 보내려는 것인지 확인"), list the counts, and ask                                                                            |

## Step 3: judge each signal

| Check                             | Signal                                                                                                                                                                                             | Severity                                                         |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Structure                         | `valid:false`, or validate exit 3                                                                                                                                                                  | 확실한 문제                                                      |
| Unfilled 누름틀                   | `current` equals `hint`, or `current` empty or missing                                                                                                                                             | 확실한 문제                                                      |
| Memo left in file                 | `-- memos` > 0 (an empty memo is still a memo)                                                                                                                                                     | 확실한 문제                                                      |
| Unchosen consent or single choice | a 동의 or single-answer line where every box is `□`                                                                                                                                                | 확실한 문제                                                      |
| Multi-select list                 | a list with no box checked                                                                                                                                                                         | 확인 필요                                                        |
| Leftover blank                    | `paren_blank`, `date_blank`, `email_at`, `seal_sign`; every `cells.txt` line                                                                                                                       | 확인 필요                                                        |
| Guidance                          | `-- guidance` lines (작성요령, 작성 예시, 삭제 후, 제출 전에 삭제, 삭제해 주세요, 지워 주세요, 삭제 바랍니다 …); a `※` line joined `++NEXT++` with the next paragraph of the same container counts | 확인 필요                                                        |
| `※` note                          | `-- ※ only` lines: usually real form content (첨부서류, 예외 조항)                                                                                                                                 | 참고                                                             |
| Risky property                    | a `[risk]` line, a title left from another document, or a note in `description`, `subject` or `keywords`                                                                                           | 확인 필요                                                        |
| Local path in text                | any `-- local paths` line: it leaks the author's computer and folder names                                                                                                                         | 확인 필요                                                        |
| Other properties                  | normal title, author, created, modified                                                                                                                                                            | 참고 (list them)                                                 |
| Layout render                     | `pdf exit=0` (with the page count if Python is available)                                                                                                                                          | 참고                                                             |
| Layout render                     | `PDF_RENDER_FAILED` (`NO_RENDERABLE_CACHE`, `MISSING_LAYOUT_CACHE`)                                                                                                                                | 확인 필요                                                        |
| Page limit                        | only if the user gave one: page count above it, or not measured                                                                                                                                    | 확인 필요                                                        |
| Visual check                      | nobody looked at the pages (in 한컴 or the PDF)                                                                                                                                                    | 확인 필요, unless the user did it or says layout does not matter |
| `LAYOUT_CACHE_DROPPED`            | warning count                                                                                                                                                                                      | 참고                                                             |

### Reading the signals correctly

- **Judge boxes line by line, never by `checkbox` totals.** stamp-plan returns `□` and `☑` both as `checkbox` (split in `-- plan`), and it does not see boxes inside tab-bearing text, so use `boxes.txt`. `☑ 동의함 □ 동의하지 않음` is chosen. `■`, `√`, `✓` also mark a choice, but a `√` inside an instruction ("√ 로 표기") is not an answer.
- **Unfilled 누름틀 comes from `fields`.** In `paras.txt` a field shows as `[Field: …]` with whatever it displays, which may be its hint.
- **Validate exit 3** means the document itself is invalid (확실한 문제), not that the check failed.
- **Cell candidates stay in the report,** even when the label looks like a heading or a sentence. `GUARDED` or `[guard …]` means stamp-plan saw a protected or special region; mention it.
- **Seal and date blanks** (`(인)`, `년 월 일`) are often filled after printing: 확인 필요.
- **Guidance text is a heuristic.** The patterns find words, not intent; the user decides. Red or blue guide text cannot be detected; tell the user to look in 한컴.
- **Properties.** `[risk]` is a hint, not a verdict. `last_saved_by` is always shown; it is usually the saving computer's account name (for example `hanyul`), so ask whether it may go out. An author that is a person's name (Korean or Latin) is normal unless it is not the submitter.
- **`LAYOUT_CACHE_DROPPED`** repeats on many 한컴-saved files: HwpForge could not reuse part of the saved line layout. Report it once with the count.

Where each property appears (0.16.6):

| Field                      | CLI `inspect` | MCP `hwpforge_inspect`   | CLI `to-json` / Python `inspect()` |
| -------------------------- | ------------- | ------------------------ | ---------------------------------- |
| title, author              | yes           | yes                      | yes                                |
| subject, keywords          | no            | yes (omitted when empty) | yes                                |
| created, modified          | no            | yes                      | yes                                |
| description, last_saved_by | no            | **no**                   | yes                                |

## Page count

The CLI reports no page count. With the Python package:

```bash
python3 -c 'from hwpforge import Document; import sys; r=Document.open(sys.argv[1]).to_pdf(discovery="platform"); print(r.report)' "$F"
```

`report["pages"]` is the count. Compare it only with a limit the user gave; without a limit, just report it. A render failure means HwpForge cannot check the layout, not that the file is broken. Rendering proves only that the saved layout replays; it does not show cut-off or overlapping text. Never call the layout correct: say whether it rendered, the page count, and that someone should look at the pages.

## 알려진 한계

아래는 스크립트가 보지 못한다. 상호 참조 항목은 항상, 나머지는 해당하면 보고의 "점검하지 못한 것"에 적는다.

- 상호 참조: 0.16.6 이하가 디코드한 HWPX 는 상호 참조의 실제 본문을 버리고 표시 문자열을 비워 두므로, 상호 참조 안의 글자는 점검하지 못하고 상호 참조가 있는지도 알 수 없다(이후 릴리스에서 수정).
- 내장 차트 XML 의 `formatCode`(숫자 형식) 안에 든 literal 문자열은 점검하지 않는다.
- ASCII 가 아닌 XML 이름공간 접두사(`<한:t>`)는 인식하지 못해 그 안의 글자를 놓친다.
- 로컬 경로: 구두점으로 끝나는 폴더 언급(`/var/,`)과 일부 URI scheme(`sftp:`, `git:`, `cid:`)을 경로로 잡을 수 있고, `file:` 밖의 시스템 경로(`/etc/x` 등)는 놓칠 수 있다.
- 내장 차트 XML 경로는 합성 입력으로만 시험했다.

## Verdict and report

| Verdict                     | When                                                                                  |
| --------------------------- | ------------------------------------------------------------------------------------- |
| **점검 불완전 — 확인 필요** | `CHECK INCOMPLETE`, or the saved lists were not fully read                            |
| **제출 불가**               | at least one 확실한 문제                                                              |
| **확인 필요**               | no 확실한 문제, at least one 확인 필요 (unverified layout and the visual check count) |
| **제출 가능**               | only 참고 items                                                                       |

Reply in Korean, in this order:

1. One line: the verdict and why ("제출 불가 — 메모 1건과 빈 누름틀 1건이 남아 있습니다").
2. A table: 심각도 · 위치 · 내용 · 고치는 방법. Severity values are exactly 확실한 문제, 확인 필요, 참고. Put 확실한 문제 first.
3. What was not checked (for example the visual check, coloured guide text, and any 알려진 한계 that applies).
4. One line: the file was not changed; the main hwpforge skill can make the fixes.

| Finding                                       | Fix path                                                                                                                  |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Unfilled 누름틀                               | `fill` with the value (main skill)                                                                                        |
| Unchecked box, blank, guidance text           | edit the text: `to-json --section N` → `patch` (main skill), or in 한컴. Not `set-cell`: it refuses 한컴-saved files      |
| Memo                                          | delete it in 한컴                                                                                                         |
| Author, title, subject, description, keywords | change them in 한컴's 문서 정보, then re-save                                                                             |
| Last saved by                                 | recorded when the file is saved, not typed by the user; ask whether that name may go out. Changing it is outside HwpForge |
| Layout, visual check                          | open in 한컴 (or the PDF) and check pages and line breaks                                                                 |
