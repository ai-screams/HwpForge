#!/usr/bin/env python3
"""Documentation site postprocessor (issue #190).

Adds a canonical link or a ``noindex`` robots tag to every HTML page of the
built site (mdBook output plus rustdoc under ``api/``) and a "by Ai-Scream"
home link at the end of each page's ``<main>``. Python 3 standard library only.

Usage:
    python3 scripts/site_postprocess.py book            # postprocess in place
    python3 scripts/site_postprocess.py --verify book   # check a processed tree
    python3 scripts/site_postprocess.py --self-check    # synthetic-tree tests

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
import shutil
import sys
import tempfile
from collections.abc import Callable
from dataclasses import dataclass
from html.parser import HTMLParser
from pathlib import Path, PurePosixPath
from urllib.parse import quote

BASE_URL = "https://ai-scream.ai/HwpForge/"
HOME_URL = "https://ai-scream.ai/"
HOME_TEXT = "by Ai-Scream"
FOOTER = f'<p class="ai-scream-home"><a href="{HOME_URL}">{HOME_TEXT}</a></p>'
FOOTER_CLASS = "ai-scream-home"
MARKER = "<!-- ai-scream-postprocess v1 -->"
MARKER_PREFIX = "<!-- ai-scream-postprocess"
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
SAFE_SEGMENT = re.compile(r"[A-Za-z0-9._-]+")
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
    """One pass over a page, collecting everything the checks need."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.stack: list[str] = []
        self.in_head = False
        self.tags: set[str] = set()
        self.canonicals: list[str] = []
        self.robots: list[str] = []
        self.refresh_in_head: list[str] = []
        self.refresh_total = 0
        self.generators: list[str] = []
        self.rustdoc_vars_in_head: list[dict[str, str]] = []
        self.body_classes: list[set[str]] = []
        self.main_count = 0
        self.title = ""
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
                self.canonicals.append(a.get("href", ""))
        elif tag == "meta":
            name = a.get("name", "").lower()
            if name == "robots":
                self.robots.append(a.get("content", ""))
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
            self.in_head = False
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
            self.title += data
        if self._anchor is not None:
            self._anchor.append(data)
        if self._p_depth:
            self._p_text.append(data)
        if self._in_footer:
            self.footer_text.append(data)
        self.data_text.append(data)


def scan(text: str) -> PageScan:
    parser = PageScan()
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
        if not SAFE_SEGMENT.fullmatch(seg) or seg in (".", ".."):
            raise SiteError(f"{rel}: path segment {seg!r} is outside [A-Za-z0-9._-]")
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
        or page.title.strip() == "Redirection"
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
    if page.title != "Redirection":
        problems.append("title is not Redirection")
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


def marker_state(rel: str, text: str) -> bool:
    """True when the page is already processed (rule 4)."""
    count = text.count(MARKER_PREFIX)
    if count == 0:
        return False
    head_end = text.find("</head>")
    if count == 1 and head_end >= 0 and text[head_end - len(MARKER):head_end] == MARKER:
        return True
    raise SiteError(f"{rel}: postprocess marker has another version or position")


def single(rel: str, text: str, needle: str) -> int:
    if text.count(needle) != 1:
        raise SiteError(f"{rel}: expected exactly one {needle}, found {text.count(needle)}")
    return text.index(needle)


def transform(plan: PagePlan, text: str) -> str:
    """Return the processed text, or the input unchanged when already processed."""
    rel = plan.rel
    head_end = single(rel, text, "</head>")
    if marker_state(rel, text):
        verify_page(plan, text)
        return text
    page = scan(text)
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
        main_end = single(rel, text, "</main>")
        if page.main_count != 1:
            raise SiteError(f"{rel}: expected one <main>, found {page.main_count}")
        text = text[:main_end] + FOOTER + text[main_end:]
    return text[:head_end] + head_tags + text[head_end:]


def verify_page(plan: PagePlan, text: str) -> None:
    rel = plan.rel
    single(rel, text, "</head>")
    if not marker_state(rel, text):
        raise SiteError(f"{rel}: not processed (marker missing)")
    page = scan(text)
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


# --------------------------------------------------------------------------
# Self-check: synthetic trees for every table row and rule
# --------------------------------------------------------------------------


# The stray </span></a> make the element stack tolerate unmatched end tags.
def md_page(extra_head: str = "", body: str = "<h1>T</h1>\n<p>body</p></span></a>", main: bool = True) -> str:
    content = f"<main>\n{body}\n</main>" if main else body
    return (
        '<!DOCTYPE HTML>\n<html lang="ko" class="light">\n    <head>\n'
        f'        <meta charset="UTF-8">\n        <title>t</title>{extra_head}\n    </head>\n'
        '    <body>\n    <div class="page"><div id="content" class="content">\n'
        f"                    {content}\n"
        '                    <nav class="nav-wrapper"></nav>\n    </div></div>\n    </body>\n</html>\n'
    )


def rd_page(crate: str, body_class: str, vars_tag: str | None = None, root: str = "../") -> str:
    if vars_tag is None:
        vars_tag = f'<meta name="rustdoc-vars" data-root-path="{root}" data-current-crate="{crate}" >'
    return (
        '<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">'
        '<meta name="generator" content="rustdoc"><title>x - Rust</title>'
        f'{vars_tag}<script defer src="{root}crates.js"></script></head>'
        f'<body class="{body_class}"><nav class="sidebar"><a href="#">x</a></nav>'
        '<main><div class="width-limiter"><section id="main-content" class="content">'
        "<p>doc <wbr>text</p><ul><li>one</li></ul></section></div></main></body></html>"
    )


def rd_redirect(target: str, *, href: str | None = None, text: str | None = None,
                js: str | None = None, title: str = "Redirection", script: bool = True) -> str:
    js_line = (
        f'    <script>location.replace("{js if js is not None else target}"'
        " + location.search + location.hash);</script>\n" if script else ""
    )
    return (
        '<!DOCTYPE html>\n<html lang="en">\n<head>\n'
        f'    <meta http-equiv="refresh" content="0;URL={target}">\n'
        f"    <title>{title}</title>\n</head>\n<body>\n"
        f'    <p>Redirecting to <a href="{href if href is not None else target}">'
        f"{text if text is not None else target}</a>...</p>\n{js_line}</body>\n</html>"
    )


STANDARD_SUMMARY = (
    "# Summary\n\n[소개](README.md)\n\n---\n\n# 시작하기\n\n"
    "- [A](guide/a.md)\n  - [B](guide/b/README.md)\n\n---\n\n[변경 이력](CHANGELOG.md)\n"
)


def crates_js(names: list[str]) -> str:
    listing = json.dumps(names, separators=(",", ":"))
    lengths = ",".join(str(len(n) + 3) for n in names)
    return f'window.ALL_CRATES = {listing};\n//{{"start":21,"fragment_lengths":[{lengths}]}}'


def build_tree(root: Path, summary: str = STANDARD_SUMMARY,
               md_files: dict[str, str] | None = None, api: bool = True) -> Path:
    book = root / "book"
    if md_files is None:
        md_files = {
            "index.html": md_page(),
            "guide/a.html": md_page(),
            "guide/b/index.html": md_page(),
            "CHANGELOG.html": md_page(),
        }
    files = dict(md_files)
    files.setdefault("404.html", md_page('\n        <base href="/HwpForge/">'))
    files.setdefault("print.html", md_page('\n        <meta name="robots" content="noindex">'))
    files.setdefault("toc.html", md_page('\n        <meta name="robots" content="noindex">',
                                         body="<ol><li>x</li></ol>", main=False))
    if api:
        files.update({
            "api/crates.js": crates_js(["alpha", "beta_two"]),
            "api/alpha/index.html": rd_page("alpha", "rustdoc mod crate"),
            "api/alpha/struct.X.html": rd_page("alpha", "rustdoc struct"),
            "api/alpha/old/struct.X.html": rd_redirect("../../alpha/struct.X.html"),
            "api/beta_two/index.html": rd_page("beta_two", "rustdoc mod crate"),
            "api/help.html": rd_page("beta_two", "rustdoc mod sys", root="./"),
            "api/settings.html": rd_page("beta_two", "rustdoc mod sys", root="./"),
            "api/src/alpha/lib.rs.html": rd_page("alpha", "rustdoc src", root="../../"),
            "api/static.files/rustdoc.css": "main{}",
            "api/trait.impl/alpha/trait.T.js": "x",
        })
    for rel, text in files.items():
        path = book / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(text.encode("utf-8"))
    (root / "SUMMARY.md").write_text(summary, encoding="utf-8")
    return book


def snapshot(book: Path) -> dict[str, tuple[bytes, int]]:
    return {
        p.relative_to(book).as_posix(): (p.read_bytes(), p.stat().st_mode)
        for p in sorted(book.rglob("*")) if p.is_file()
    }


CheckFn = Callable[[Path], None]
CASES: list[tuple[str, CheckFn]] = []


def case(name: str) -> Callable[[CheckFn], CheckFn]:
    """Register a self-check case under ``name``."""

    def register(fn: CheckFn) -> CheckFn:
        CASES.append((name, fn))
        return fn

    return register


def expect_error(fn: Callable[[], object], fragment: str) -> None:
    try:
        fn()
    except SiteError as err:
        if fragment not in str(err):
            raise AssertionError(f"error {str(err)!r} lacks {fragment!r}") from None
        return
    raise AssertionError(f"expected SiteError containing {fragment!r}")


def run_ok(root: Path, summary: str = STANDARD_SUMMARY,
           md_files: dict[str, str] | None = None) -> tuple[Path, dict[str, str]]:
    book = build_tree(root, summary, md_files)
    postprocess(book, summary)
    verify(book, summary)
    return book, read_tree(book)


def run_err(root: Path, fragment: str, summary: str = STANDARD_SUMMARY,
            md_files: dict[str, str] | None = None, api: bool = True) -> None:
    book = build_tree(root, summary, md_files, api)
    expect_error(lambda: postprocess(book, summary), fragment)


def canon(text: str) -> list[str]:
    """Independent of PageScan: plain-string view of the inserted head tags."""
    return re.findall(r'<link rel="canonical" href="([^"]*)">', text)


def footer_is_last_in_main(text: str) -> bool:
    i = text.find(FOOTER)
    return text.count(FOOTER) == 1 and text[i + len(FOOTER):].lstrip().startswith("</main>")


def assert_row(text: str, canonical: str | None, footer: bool) -> None:
    if canonical is None:
        assert canon(text) == [], canon(text)
        assert text.count(NOINDEX_TAG) == 1, "noindex count"
    else:
        assert canon(text) == [canonical], canon(text)
        assert "robots" not in text, "robots on canonical page"
    assert footer_is_last_in_main(text) if footer else FOOTER_CLASS not in text.split("</head>")[1]
    head = text.split("</head>")[0]
    assert head.endswith(MARKER), "marker not directly before </head>"


# §2.1 rows ------------------------------------------------------------------
# 이것을 실패시키는 것: mdbook_plans 의 canonical·footer 값, transform 의 삽입 위치
@case("mdbook_rows")
def mdbook_rows(tmp: Path) -> None:
    _, t = run_ok(tmp)
    assert_row(t["index.html"], BASE_URL, footer=True)
    assert_row(t["guide/a.html"], BASE_URL + "guide/a.html", footer=True)
    assert_row(t["guide/b/index.html"], BASE_URL + "guide/b/index.html", footer=True)
    assert_row(t["CHANGELOG.html"], BASE_URL + "CHANGELOG.html", footer=True)
    assert_row(t["404.html"], None, footer=True)
    assert_row(t["print.html"], None, footer=True)  # existing noindex kept, not doubled
    assert_row(t["toc.html"], None, footer=False)
    assert "api/" not in t["index.html"]


# 이것을 실패시키는 것: 첫 장 원본의 canonical 을 자기 주소로 두는 것
@case("mdbook_first_chapter_not_readme")
def mdbook_first_chapter_not_readme(tmp: Path) -> None:
    summary = "# Summary\n\n[Intro](intro.md)\n\n- [A](a.md)\n"
    files = {"index.html": md_page(), "intro.html": md_page(), "a.html": md_page()}
    _, t = run_ok(tmp, summary=summary, md_files=files)
    assert_row(t["intro.html"], BASE_URL, footer=True)
    assert_row(t["index.html"], BASE_URL, footer=True)
    assert_row(t["a.html"], BASE_URL + "a.html", footer=True)


# §2.2 rows ------------------------------------------------------------------
# 이것을 실패시키는 것: rustdoc_plans 의 분류(콘텐츠·리다이렉트·보조)
@case("rustdoc_rows")
def rustdoc_rows(tmp: Path) -> None:
    _, t = run_ok(tmp)
    for rel in ("api/alpha/index.html", "api/alpha/struct.X.html", "api/beta_two/index.html"):
        assert_row(t[rel], BASE_URL + rel, footer=True)
        assert t[rel].count(RUSTDOC_STYLE) == 1
    for rel in ("api/help.html", "api/settings.html", "api/src/alpha/lib.rs.html"):
        assert_row(t[rel], None, footer=True)
        assert t[rel].count(RUSTDOC_STYLE) == 1
    red = t["api/alpha/old/struct.X.html"]
    assert_row(red, None, footer=False)
    assert RUSTDOC_STYLE not in red


# §2.3-1 SUMMARY traversal (expected lists = real mdBook 0.4.52 output) ------
# 이것을 실패시키는 것: 초안·구분선·part title 처리, README→index.html 규칙
@case("summary_traversal")
def summary_traversal(tmp: Path) -> None:
    cases = {
        "# S\n\n[Draft]()\n\n- [A](a.md)\n- [B](b.md)": ["a.html", "b.html"],
        "# S\n\n---\n\n- [A](a.md)": ["a.html"],
        "# S\n\n[I](intro.md)\n\n- [G](g/README.md)\n  - [H](g/h.md)":
            ["intro.html", "g/index.html", "g/h.html"],
        "# S\n\n- [G](nested/README.md)\n- [A](a.md)": ["nested/index.html", "a.html"],
        "# S\n\n[I](intro.md)\n\n- [A](a.md)": ["intro.html", "a.html"],
        "# S\n\n[I](intro.md)\n\n- [R](sub/README)": ["intro.html", "sub/index.html"],
        "# S\n\n[I](intro.md)\n\n- [R](sub/readme.md)": ["intro.html", "sub/index.html"],
        "# S\n\n# Part\n\n- [A](a.md)\n\n---\n\n[S](s.md)": ["a.html", "s.html"],
        "# S\n\n- [D]()\n  - [A](a.md)": ["a.html"],
        "# S\n\n[R](README)": ["index.html"],
    }
    for text, want in cases.items():
        got = summary_chapters(text)
        assert got == want, (text, got)
    expect_error(lambda: summary_chapters("# S\n\n* weird line"), "unsupported line")
    expect_error(lambda: summary_chapters("# S\n\n- [A](../a.md)"), "unsupported target")
    expect_error(lambda: summary_chapters("# S\n\n- [A](a.md)\n- [A2](a.md)"), "twice")


# 이것을 실패시키는 것: 첫 non-draft 장이 nested/README.md 일 때 원본 canonical 을 루트로
@case("mdbook_first_chapter_nested_readme")
def mdbook_first_chapter_nested_readme(tmp: Path) -> None:
    summary = "# S\n\n[D]()\n\n- [G](nested/README.md)\n- [A](a.md)\n"
    files = {"index.html": md_page(), "nested/index.html": md_page(), "a.html": md_page()}
    _, t = run_ok(tmp, summary=summary, md_files=files)
    assert_row(t["nested/index.html"], BASE_URL, footer=True)
    assert_row(t["index.html"], BASE_URL, footer=True)


# 이것을 실패시키는 것: 닫힌 분류(목록 밖 HTML·빠진 장·기대 밖 페이지)
@case("closed_classification")
def closed_classification(tmp: Path) -> None:
    extra = {"index.html": md_page(), "guide/a.html": md_page(), "guide/b/index.html": md_page(),
             "CHANGELOG.html": md_page(), "stray.html": md_page()}
    run_err(tmp / "stray", "outside the page classification", md_files=extra)
    missing = {"index.html": md_page(), "guide/a.html": md_page(), "CHANGELOG.html": md_page()}
    run_err(tmp / "missing", "guide/b/index.html: expected mdBook output is missing", md_files=missing)
    run_err(tmp / "noapi", "api/: rustdoc output is missing", api=False)


def mutate(root: Path, rel: str, text: str | None) -> Path:
    book = build_tree(root)
    path = book / rel
    if text is None:
        path.unlink()
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    return book


def err_after(root: Path, rel: str, text: str | None, fragment: str) -> None:
    book = mutate(root, rel, text)
    expect_error(lambda: postprocess(book, STANDARD_SUMMARY), fragment)


# 이것을 실패시키는 것: crates.js exact-match 파싱, 이름 문법·중복 검사, 집합 동등성
@case("crates_js_contract")
def crates_js_contract(tmp: Path) -> None:
    assert parse_crates_js(crates_js(["a", "b_c"])) == ["a", "b_c"]
    assert parse_crates_js(crates_js(["a"]) + "\n") == ["a"]
    expect_error(lambda: parse_crates_js('var ALL_CRATES = ["a"];'), "unknown format")
    expect_error(lambda: parse_crates_js('window.ALL_CRATES = ["a"];'), "unknown format")
    expect_error(lambda: parse_crates_js(crates_js(["Alpha"])), "bad crate name")
    expect_error(lambda: parse_crates_js(crates_js(["a", "a"])), "duplicate")
    err_after(tmp / "extra", "api/crates.js", crates_js(["alpha", "beta_two", "gamma"]), "!= crate-root")
    err_after(tmp / "less", "api/crates.js", crates_js(["alpha"]), "!= crate-root")


# 이것을 실패시키는 것: crate-root 서명의 각 조건(generator·body class·head 안 meta name·값)
@case("crate_root_signature")
def crate_root_signature(tmp: Path) -> None:
    rel = "api/beta_two/index.html"
    bad = {
        "id_form": rd_page("beta_two", "rustdoc mod crate",
                           vars_tag='<div id="rustdoc-vars" data-current-crate="beta_two"></div>'),
        "no_name": rd_page("beta_two", "rustdoc mod crate",
                           vars_tag='<meta data-current-crate="beta_two">'),
        "two_vars": rd_page("beta_two", "rustdoc mod crate",
                            vars_tag='<meta name="rustdoc-vars" data-current-crate="beta_two">' * 2),
        "wrong_crate": rd_page("alpha", "rustdoc mod crate"),
        "no_crate_class": rd_page("beta_two", "rustdoc mod"),
        "no_generator": rd_page("beta_two", "rustdoc mod crate").replace(
            '<meta name="generator" content="rustdoc">', ""),
        "vars_in_body": rd_page("beta_two", "rustdoc mod crate", vars_tag="").replace(
            "<main>", '<main><meta name="rustdoc-vars" data-current-crate="beta_two">'),
    }
    for name, text in bad.items():
        err_after(tmp / name, rel, text, "!= crate-root")


# 이것을 실패시키는 것: trait.impl/type.impl HTML 금지, api/index.html 금지, 모르는 최상위 HTML
@case("rustdoc_forbidden")
def rustdoc_forbidden(tmp: Path) -> None:
    err_after(tmp / "ti", "api/trait.impl/alpha/x.html", md_page(), "must not contain HTML")
    err_after(tmp / "ty", "api/type.impl/alpha/x.html", md_page(), "must not contain HTML")
    err_after(tmp / "idx", "api/index.html", md_page(), "api/index.html exists")
    err_after(tmp / "top", "api/all.html", md_page(), "unknown top-level rustdoc page")
    err_after(tmp / "dir", "api/static.files/x.html", md_page(), "outside every known rustdoc")
    err_after(tmp / "nomain", "api/alpha/fn.f.html", md_page(main=False), "neither a redirect")


# 이것을 실패시키는 것: 리다이렉트 서명의 네 목적지 일치·title·script·허용 요소 검사
@case("redirect_signature")
def redirect_signature(tmp: Path) -> None:
    rel = "api/alpha/old/struct.X.html"
    t = "../../alpha/struct.X.html"
    bad = {
        "href": rd_redirect(t, href="../other.html"),
        "text": rd_redirect(t, text="elsewhere"),
        "js": rd_redirect(t, js="../other.html"),
        "no_script": rd_redirect(t, script=False),
        "title": rd_redirect(t, title="Moved"),
        "extra": rd_redirect(t).replace("</body>", "<div>x</div></body>"),
        "main": rd_redirect(t).replace("</body>", "<main></main></body>"),  # caught by allowed tags
        "only_title": md_page(main=False).replace("<title>t</title>", "<title>Redirection</title>"),
        "refresh_in_body": rd_redirect(t).replace(
            f'    <meta http-equiv="refresh" content="0;URL={t}">\n', "").replace(
            "<body>", f'<body><meta http-equiv="Refresh" content="0;URL={t}">'),
    }
    for name, text in bad.items():
        err_after(tmp / name, rel, text, "partial redirect signature")
    # http-equiv is matched case-insensitively
    book = mutate(tmp / "case", rel, rd_redirect(t).replace('http-equiv="refresh"', 'http-equiv="REFRESH"'))
    postprocess(book, STANDARD_SUMMARY)
    verify(book, STANDARD_SUMMARY)


# 이것을 실패시키는 것: 같은 태그 no-op, 다른 값·중복·반대 태그 충돌 실패(rule 3)
@case("tag_cardinality")
def tag_cardinality(tmp: Path) -> None:
    same = md_page(f'<link rel="canonical" href="{BASE_URL}guide/a.html">')
    book = mutate(tmp / "same", "guide/a.html", same)
    postprocess(book, STANDARD_SUMMARY)
    assert canon((book / "guide/a.html").read_text(encoding="utf-8")) == [BASE_URL + "guide/a.html"]
    err_after(tmp / "diff", "guide/a.html", md_page('<link rel="canonical" href="https://x/">'),
              "conflicting canonical")
    err_after(tmp / "dup", "guide/a.html", md_page(f'<link rel="canonical" href="{BASE_URL}guide/a.html">' * 2),
              "conflicting canonical")
    err_after(tmp / "robots", "guide/a.html", md_page(NOINDEX_TAG), "already has a robots tag")
    err_after(tmp / "nofollow", "404.html", md_page('<meta name="robots" content="noindex, nofollow">'),
              "conflicting robots")
    err_after(tmp / "dup_robots", "print.html", md_page(NOINDEX_TAG * 2), "conflicting robots")
    err_after(tmp / "canon_on_noindex", "toc.html",
              md_page(f'<link rel="canonical" href="{BASE_URL}">', main=False), "already has a canonical")


# 이것을 실패시키는 것: 삽입 지점 단일성 검사(</head>·</main> 개수)
@case("insertion_points")
def insertion_points(tmp: Path) -> None:
    err_after(tmp / "mains", "guide/a.html", md_page(body="<p>a</p></main><main><p>b</p>"),
              "expected exactly one </main>")
    err_after(tmp / "heads", "guide/a.html", md_page("</head><head>"), "expected exactly one </head>")
    # transform itself must refuse, not only the verify pass that follows it
    plan = PagePlan("x.html", "t", BASE_URL, footer=False)
    expect_error(lambda: transform(plan, md_page("</head><head>")), "expected exactly one </head>")
    err_after(tmp / "nomain", "guide/a.html", md_page(main=False), "expected exactly one </main>")
    err_after(tmp / "prefooter", "guide/a.html", md_page(body=FOOTER), "already has a home link")


# 이것을 실패시키는 것: 표지 위치·버전 검사(rule 4)
@case("marker_rules")
def marker_rules(tmp: Path) -> None:
    err_after(tmp / "v2", "guide/a.html",
              md_page().replace("</head>", "<!-- ai-scream-postprocess v2 --></head>"), "another version")
    err_after(tmp / "moved", "guide/a.html",
              md_page().replace("<title>", MARKER + "<title>"), "another version")
    expect_error(lambda: verify(build_tree(tmp / "raw"), STANDARD_SUMMARY), "not processed")


# 이것을 실패시키는 것: 쓰기 전에 모든 파일을 검증하지 않고 파일마다 바로 쓰는 것(rule 2)
@case("preflight_then_write")
def preflight_then_write(tmp: Path) -> None:
    # CHANGELOG.html sorts before the broken guide/a.html, so a write-as-you-go
    # implementation would already have modified it.
    book = mutate(tmp, "guide/a.html", md_page(body="</main><main>"))
    before = snapshot(book)
    expect_error(lambda: postprocess(book, STANDARD_SUMMARY), "exactly one </main>")
    assert snapshot(book) == before, "a file was written before validation finished"


# 이것을 실패시키는 것: 표지 no-op 가 없어 두 번째 실행이 다시 삽입하는 것, 파일 모드 유실(rule 5)
@case("idempotent_and_mode")
def idempotent_and_mode(tmp: Path) -> None:
    book = build_tree(tmp)
    os.chmod(book / "guide/a.html", 0o644)
    before = snapshot(book)
    assert postprocess(book, STANDARD_SUMMARY) > 0
    once = snapshot(book)
    assert once != before
    assert postprocess(book, STANDARD_SUMMARY) == 0
    assert snapshot(book) == once, "second run changed bytes"
    assert once["guide/a.html"][1] & 0o777 == 0o644, "mode changed"
    assert not [p for p in book.rglob(".*") if p.is_file()], "temp file left behind"


# 이것을 실패시키는 것: 경로 세그먼트 문자 집합 단언(rule 6)
@case("url_rules")
def url_rules(tmp: Path) -> None:
    assert url_for("guide/HwpForge_x.html") == BASE_URL + "guide/HwpForge_x.html"
    expect_error(lambda: url_for("가이드/a.html"), "outside [A-Za-z0-9._-]")
    expect_error(lambda: url_for("a b.html"), "outside [A-Za-z0-9._-]")
    summary = "# S\n\n[I](README.md)\n\n- [G](가이드.md)\n"
    run_err(tmp, "outside [A-Za-z0-9._-]", summary=summary,
            md_files={"index.html": md_page(), "가이드.html": md_page()})


# 이것을 실패시키는 것: --verify 의 DOM 단언(개수·부모·마지막 자식·href·text·금지 클래스)
@case("dom_assertion")
def dom_assertion(tmp: Path) -> None:
    book, texts = run_ok(tmp)
    good = texts["guide/a.html"]
    good_rd = texts["api/alpha/struct.X.html"]
    bad = {
        "duplicate": ("guide/a.html", good.replace(FOOTER, FOOTER * 2), "2 home links"),
        "not_last": ("guide/a.html", good.replace(FOOTER + "</main>", "</main>").replace(
            "<h1>T</h1>", "<h1>T</h1>" + FOOTER), "not the last element"),
        "text_after": ("guide/a.html", good.replace(FOOTER, FOOTER + "tail"), "not the last element"),
        "outside_main": ("guide/a.html", good.replace(FOOTER + "</main>", "</main>" + FOOTER),
                         "not a <p> child"),
        "nested": ("api/alpha/struct.X.html", good_rd.replace(FOOTER + "</main>", "</main>").replace(
            "</section>", FOOTER + "</section>"), "not a <p> child"),
        "href": ("guide/a.html", good.replace(f'href="{HOME_URL}"', 'href="https://x/"'), "exactly one <a"),
        "text": ("guide/a.html", good.replace(f">{HOME_TEXT}<", ">by someone<"), "exactly one <a"),
        "two_a": ("guide/a.html", good.replace(f"{HOME_TEXT}</a>", f"{HOME_TEXT}</a><a href=\"#\">x</a>"),
                  "exactly one <a"),
        "wrapped_text": ("guide/a.html", good.replace("<p class=\"ai-scream-home\">",
                                                      "<p class=\"ai-scream-home\">x"), "text differs"),
        "div_class": ("guide/a.html", good.replace('<p class="ai-scream-home">', '<div class="ai-scream-home">')
                      .replace("</a></p>", "</a></div>"), "not a <p> child"),
        "two_mains": ("guide/a.html", good.replace("<h1>T</h1>", "<main></main>"), "2 <main>"),
        "on_toc": ("toc.html", texts["toc.html"].replace("<ol>", FOOTER + "<ol>"), "must not carry"),
        "on_redirect": ("api/alpha/old/struct.X.html",
                        texts["api/alpha/old/struct.X.html"].replace("</body>", FOOTER + "</body>"),
                        "partial redirect signature"),
        "no_style": ("api/alpha/struct.X.html", good_rd.replace(RUSTDOC_STYLE, ""), "style missing"),
        "canonical_changed": ("guide/a.html", good.replace(BASE_URL + "guide/a.html", BASE_URL),
                              "want canonical"),
    }
    for name, (rel, text, fragment) in bad.items():
        case_book = tmp / name / "book"
        shutil.copytree(book, case_book)
        (case_book / rel).write_text(text, encoding="utf-8")
        try:
            expect_error(lambda b=case_book: verify(b, STANDARD_SUMMARY), fragment)
        except AssertionError as err:
            raise AssertionError(f"{name}: {err}") from None


def self_check() -> int:
    failures = 0
    with tempfile.TemporaryDirectory(prefix="site-postprocess-") as tmpdir:
        for name, fn in CASES:
            case_dir = Path(tmpdir) / name
            case_dir.mkdir()
            try:
                fn(case_dir)
            except Exception as err:  # noqa: BLE001 — report every case
                failures += 1
                print(f"FAIL {name}: {type(err).__name__}: {err}")
            else:
                print(f"ok   {name}")
    print(f"{len(CASES) - failures}/{len(CASES)} self-check cases passed")
    return 1 if failures else 0


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("book", nargs="?", type=Path, help="built site directory (mdBook build-dir)")
    parser.add_argument("--verify", action="store_true", help="check a processed tree, write nothing")
    parser.add_argument("--self-check", action="store_true", help="run the synthetic-tree tests")
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
