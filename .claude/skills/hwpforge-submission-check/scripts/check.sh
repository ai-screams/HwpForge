#!/usr/bin/env bash
# Pre-submission check for one .hwpx — report only, never writes next to the input.
# Usage: bash check.sh <doc.hwpx> [hwpforge-binary]
# Last line: "CHECK COMPLETE" (exit 0) or "CHECK INCOMPLETE: <steps>" (exit 1).
# Full outputs stay in the printed scratch dir.
F=${1:?usage: bash check.sh <doc.hwpx> [hwpforge-binary]}
HF=${2:-hwpforge}
D=$(mktemp -d) || { echo "CHECK INCOMPLETE: mktemp"; exit 1; }
echo "scratch: $D"
FAILED=""
fail() { FAILED="$FAILED $1"; }
# grep that treats "no match" (exit 1) as success; only exit >= 2 is an error.
g() { grep "$@"; [ $? -le 1 ]; }
codes() { g -o '"code":"[A-Z_]*"' "$1" | sort | uniq -c | tr -s ' \n' ' '; }
run() {
  n=$1; shift
  "$@" > "$D/$n.json" 2> "$D/$n.err"; rc=$?
  echo "$n exit=$rc $(codes "$D/$n.err")"
  return $rc
}

run validate "$HF" validate "$F" --json; rc=$?; [ $rc -eq 0 ] || [ $rc -eq 3 ] || fail validate # 3 = document invalid (a finding, not a failed check)
run fields "$HF" fields "$F" --json || fail fields
run plan "$HF" stamp-plan "$F" --json || fail plan
run inspect "$HF" inspect "$F" --json || fail inspect
run tojson "$HF" to-json "$F" -o "$D/doc.json" --json || fail tojson
run pdf "$HF" to-pdf "$F" -o "$D/doc.pdf" --discovery platform --json # failure = layout unverified, not incomplete

echo "-- validate"
jq -c '{valid, sections, errors, warnings: (.warnings // [] | group_by(.code) | map({code: .[0].code, count: length}))}' "$D/validate.json" || fail validate-read
echo "-- fields"
jq -c '.fields[] | {name, hint, current, fillable, section}' "$D/fields.json" || fail fields-read
# Shared jq library.
# Visible text (allowlist): Text, InlineText Plain, and control strings under display keys (charts: title,
# categories, series names, the <t>/<v> text of chart_xml under any prefix). hint_text (Field) is the body
# only while the field's display_text is empty (HWPX encoder section.rs Field arm): always read, shown only
# then. CrossRef contributes display_text only, never target Name/Raw: HWPX decoded by <= 0.16.6 leaves
# display_text empty and drops the real body, so the target name would be a wrong substitute.
# Nested paragraphs/runs are walked separately.
# Coverage inventory (denylist): every string leaf in the sections EXCEPT known non-visible keys, and every
# text node of chart_xml except <f>/<formatCode> (CDATA counts), so text the walk does not know makes the two
# counts differ and fails the gate.
# Local paths: one matcher for body text, hidden strings and properties; other URIs are removed first.
TEXTLIB='
  def lkey($q): [$q[] | strings] | last;
  def visible: ["Text", "Plain", "text", "display_text", "main_text", "sub_text", "compose_text", "script", "title", "categories", "chart_xml"];
  def denied: ["Control", "Image", "Name", "Object", "Other", "Raw", "Tab", "Table", "Unknown", "align", "apply_page_type", "apply_type", "arc_type", "arrow_type", "author", "background", "bar_shape", "bg_color", "bookmark_type", "border", "chart_type", "circle_type", "color", "column_type", "command", "compose_type", "connect_type", "content_type", "create_datetime", "data", "drop_cap_style", "fg_color", "field_type", "fill", "fill_area", "fill_color", "font", "font_name", "font_style", "format", "gradient_type", "grouping", "gutter_type", "head_arrow", "help_text", "horz_rel_to", "id", "image_id", "kind", "layout_mode", "legend", "line_color", "line_style", "line_type", "mode", "name", "number_format", "of_pie_type", "page_break", "path", "pattern_type", "position", "primary", "radar_style", "ref_type", "scatter_style", "secondary", "segment_types", "shape", "side", "stock_variant", "tag", "tail_arrow", "text_border", "text_color", "text_direction", "text_flow", "text_vertical_align", "text_wrap", "token", "url", "vert_align", "vert_rel_to", "vertical_align"];
  def series_name($q): lkey($q) == "name" and (($q | index(["series"])) != null);
  def fallback($q): lkey($q) == "hint_text";
  def pagedeco($q): lkey($q) == "decoration" and $q[-2] == "page_number";
  def shown($o; $q): if fallback($q) then (($o | getpath($q[:-1]) | .display_text? // "") == "")
    else (visible | index([lkey($q)])) != null or series_name($q) or pagedeco($q) end;
  def nested($q): [$q[] | select(. == "paragraphs" or . == "runs" or . == "anchor_runs" or . == "content")] | length > 0;
  def unent: gsub("&lt;"; "<") | gsub("&gt;"; ">") | gsub("&quot;"; "\"") | gsub("&apos;"; "\u0027")
    | gsub("&#[xX](?<h>[0-9A-Fa-f]+);"; [.h | ascii_downcase | explode | reduce .[] as $c (0; . * 16 + (if $c >= 97 then $c - 87 else $c - 48 end))] | implode)
    | gsub("&#(?<d>[0-9]+);"; [.d | tonumber] | implode) | gsub("&amp;"; "&");
  def xmlvis: [scan("<(?:[A-Za-z_][A-Za-z0-9_.-]*:)?(?:t|v)(?=[\\s/>])[^>]*>([^<]*)</(?:[A-Za-z_][A-Za-z0-9_.-]*:)?(?:t|v)\\s*>") | .[0] | unent | select(test("\\S"))];
  def xmlnodes: gsub("<!--[\\s\\S]*?-->"; "") | gsub("<\\?[\\s\\S]*?\\?>"; "")
    | ([scan("<!\\[CDATA\\[[\\s\\S]*?\\]\\]>")] | length) as $cd | gsub("<!\\[CDATA\\[[\\s\\S]*?\\]\\]>"; "<cdata/>")
    | $cd + (if test("^[^<]*[^<\\s]") then 1 else 0 end)
      + ([scan("<(/?)(?:[A-Za-z_][A-Za-z0-9_.-]*:)?([A-Za-z_][A-Za-z0-9_.-]*)[^>]*>([^<]*)")
          | select(.[2] | unent | test("\\S")) | .[1] as $n | select(.[0] == "/" or ((["f", "formatCode"] | index([$n])) == null))] | length);
  def vis: . as $o | [paths(type == "string") as $q | select(nested($q) | not)
    | select((visible | index([lkey($q)])) != null or series_name($q) or fallback($q))
    | ($o | getpath($q)) as $s
    | if lkey($q) == "chart_xml" then ($s | xmlvis[] | {s: ., show: true}) else {s: $s, show: shown($o; $q)} end];
  def rt: if has("Text") then .Text
    elif has("InlineText") then ([.InlineText.segments[] | if has("Plain") then .Plain elif has("Tab") then "\t" else "" end] | join(""))
    elif has("Control") then (.Control | to_entries[0] | .key as $kind | .value
      | ([.anchor_runs[]?.content | rt] | join("")) as $anchor
      | (vis | map(select(.show and .s != "") | .s) | join(" / ")) as $v
      | $anchor + (if $v != "" then "[\($kind): \($v)]" else "" end))
    else "" end;
  def nodes: if has("Text") then 1
    elif has("InlineText") then [.InlineText.segments[] | select(has("Plain"))] | length
    elif has("Control") then (.Control | to_entries[0].value | (vis | length) + ([.anchor_runs[]?.content | nodes] | add // 0))
    else 0 end;
  def inventory: . as $o | [paths(type == "string") as $q | select((denied | index([lkey($q)])) == null or series_name($q))
    | if lkey($q) == "chart_xml" then ($o | getpath($q) | xmlnodes) else 1 end] | add // 0;
  def got: ([paths(objects and has("runs")) as $p | getpath($p).runs[]?.content | nodes] | add // 0)
    + ([.document.sections[]? | .page_number | objects | .decoration | strings] | length);
  def haslocal: gsub("(?i)\\b(?!file:)[a-z][a-z0-9+.-]*://[^\\s\"<>]*"; "")
    | gsub("(?i)\\b(?:mailto|tel|sms|urn|data|javascript|news):[^\\s\"<>]*"; "")
    | test("(?i)\\bfile:/")
      or test("(^|[^A-Za-z0-9._~/-])~/[^/\\s]")
      or test("(^|[^A-Za-z0-9._~/-])/(users|volumes)/[^/\\s]"; "i")
      or test("(^|[^A-Za-z0-9._~/-])/(home|root|tmp|opt|var|mnt|private)/[^/\\s]")
      or test("(^|[^A-Za-z0-9])[a-z]:[\\\\/]+[^\\\\/\\s]"; "i")
      or test("\\\\\\\\[^\\\\\\s]+\\\\[^\\\\\\s]+");'

echo "-- metadata (all non-empty; [risk] = draft/internal/test word, local path, or login-style/email id in author or last_saved_by)"
jq -r "$TEXTLIB"'.document.metadata | to_entries[] | select(.value != null and .value != "" and .value != [] and .value != {})
  | (.value | tostring) as $v
  | "\(.key): \($v)\(if ($v | test("내부|검토|초안|테스트|임시|예시|사본"))
      or ($v | test("\\b(draft|test|tmp|temp|copy|sample|internal)\\b"; "i"))
      or ($v | haslocal)
      or ((.key == "author" or .key == "last_saved_by") and ($v | test("^[a-z0-9._-]+$|@")))
    then "  [risk]" else "" end)"' "$D/doc.json" || fail metadata

# Every paragraph (tables, captions, text boxes, groups, notes, memos, headers included) with its path and full text.
jq -r "$TEXTLIB"'
  . as $r | paths(objects and has("runs")) as $p
  | ([range(0; $p | length) as $i | $p[$i] as $s
      | if $s == "sections" then "s\($p[$i+1])"
        elif $s == "paragraphs" then "p\($p[$i+1])"
        elif $s == "rows" then "r\($p[$i+1])"
        elif $s == "cells" then "c\($p[$i+1])"
        elif $s == "Table" then "tbl"
        elif ($s | type) == "string" and ($s | test("^(document|runs|content|Control)$") | not) then (($s | ascii_downcase) + (if ($p[$i+1] | type) == "number" and $s != "paragraphs" then "\($p[$i+1])" else "" end))
        else empty end] | join(" ")) as $loc
  | "\($loc): \($r | getpath($p) | [.runs[]?.content | rt] | join("") | gsub("\n"; " / "))"' "$D/doc.json" > "$D/paras.txt" || fail paras
P=$(g -c '' "$D/paras.txt")
# Page-number decoration is printed around the number ("- 1 -"); it lives outside any paragraph.
jq -r '.document.sections | to_entries[] | .key as $i | .value.page_number | objects
  | select(.decoration != "") | "s\($i) page_number: \(.decoration) N \(.decoration)"' "$D/doc.json" >> "$D/paras.txt" || fail page-number
# Coverage: the key-agnostic inventory of the sections must equal the text nodes the walk read.
ALL=$(jq "$TEXTLIB"' .document.sections | inventory' "$D/doc.json") || fail coverage
GOT=$(jq "$TEXTLIB"' got' "$D/doc.json") || fail coverage
echo "-- paragraphs: $P · page-number lines: $(g -c ' page_number: ' "$D/paras.txt") · text nodes read: $GOT of $ALL"; [ "$ALL" = "$GOT" ] || fail text-coverage

jq -R -r "$TEXTLIB"' select(haslocal)' "$D/paras.txt" > "$D/localpaths.txt" || fail local-paths
# Hidden = a local path in a string the walk does not show (same predicate as the walk).
jq -r "$TEXTLIB"' .document.sections as $o | $o | paths(type == "string") as $q | ($o | getpath($q)) as $s | select($s | haslocal)
  | select(if lkey($q) == "chart_xml" then ($s | xmlvis | join(" ") | haslocal | not) else (shown($o; $q) | not) end)
  | "hidden \(lkey($q)): \($s)"' "$D/doc.json" >> "$D/localpaths.txt" || fail local-paths
echo "-- local paths (확인 필요): $(g -c '' "$D/localpaths.txt")"; cat "$D/localpaths.txt"

M=$(jq '[.. | objects | select(has("Memo"))] | length' "$D/doc.json") || fail memo-count
echo "-- memos: $M"
g ' memo[ :]' "$D/paras.txt" || fail memo-lines

count() { g -o "$1" "$D/paras.txt" | g -c . ; }
echo "-- boxes: □ $(count '□') · ☑ $(count '☑') · ■ $(count '■') · √ $(count '√') · ✓ $(count '✓')"
g -E '[□☑■√✓]' "$D/paras.txt" > "$D/boxes.txt" || fail boxes
echo "box lines: $(g -c '' "$D/boxes.txt")"; cat "$D/boxes.txt"

echo "-- plan"
jq -c '{candidates: ([.candidates[] | if .pattern == "checkbox" then "checkbox " + .marker else .pattern end] | group_by(.) | map({(.[0]): length}) | add // {}), cells: (.cells | length), skipped_tables: (.skipped_tables | length)}' "$D/plan.json" || fail plan-read
jq -r '.cells[] | "cell table\(.table) r\(.at.row)c\(.at.col)\(if .guarded then " GUARDED" else "" end): \(.labels | map("\(.direction)=\(.normalized)\(if .guard then " [guard \(.guard)]" else "" end)") | join(" / "))"' "$D/plan.json" > "$D/cells.txt" || fail cells
echo "cell lines: $(g -c '' "$D/cells.txt")"; cat "$D/cells.txt"

# Removal/example instructions anywhere. A ※ line is joined with the NEXT paragraph of the same container
# (same path, paragraph index + 1); a joined next line is not printed again.
awk -v G="$D/guidance.txt" -v N="$D/notes.txt" '
  BEGIN { re = "작성 ?요령|작성 ?방법|작성 ?예시|작성례|기재 ?요령|\\(예:|예시\\)|<예시>|삭제 ?후|삭제하고|삭제하[여십]|삭제해 ?주|삭제 ?바랍|삭제 ?바람|(제출|작성) ?(전|후|시)에? ?(반드시 )?(삭제|제거)|제거 ?후|제거하고|제거해 ?주|제거 ?바랍|지우고|지운 ?후|지워 ?주" }
  {
    line[NR] = $0; loc = $0; sub(/: .*/, "", loc)
    k = loc; sub(/ p[0-9]+$/, "", k); n = loc; sub(/.* p/, "", n)
    if (loc ~ / p[0-9]+$/) at[k SUBSEP n] = NR
    cont[NR] = k; idx[NR] = n + 0; hasidx[NR] = (loc ~ / p[0-9]+$/)
  }
  END {
    for (i = 1; i <= NR; i++) {
      if (used[i]) continue
      if (line[i] ~ re) { print line[i] > G; continue }
      if (index(line[i], "※") > 0) {
        j = 0; if (hasidx[i] && ((cont[i] SUBSEP (idx[i] + 1)) in at)) j = at[cont[i] SUBSEP (idx[i] + 1)]
        if (j && line[j] ~ re) { print line[i] "  ++NEXT++  " line[j] > G; used[j] = 1 }
        else print line[i] > N
      }
    }
  }' "$D/paras.txt" || fail guidance
touch "$D/guidance.txt" "$D/notes.txt"
echo "-- guidance (확인 필요): $(g -c '' "$D/guidance.txt")"; cat "$D/guidance.txt"
echo "-- ※ only (참고): $(g -c '' "$D/notes.txt")"; cat "$D/notes.txt"

echo "-- pdf"; cat "$D/pdf.json"; echo; echo "pdf error codes: $(codes "$D/pdf.err")"

if [ -z "$FAILED" ]; then echo "CHECK COMPLETE"; exit 0; fi
echo "CHECK INCOMPLETE:$FAILED"; exit 1
