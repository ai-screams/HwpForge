#!/usr/bin/env python3
"""Documentation site postprocessor (issue #190).

Adds a canonical link or a ``noindex`` robots tag to every HTML page of the
built site (mdBook output plus rustdoc under ``api/``) and a "by Ai-Scream"
home link at the end of each page's ``<main>``. Python 3 standard library only.

Usage:
    python3 scripts/site_postprocess.py book            # postprocess in place
    python3 scripts/site_postprocess.py --verify book   # check a processed tree
    python3 scripts/site_postprocess.py --self-check    # run scripts/test_site_postprocess.py

Page classes (plan 2026-09-26-issue-190-seo §2):

    mdBook   index.html / chapters     canonical  footer
             404.html                  noindex    footer
             print.html                noindex    footer (hidden in print CSS)
             toc.html                  noindex    -
    rustdoc  api/<crate>/** content    canonical  footer
             api/<crate>/** redirect   noindex    -
             api/help.html, api/settings.html, api/src/**
                                       noindex    footer

The classification is closed: any HTML file that matches none of these rows
fails the run, and so does any file whose insertion points are ambiguous.
Every file is validated before the first one is written.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tempfile
import unittest
from collections import Counter
from dataclasses import dataclass
from html.parser import HTMLParser
from pathlib import Path, PurePosixPath
from urllib.parse import quote

BASE_URL = "https://ai-scream.ai/HwpForge/"
HOME_URL = "https://ai-scream.ai/"
HOME_TEXT = "by Ai-Scream"
FOOTER = f'<p class="ai-scream-home"><a href="{HOME_URL}">{HOME_TEXT}</a></p>'
FOOTER_CLASS = "ai-scream-home"
MARKER_BODY = " ai-scream-postprocess v1 "
MARKER = f"<!--{MARKER_BODY}-->"
MARKER_PREFIX = "ai-scream-postprocess"
NOINDEX_TAG = '<meta name="robots" content="noindex">'
# rustdoc does not load theme/custom.css, so its pages carry one inline rule.
RUSTDOC_STYLE = (
    "<style>.ai-scream-home{max-width:960px;margin:2em 0 0;padding-top:1em;"
    "border-top:1px solid var(--border-color);font-size:0.875rem;text-align:right}"
    "@media print{.ai-scream-home{display:none}}</style>"
)

DEFAULT_SUMMARY = Path(__file__).resolve().parent.parent / "docs" / "SUMMARY.md"
MDBOOK_FIXED = ("404.html", "print.html", "toc.html")
RUSTDOC_AUX_FILES = ("help.html", "settings.html")
RUSTDOC_NO_HTML_DIRS = ("trait.impl", "type.impl")
CRATE_NAME = re.compile(r"[a-z0-9_]+")
CRATES_JS = re.compile(
    r'window\.ALL_CRATES = (\[[^\]\n]*\]);\n//\{"start":\d+,"fragment_lengths":\[[0-9,]*\]\}\n?'
)
VOID_TAGS = frozenset(
    ("area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
     "source", "track", "wbr")
)
REDIRECT_TAGS = frozenset(("html", "head", "meta", "title", "body", "p", "a", "script"))


class SiteError(Exception):
    """The tree does not match the closed page classification."""


@dataclass(frozen=True)
class PagePlan:
    """What one HTML file must look like after processing."""

    rel: str  # path relative to the book root, POSIX separators
    kind: str
    canonical: str | None  # None means the page gets noindex instead
    footer: bool
    rustdoc_style: bool = False


# --------------------------------------------------------------------------
# HTML scanning
# --------------------------------------------------------------------------


class PageScan(HTMLParser):
    """One pass over a page, collecting everything the checks need.

    Insertion points come from parser events (``</head>`` and ``</main>`` end
    tags as HTMLParser sees them), so the same text inside a comment or a
    script, or an upper-case end tag, is handled like a browser would.
    """

    def __init__(self, text: str) -> None:
        super().__init__(convert_charrefs=True)
        self._text = text
        self._line_starts = [0] + [m.end() for m in re.finditer("\n", text)]
        self.stack: list[str] = []
        self.in_head = False
        self.tags: set[str] = set()
        self.tag_counts: Counter[str] = Counter()
        self.head_ends: list[int] = []  # offsets of real </head> end tags
        self.main_ends: list[int] = []  # offsets of real </main> end tags
        self.comments: list[tuple[int, int, str]] = []  # (start, end, data)
        self.canonicals: list[str] = []  # inside <head>
        self.robots: list[str] = []  # inside <head>
        self.canonicals_elsewhere: list[str] = []
        self.robots_elsewhere: list[str] = []
        self.refresh_in_head: list[str] = []
        self.refresh_total = 0
        self.generators: list[str] = []
        self.rustdoc_vars_in_head: list[dict[str, str]] = []
        self.body_classes: list[set[str]] = []
        self.main_count = 0
        self.titles: list[str] = []
        self._in_title = False
        self.anchors: list[tuple[str, str]] = []  # (href, text)
        self._anchor: list[str] | None = None
        self._anchor_href = ""
        self.paragraphs: list[str] = []
        self._p_depth = 0
        self._p_text: list[str] = []
        self.scripts: list[str] = []
        self._in_script = False
        self.data_text: list[str] = []
        # footer bookkeeping
        self.footer_count = 0
        self.footer_parent_ok: list[bool] = []
        self.footer_inner_tags: list[str] = []
        self.footer_text: list[str] = []
        self._in_footer = False
        self._after_footer = False
        self.footer_last_child: bool | None = None

    # -- helpers ----------------------------------------------------------

    def _offset(self) -> int:
        line, col = self.getpos()
        return self._line_starts[line - 1] + col

    def _end_tag_offset(self, tag: str) -> int:
        at = self._offset()
        if self._text[at:at + len(tag) + 2].lower() != f"</{tag}":
            raise SiteError(f"parser position drift at </{tag}> (offset {at})")
        return at

    def _close_to(self, tag: str) -> None:
        if tag in self.stack:
            while self.stack:
                if self.stack.pop() == tag:
                    break

    def _leave_footer_tail(self, is_main_end: bool) -> None:
        if self._after_footer:
            self.footer_last_child = is_main_end
            self._after_footer = False

    # -- HTMLParser callbacks --------------------------------------------

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self._start(tag, attrs, self_closing=False)

    def handle_startendtag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self._start(tag, attrs, self_closing=True)

    def _start(self, tag: str, attrs: list[tuple[str, str | None]], self_closing: bool) -> None:
        a = {k.lower(): (v or "") for k, v in attrs}
        classes = set(a.get("class", "").split())
        self.tags.add(tag)
        self.tag_counts[tag] += 1
        self._leave_footer_tail(False)

        if self._in_footer:
            self.footer_inner_tags.append(tag)
        if FOOTER_CLASS in classes:
            self.footer_count += 1
            self.footer_parent_ok.append(
                tag == "p" and bool(self.stack) and self.stack[-1] == "main"
                and self.stack.count("main") == 1
            )
            if tag == "p":
                self._in_footer = True

        if tag == "head":
            self.in_head = True
        elif tag == "body":
            self.in_head = False
            self.body_classes.append(classes)
        elif tag == "main":
            self.main_count += 1
        elif tag == "title":
            self._in_title = True
            self.titles.append("")
        elif tag == "script":
            self._in_script = True
            self.scripts.append("")
        elif tag == "a":
            self._anchor = []
            self._anchor_href = a.get("href", "")
        elif tag == "p":
            self._p_depth += 1
            self._p_text = []
        elif tag == "link":
            if "canonical" in a.get("rel", "").lower().split():
                (self.canonicals if self.in_head else self.canonicals_elsewhere).append(a.get("href", ""))
        elif tag == "meta":
            name = a.get("name", "").lower()
            if name == "robots":
                (self.robots if self.in_head else self.robots_elsewhere).append(a.get("content", ""))
            elif name == "generator":
                self.generators.append(a.get("content", ""))
            elif name == "rustdoc-vars" and self.in_head:
                self.rustdoc_vars_in_head.append(a)
            if a.get("http-equiv", "").lower() == "refresh":
                self.refresh_total += 1
                if self.in_head:
                    self.refresh_in_head.append(a.get("content", ""))

        if tag not in VOID_TAGS and not self_closing:
            self.stack.append(tag)

    def _end_p(self) -> None:
        if self._p_depth:
            self._p_depth -= 1
            self.paragraphs.append("".join(self._p_text))
        if self._in_footer:
            self._in_footer = False
            self._after_footer = True
        self._close_to("p")

    def handle_endtag(self, tag: str) -> None:
        if tag == "p":
            self._end_p()
            return
        self._leave_footer_tail(tag == "main")
        if tag == "head":
            self.head_ends.append(self._end_tag_offset(tag))
            self.in_head = False
        elif tag == "main":
            self.main_ends.append(self._end_tag_offset(tag))
        elif tag == "title":
            self._in_title = False
        elif tag == "script":
            self._in_script = False
        elif tag == "a" and self._anchor is not None:
            self.anchors.append((self._anchor_href, "".join(self._anchor)))
            self._anchor = None
        self._close_to(tag)

    def handle_data(self, data: str) -> None:
        if self._after_footer and data.strip():
            self.footer_last_child = False
            self._after_footer = False
        if self._in_script:
            self.scripts[-1] += data
            return
        if self._in_title:
            self.titles[-1] += data
        if self._anchor is not None:
            self._anchor.append(data)
        if self._p_depth:
            self._p_text.append(data)
        if self._in_footer:
            self.footer_text.append(data)
        self.data_text.append(data)


    def handle_comment(self, data: str) -> None:
        start = self._offset()
        self.comments.append((start, start + len(data) + 7, data))

    def handle_decl(self, decl: str) -> None:
        self._leave_footer_tail(False)


def scan(text: str) -> PageScan:
    parser = PageScan(text)
    parser.feed(text)
    parser.close()
    return parser


# --------------------------------------------------------------------------
# Classification
# --------------------------------------------------------------------------


def url_for(rel: str) -> str:
    """Canonical URL for a path relative to the book root (rule 6)."""
    segments = rel.split("/")
    for seg in segments:
        if seg in ("", ".", ".."):
            raise SiteError(f"{rel}: empty or dot path segment")
    return BASE_URL + "/".join(quote(seg, safe="") for seg in segments)


SUMMARY_LINK = re.compile(r"(?:[-*]\s+)?\[[^\]]*\]\(([^)]*)\)")
SUMMARY_SEPARATOR = re.compile(r"-{3,}")


def summary_chapters(summary: str) -> list[str]:
    """Output HTML paths of the non-draft chapters, in mdBook order.

    Mirrors mdBook 0.4.52 (probed against the real binary): part titles,
    separators and drafts make no page; a chapter whose file stem is README
    (any case, extension optional) renders to ``index.html`` in its directory.
    """
    outputs: list[str] = []
    for lineno, raw in enumerate(summary.splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#") or SUMMARY_SEPARATOR.fullmatch(line):
            continue
        match = SUMMARY_LINK.fullmatch(line)
        if match is None:
            raise SiteError(f"SUMMARY.md:{lineno}: unsupported line {raw!r}")
        target = match.group(1).strip()
        if not target:
            continue  # draft chapter
        path = PurePosixPath(target)
        if path.is_absolute() or ".." in path.parts or "#" in target:
            raise SiteError(f"SUMMARY.md:{lineno}: unsupported target {target!r}")
        if path.stem.lower() == "readme":
            out = path.parent / "index.html"
        else:
            out = path.with_suffix(".html")
        rel = out.as_posix()
        if rel in outputs:
            raise SiteError(f"SUMMARY.md:{lineno}: {rel} is produced twice")
        outputs.append(rel)
    if not outputs:
        raise SiteError("SUMMARY.md: no chapters")
    return outputs


def mdbook_plans(book: Path, summary: str) -> dict[str, PagePlan]:
    chapters = summary_chapters(summary)
    plans: dict[str, PagePlan] = {
        "index.html": PagePlan("index.html", "mdbook-index", BASE_URL, footer=True)
    }
    for i, rel in enumerate(chapters):
        if rel == "index.html":
            continue
        # The first chapter is also copied to index.html; its own output
        # points at the root so the two copies share one canonical.
        canonical = BASE_URL if i == 0 else url_for(rel)
        plans[rel] = PagePlan(rel, "mdbook-chapter", canonical, footer=True)
    plans["404.html"] = PagePlan("404.html", "mdbook-404", None, footer=True)
    plans["print.html"] = PagePlan("print.html", "mdbook-print", None, footer=True)
    plans["toc.html"] = PagePlan("toc.html", "mdbook-toc", None, footer=False)
    for rel in plans:
        url_for(rel)
        if not (book / rel).is_file():
            raise SiteError(f"{rel}: expected mdBook output is missing")
    return plans


def parse_crates_js(text: str) -> list[str]:
    match = CRATES_JS.fullmatch(text)
    if match is None:
        raise SiteError("api/crates.js: unknown format (expected window.ALL_CRATES = [...]; + //{...})")
    crates = json.loads(match.group(1))
    if not isinstance(crates, list) or not crates:
        raise SiteError("api/crates.js: crate list is empty or not a list")
    for name in crates:
        if not isinstance(name, str) or not CRATE_NAME.fullmatch(name):
            raise SiteError(f"api/crates.js: bad crate name {name!r}")
    if len(set(crates)) != len(crates):
        raise SiteError("api/crates.js: duplicate crate name")
    return crates


def is_crate_root(page: PageScan, dirname: str) -> bool:
    """Crate-root signature (plan §2.3-1)."""
    return (
        page.generators == ["rustdoc"]
        and len(page.body_classes) == 1
        and {"rustdoc", "mod", "crate"} <= page.body_classes[0]
        and len(page.rustdoc_vars_in_head) == 1
        and page.rustdoc_vars_in_head[0].get("data-current-crate") == dirname
        and page.main_count == 1
    )


def redirect_target(page: PageScan) -> str | None:
    """Target T when the page is a rustdoc redirect, None when it has no redirect trait.

    Raises when only part of the signature is present.
    """
    hinted = (
        page.refresh_total > 0
        or any(t.strip() == "Redirection" for t in page.titles)
        or any("location.replace" in s for s in page.scripts)
        or "Redirecting to" in "".join(page.data_text)
    )
    if not hinted:
        return None
    problems: list[str] = []
    target = ""
    if page.refresh_total != 1 or len(page.refresh_in_head) != 1:
        problems.append("needs exactly one <meta http-equiv=refresh> in <head>")
    else:
        m = re.fullmatch(r"0;URL=(.+)", page.refresh_in_head[0])
        if m is None:
            problems.append("refresh content is not 0;URL=T")
        else:
            target = m.group(1)
    if page.titles != ["Redirection"]:
        problems.append("needs exactly one <title> whose text is Redirection")
    if page.tag_counts["head"] != 1 or page.tag_counts["body"] != 1:
        problems.append("needs exactly one <head> and one <body>")
    if page.anchors != [(target, target)]:
        problems.append("needs one <a href=T>T</a>")
    if page.paragraphs != [f"Redirecting to {target}..."]:
        problems.append("needs one <p>Redirecting to T...</p>")
    expected_js = f'location.replace("{target}" + location.search + location.hash);'
    if [s.strip() for s in page.scripts] != [expected_js]:
        problems.append("needs location.replace(\"T\" + location.search + location.hash)")
    if not page.tags <= REDIRECT_TAGS:
        problems.append(f"unexpected elements {sorted(page.tags - REDIRECT_TAGS)}")
    if problems:
        raise SiteError("partial redirect signature: " + "; ".join(problems))
    return target


def rustdoc_plans(book: Path, texts: dict[str, str]) -> dict[str, PagePlan]:
    api = book / "api"
    if not api.is_dir():
        raise SiteError("api/: rustdoc output is missing")
    if (api / "index.html").exists():
        raise SiteError("api/index.html exists; the rustdoc root must not have an index page")
    crates = parse_crates_js((api / "crates.js").read_text(encoding="utf-8"))

    roots: set[str] = set()
    for entry in sorted(api.iterdir()):
        root_index = entry / "index.html"
        if entry.is_dir() and root_index.is_file():
            rel = f"api/{entry.name}/index.html"
            if is_crate_root(scan(texts[rel]), entry.name):
                roots.add(entry.name)
    if roots != set(crates):
        raise SiteError(
            f"api/crates.js {sorted(crates)} != crate-root pages {sorted(roots)}"
        )

    plans: dict[str, PagePlan] = {}
    for rel in sorted(r for r in texts if r.startswith("api/")):
        parts = rel.split("/")[1:]
        top = parts[0]
        if len(parts) == 1:
            if top not in RUSTDOC_AUX_FILES:
                raise SiteError(f"{rel}: unknown top-level rustdoc page")
            plans[rel] = PagePlan(rel, "rustdoc-aux", None, footer=True, rustdoc_style=True)
        elif top in RUSTDOC_NO_HTML_DIRS:
            raise SiteError(f"{rel}: {top}/ must not contain HTML")
        elif top == "src":
            plans[rel] = PagePlan(rel, "rustdoc-aux", None, footer=True, rustdoc_style=True)
        elif top in roots:
            page = scan(texts[rel])
            try:
                target = redirect_target(page)
            except SiteError as err:
                raise SiteError(f"{rel}: {err}") from None
            if target is not None:
                plans[rel] = PagePlan(rel, "rustdoc-redirect", None, footer=False)
            elif page.main_count == 1:
                plans[rel] = PagePlan(
                    rel, "rustdoc-content", url_for(rel), footer=True, rustdoc_style=True
                )
            else:
                raise SiteError(f"{rel}: neither a redirect nor a page with one <main>")
        else:
            raise SiteError(f"{rel}: outside every known rustdoc directory")
    return plans


def read_tree(book: Path) -> dict[str, str]:
    texts: dict[str, str] = {}
    for path in sorted(book.rglob("*.html")):
        rel = path.relative_to(book).as_posix()
        texts[rel] = path.read_bytes().decode("utf-8")
    return texts


def classify(book: Path, summary: str) -> tuple[dict[str, str], dict[str, PagePlan]]:
    if not book.is_dir():
        raise SiteError(f"{book}: not a directory")
    texts = read_tree(book)
    plans = mdbook_plans(book, summary)
    plans.update(rustdoc_plans(book, texts))
    unknown = sorted(set(texts) - set(plans))
    if unknown:
        raise SiteError(f"HTML outside the page classification: {unknown[:5]}")
    return texts, plans


# --------------------------------------------------------------------------
# Per-page transformation and verification
# --------------------------------------------------------------------------


def only(rel: str, offsets: list[int], what: str) -> int:
    if len(offsets) != 1:
        raise SiteError(f"{rel}: expected exactly one {what}, found {len(offsets)}")
    return offsets[0]


def marker_state(rel: str, page: PageScan, head_end: int) -> bool:
    """True when the page is already processed (rule 4)."""
    markers = [c for c in page.comments if c[2].strip().startswith(MARKER_PREFIX)]
    if not markers:
        return False
    if len(markers) == 1 and markers[0][2] == MARKER_BODY and markers[0][1] == head_end:
        return True
    raise SiteError(f"{rel}: postprocess marker has another version or position")


def check_head_tags(rel: str, page: PageScan) -> None:
    if page.canonicals_elsewhere or page.robots_elsewhere:
        raise SiteError(f"{rel}: canonical or robots tag outside <head>")


def transform(plan: PagePlan, text: str) -> str:
    """Return the processed text, or the input unchanged when already processed."""
    rel = plan.rel
    page = scan(text)
    head_end = only(rel, page.head_ends, "</head>")
    if marker_state(rel, page, head_end):
        verify_page(plan, text)
        return text
    check_head_tags(rel, page)
    head_tags = ""
    if plan.canonical is not None:
        if page.robots:
            raise SiteError(f"{rel}: canonical page already has a robots tag")
        if len(page.canonicals) > 1 or (page.canonicals and page.canonicals[0] != plan.canonical):
            raise SiteError(f"{rel}: conflicting canonical {page.canonicals}")
        if not page.canonicals:
            head_tags += f'<link rel="canonical" href="{plan.canonical}">'
    else:
        if page.canonicals:
            raise SiteError(f"{rel}: noindex page already has a canonical link")
        if len(page.robots) > 1 or (page.robots and page.robots[0] != "noindex"):
            raise SiteError(f"{rel}: conflicting robots {page.robots}")
        if not page.robots:
            head_tags += NOINDEX_TAG
    if page.footer_count:
        raise SiteError(f"{rel}: unprocessed page already has a home link")
    if plan.rustdoc_style:
        head_tags += RUSTDOC_STYLE
    head_tags += MARKER

    if plan.footer:
        if page.main_count != 1:
            raise SiteError(f"{rel}: expected one <main>, found {page.main_count}")
        main_end = only(rel, page.main_ends, "</main>")
        if main_end < head_end:
            raise SiteError(f"{rel}: </main> comes before </head>")
        text = text[:main_end] + FOOTER + text[main_end:]
    return text[:head_end] + head_tags + text[head_end:]


def verify_page(plan: PagePlan, text: str) -> None:
    rel = plan.rel
    page = scan(text)
    head_end = only(rel, page.head_ends, "</head>")
    if not marker_state(rel, page, head_end):
        raise SiteError(f"{rel}: not processed (marker missing)")
    check_head_tags(rel, page)
    if plan.canonical is not None:
        if page.canonicals != [plan.canonical] or page.robots:
            raise SiteError(f"{rel}: want canonical {plan.canonical} and no robots, "
                            f"got {page.canonicals} / {page.robots}")
    elif page.robots != ["noindex"] or page.canonicals:
        raise SiteError(f"{rel}: want one noindex and no canonical, got {page.robots} / {page.canonicals}")
    if plan.rustdoc_style and text.count(RUSTDOC_STYLE) != 1:
        raise SiteError(f"{rel}: rustdoc home-link style missing")
    if not plan.footer:
        if page.footer_count:
            raise SiteError(f"{rel}: page class must not carry a home link")
        return
    problems: list[str] = []
    if page.main_count != 1:
        problems.append(f"{page.main_count} <main>")
    if page.footer_count != 1:
        problems.append(f"{page.footer_count} home links")
    elif not page.footer_parent_ok[0]:
        problems.append("home link is not a <p> child of the single <main>")
    elif page.footer_last_child is not True:
        problems.append("home link is not the last element of <main>")
    elif page.footer_inner_tags != ["a"] or [a for a in page.anchors if a[1] == HOME_TEXT] != [(HOME_URL, HOME_TEXT)]:
        problems.append("home link must hold exactly one <a href=HOME>by Ai-Scream</a>")
    elif "".join(page.footer_text) != HOME_TEXT:
        problems.append("home link text differs")
    if problems:
        raise SiteError(f"{rel}: " + "; ".join(problems))


def write_atomic(path: Path, data: bytes) -> None:
    """Rule 5: temp file in the same directory, same mode, then os.replace."""
    mode = path.stat().st_mode & 0o7777
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.")
    try:
        with os.fdopen(fd, "wb") as fh:
            fh.write(data)
        os.chmod(tmp, mode)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def postprocess(book: Path, summary: str) -> int:
    """Validate every page, then write the changed ones. Returns the write count."""
    texts, plans = classify(book, summary)
    pending: list[tuple[Path, bytes]] = []
    for rel in sorted(plans):
        new = transform(plans[rel], texts[rel])
        verify_page(plans[rel], new)
        if new != texts[rel]:
            pending.append((book / rel, new.encode("utf-8")))
    for path, data in pending:
        write_atomic(path, data)
    return len(pending)


def verify(book: Path, summary: str) -> dict[str, int]:
    texts, plans = classify(book, summary)
    counts: dict[str, int] = {}
    for rel in sorted(plans):
        verify_page(plans[rel], texts[rel])
        counts[plans[rel].kind] = counts.get(plans[rel].kind, 0) + 1
    return counts


def self_check() -> int:
    """Run the unittest suite in scripts/test_site_postprocess.py."""
    here = str(Path(__file__).resolve().parent)
    if here not in sys.path:
        sys.path.insert(0, here)
    import test_site_postprocess  # the test module lives next to this file

    suite = unittest.defaultTestLoader.loadTestsFromModule(test_site_postprocess)
    result = unittest.TextTestRunner(stream=sys.stdout, verbosity=2).run(suite)
    return 0 if result.wasSuccessful() else 1


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("book", nargs="?", type=Path, help="built site directory (mdBook build-dir)")
    parser.add_argument("--verify", action="store_true", help="check a processed tree, write nothing")
    parser.add_argument("--self-check", action="store_true", help="run scripts/test_site_postprocess.py")
    parser.add_argument("--summary", type=Path, default=DEFAULT_SUMMARY, help="mdBook SUMMARY.md")
    args = parser.parse_args(argv)
    if args.self_check:
        if args.book is not None or args.verify:
            parser.error("--self-check takes no other arguments")
        return self_check()
    if args.book is None:
        parser.error("book directory is required")
    summary = args.summary.read_text(encoding="utf-8")
    try:
        if args.verify:
            counts = verify(args.book, summary)
            detail = ", ".join(f"{k}={v}" for k, v in sorted(counts.items()))
            print(f"site_postprocess: verified {sum(counts.values())} pages ({detail})")
        else:
            written = postprocess(args.book, summary)
            print(f"site_postprocess: wrote {written} pages")
    except SiteError as err:
        print(f"site_postprocess: error: {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
