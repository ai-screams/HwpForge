#!/usr/bin/env python3
"""Synthetic-tree tests for scripts/site_postprocess.py (stdlib unittest).

Run with ``python3 scripts/site_postprocess.py --self-check`` (what CI does) or
``python3 -m unittest scripts/test_site_postprocess.py``. Every check is a
unittest assertion, so ``python3 -O`` does not weaken the suite.

Each test names the code it guards in a "이것을 실패시키는 것" comment; the
mutation that proves it is recorded in the plan's implementation log.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import sys
import tempfile
import unittest
from collections.abc import Callable
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import site_postprocess as sp

BASE = sp.BASE_URL
FOOTER = sp.FOOTER


# --------------------------------------------------------------------------
# Synthetic page and tree builders
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
                js: str | None = None, title: str = "<title>Redirection</title>",
                script: bool = True) -> str:
    js_line = (
        f'    <script>location.replace("{js if js is not None else target}"'
        " + location.search + location.hash);</script>\n" if script else ""
    )
    return (
        '<!DOCTYPE html>\n<html lang="en">\n<head>\n'
        f'    <meta http-equiv="refresh" content="0;URL={target}">\n'
        f"    {title}\n</head>\n<body>\n"
        f'    <p>Redirecting to <a href="{href if href is not None else target}">'
        f"{text if text is not None else target}</a>...</p>\n{js_line}</body>\n</html>"
    )


STANDARD_SUMMARY = (
    "# Summary\n\n[소개](README.md)\n\n---\n\n# 시작하기\n\n"
    "- [A](guide/a.md)\n  - [B](guide/b/README.md)\n\n---\n\n[변경 이력](CHANGELOG.md)\n"
)
REDIRECT = "api/alpha/old/struct.X.html"
REDIRECT_TARGET = "../../alpha/struct.X.html"


def crates_js(names: list[str]) -> str:
    listing = json.dumps(names, separators=(",", ":"))
    lengths = ",".join(str(len(n) + 3) for n in names)
    return f'window.ALL_CRATES = {listing};\n//{{"start":21,"fragment_lengths":[{lengths}]}}'


def standard_md() -> dict[str, str]:
    return {
        "index.html": md_page(),
        "guide/a.html": md_page(),
        "guide/b/index.html": md_page(),
        "CHANGELOG.html": md_page(),
    }


def build_tree(root: Path, summary: str = STANDARD_SUMMARY,
               md_files: dict[str, str] | None = None, api: bool = True) -> Path:
    book = root / "book"
    files = dict(standard_md() if md_files is None else md_files)
    files.setdefault("404.html", md_page('\n        <base href="/HwpForge/">'))
    files.setdefault("print.html", md_page('\n        <meta name="robots" content="noindex">'))
    files.setdefault("toc.html", md_page('\n        <meta name="robots" content="noindex">',
                                         body="<ol><li>x</li></ol>", main=False))
    if api:
        files.update({
            "api/crates.js": crates_js(["alpha", "beta_two"]),
            "api/alpha/index.html": rd_page("alpha", "rustdoc mod crate"),
            "api/alpha/struct.X.html": rd_page("alpha", "rustdoc struct"),
            REDIRECT: rd_redirect(REDIRECT_TARGET),
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
    return book


def snapshot(book: Path) -> dict[str, tuple[bytes, int]]:
    return {
        p.relative_to(book).as_posix(): (p.read_bytes(), p.stat().st_mode)
        for p in sorted(book.rglob("*")) if p.is_file()
    }


def canon(text: str) -> list[str]:
    """Independent of PageScan: plain-string view of the inserted canonical tags."""
    return re.findall(r'<link rel="canonical" href="([^"]*)">', text)


# --------------------------------------------------------------------------
# Base class
# --------------------------------------------------------------------------


class SiteCase(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="site-postprocess-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)
        self._n = 0

    def fresh(self) -> Path:
        self._n += 1
        path = self.tmp / f"t{self._n}"
        path.mkdir()
        return path

    def raises(self, fragment: str, fn: Callable[..., object], *args: object) -> None:
        with self.assertRaises(sp.SiteError) as ctx:
            fn(*args)
        self.assertIn(fragment, str(ctx.exception))

    def run_ok(self, summary: str = STANDARD_SUMMARY,
               md_files: dict[str, str] | None = None) -> tuple[Path, dict[str, str]]:
        book = build_tree(self.fresh(), summary, md_files)
        sp.postprocess(book, summary)
        sp.verify(book, summary)
        return book, sp.read_tree(book)

    def run_err(self, fragment: str, summary: str = STANDARD_SUMMARY,
                md_files: dict[str, str] | None = None, api: bool = True) -> None:
        book = build_tree(self.fresh(), summary, md_files, api)
        self.raises(fragment, sp.postprocess, book, summary)

    def mutated(self, rel: str, text: str) -> Path:
        book = build_tree(self.fresh())
        path = book / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return book

    def err_after(self, rel: str, text: str, fragment: str) -> None:
        self.raises(fragment, sp.postprocess, self.mutated(rel, text), STANDARD_SUMMARY)

    def ok_after(self, rel: str, text: str) -> str:
        book = self.mutated(rel, text)
        sp.postprocess(book, STANDARD_SUMMARY)
        sp.verify(book, STANDARD_SUMMARY)
        return (book / rel).read_text(encoding="utf-8")

    def assert_row(self, text: str, canonical: str | None, footer: bool) -> None:
        head, _, rest = text.partition(sp.MARKER + "</head>")
        self.assertTrue(rest, "marker is not directly before </head>")
        if canonical is None:
            self.assertEqual(canon(text), [])
            self.assertEqual(text.count(sp.NOINDEX_TAG), 1)
        else:
            self.assertEqual(canon(head), [canonical])
            self.assertEqual(canon(rest), [])
            self.assertNotIn("robots", text)
        if footer:
            self.assertEqual(text.count(FOOTER), 1)
            self.assertTrue(text.split(FOOTER, 1)[1].lstrip().lower().startswith("</main>"))
        else:
            self.assertNotIn(sp.FOOTER_CLASS, rest)


# --------------------------------------------------------------------------
# §2.1 / §2.2 table rows
# --------------------------------------------------------------------------


class TableRows(SiteCase):
    # 이것을 실패시키는 것: mdbook_plans 의 canonical·footer 값, transform 의 삽입 위치
    def test_mdbook_rows(self) -> None:
        _, t = self.run_ok()
        self.assert_row(t["index.html"], BASE, footer=True)
        self.assert_row(t["guide/a.html"], BASE + "guide/a.html", footer=True)
        self.assert_row(t["guide/b/index.html"], BASE + "guide/b/index.html", footer=True)
        self.assert_row(t["CHANGELOG.html"], BASE + "CHANGELOG.html", footer=True)
        self.assert_row(t["404.html"], None, footer=True)
        self.assert_row(t["print.html"], None, footer=True)  # existing noindex kept, not doubled
        self.assert_row(t["toc.html"], None, footer=False)

    # 이것을 실패시키는 것: 첫 장 원본의 canonical 을 자기 주소로 두는 것
    def test_first_chapter_not_readme(self) -> None:
        summary = "# Summary\n\n[Intro](intro.md)\n\n- [A](a.md)\n"
        files = {"index.html": md_page(), "intro.html": md_page(), "a.html": md_page()}
        _, t = self.run_ok(summary, files)
        self.assert_row(t["intro.html"], BASE, footer=True)
        self.assert_row(t["index.html"], BASE, footer=True)
        self.assert_row(t["a.html"], BASE + "a.html", footer=True)

    # 이것을 실패시키는 것: 첫 non-draft 장이 nested/README.md 일 때 원본 canonical 을 루트로
    def test_first_chapter_nested_readme(self) -> None:
        summary = "# S\n\n[D]()\n\n- [G](nested/README.md)\n- [A](a.md)\n"
        files = {"index.html": md_page(), "nested/index.html": md_page(), "a.html": md_page()}
        _, t = self.run_ok(summary, files)
        self.assert_row(t["nested/index.html"], BASE, footer=True)
        self.assert_row(t["index.html"], BASE, footer=True)

    # 이것을 실패시키는 것: rustdoc_plans 의 분류(콘텐츠·리다이렉트·보조)
    def test_rustdoc_rows(self) -> None:
        _, t = self.run_ok()
        for rel in ("api/alpha/index.html", "api/alpha/struct.X.html", "api/beta_two/index.html"):
            with self.subTest(rel=rel):
                self.assert_row(t[rel], BASE + rel, footer=True)
                self.assertEqual(t[rel].count(sp.RUSTDOC_STYLE), 1)
        for rel in ("api/help.html", "api/settings.html", "api/src/alpha/lib.rs.html"):
            with self.subTest(rel=rel):
                self.assert_row(t[rel], None, footer=True)
                self.assertEqual(t[rel].count(sp.RUSTDOC_STYLE), 1)
        self.assert_row(t[REDIRECT], None, footer=False)
        self.assertNotIn(sp.RUSTDOC_STYLE, t[REDIRECT])


# --------------------------------------------------------------------------
# §2.3-1 closed classification
# --------------------------------------------------------------------------


class Classification(SiteCase):
    # 이것을 실패시키는 것: 초안·구분선·part title 처리, README→index.html 규칙
    # (기대 목록 = 실제 mdBook 0.4.52 산출물)
    def test_summary_traversal(self) -> None:
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
            with self.subTest(summary=text):
                self.assertEqual(sp.summary_chapters(text), want)
        self.raises("unsupported line", sp.summary_chapters, "# S\n\n* weird line")
        self.raises("unsupported target", sp.summary_chapters, "# S\n\n- [A](../a.md)")
        self.raises("twice", sp.summary_chapters, "# S\n\n- [A](a.md)\n- [A2](a.md)")

    # 이것을 실패시키는 것: 목록 밖 HTML·빠진 장·rustdoc 부재 검사
    def test_closed(self) -> None:
        extra = standard_md() | {"stray.html": md_page()}
        self.run_err("outside the page classification", md_files=extra)
        missing = standard_md()
        del missing["guide/b/index.html"]
        self.run_err("guide/b/index.html: expected mdBook output is missing", md_files=missing)
        self.run_err("api/: rustdoc output is missing", api=False)

    # 이것을 실패시키는 것: crates.js exact-match 파싱, 이름 문법·중복, 집합 동등성
    def test_crates_js(self) -> None:
        self.assertEqual(sp.parse_crates_js(crates_js(["a", "b_c"])), ["a", "b_c"])
        self.assertEqual(sp.parse_crates_js(crates_js(["a"]) + "\n"), ["a"])
        self.raises("unknown format", sp.parse_crates_js, 'var ALL_CRATES = ["a"];')
        self.raises("unknown format", sp.parse_crates_js, 'window.ALL_CRATES = ["a"];')
        self.raises("bad crate name", sp.parse_crates_js, crates_js(["Alpha"]))
        self.raises("duplicate", sp.parse_crates_js, crates_js(["a", "a"]))
        self.err_after("api/crates.js", crates_js(["alpha", "beta_two", "gamma"]), "!= crate-root")
        self.err_after("api/crates.js", crates_js(["alpha"]), "!= crate-root")

    # 이것을 실패시키는 것: crate-root 서명의 각 조건(generator·body class·head 안 meta name·값)
    def test_crate_root_signature(self) -> None:
        tag = '<meta name="rustdoc-vars" data-current-crate="beta_two">'
        bad = {
            "id_form": rd_page("beta_two", "rustdoc mod crate",
                               vars_tag='<div id="rustdoc-vars" data-current-crate="beta_two"></div>'),
            "no_name": rd_page("beta_two", "rustdoc mod crate", vars_tag='<meta data-current-crate="beta_two">'),
            "two_vars": rd_page("beta_two", "rustdoc mod crate", vars_tag=tag * 2),
            "wrong_crate": rd_page("alpha", "rustdoc mod crate"),
            "no_crate_class": rd_page("beta_two", "rustdoc mod"),
            "no_generator": rd_page("beta_two", "rustdoc mod crate").replace(
                '<meta name="generator" content="rustdoc">', ""),
            "vars_in_body": rd_page("beta_two", "rustdoc mod crate", vars_tag="").replace(
                "<main>", "<main>" + tag),
        }
        for name, text in bad.items():
            with self.subTest(name=name):
                self.err_after("api/beta_two/index.html", text, "!= crate-root")

    # 이것을 실패시키는 것: trait.impl/type.impl HTML 금지, api/index.html 금지, 모르는 위치
    def test_rustdoc_forbidden(self) -> None:
        self.err_after("api/trait.impl/alpha/x.html", md_page(), "must not contain HTML")
        self.err_after("api/type.impl/alpha/x.html", md_page(), "must not contain HTML")
        self.err_after("api/index.html", md_page(), "api/index.html exists")
        self.err_after("api/all.html", md_page(), "unknown top-level rustdoc page")
        self.err_after("api/static.files/x.html", md_page(), "outside every known rustdoc")
        self.err_after("api/alpha/fn.f.html", md_page(main=False), "neither a redirect")


# --------------------------------------------------------------------------
# §2.2 redirect signature
# --------------------------------------------------------------------------


class Redirects(SiteCase):
    # 이것을 실패시키는 것: 네 목적지 일치·title/head/body 개수·script·허용 요소 검사
    def test_partial_signature_fails(self) -> None:
        t = REDIRECT_TARGET
        refresh = f'    <meta http-equiv="refresh" content="0;URL={t}">\n'
        bad = {
            "href": rd_redirect(t, href="../other.html"),
            "text": rd_redirect(t, text="elsewhere"),
            "js": rd_redirect(t, js="../other.html"),
            "no_script": rd_redirect(t, script=False),
            "title": rd_redirect(t, title="<title>Moved</title>"),
            "split_title": rd_redirect(t, title="<title>Redir</title><title>ection</title>"),
            "two_heads": rd_redirect(t).replace("<body>", "<head></head><body>"),
            "two_bodies": rd_redirect(t).replace("</body>", "</body><body></body>"),
            "extra": rd_redirect(t).replace("</body>", "<div>x</div></body>"),
            "main": rd_redirect(t).replace("</body>", "<main></main></body>"),
            "only_title": md_page(main=False).replace("<title>t</title>", "<title>Redirection</title>"),
            "refresh_in_body": rd_redirect(t).replace(refresh, "").replace(
                "<body>", f'<body><meta http-equiv="Refresh" content="0;URL={t}">'),
        }
        for name, text in bad.items():
            with self.subTest(name=name):
                self.err_after(REDIRECT, text, "partial redirect signature")

    # 이것을 실패시키는 것: http-equiv 대소문자 무관 비교
    def test_refresh_case_insensitive(self) -> None:
        text = rd_redirect(REDIRECT_TARGET).replace('http-equiv="refresh"', 'http-equiv="REFRESH"')
        self.assert_row(self.ok_after(REDIRECT, text), None, footer=False)


# --------------------------------------------------------------------------
# §2.3-2..6 robustness rules
# --------------------------------------------------------------------------


class Rules(SiteCase):
    # 이것을 실패시키는 것: 같은 태그 no-op, 다른 값·중복·반대 태그 충돌 실패(rule 3)
    def test_tag_cardinality(self) -> None:
        same = md_page(f'<link rel="canonical" href="{BASE}guide/a.html">')
        self.assertEqual(canon(self.ok_after("guide/a.html", same)), [BASE + "guide/a.html"])
        self.err_after("guide/a.html", md_page('<link rel="canonical" href="https://x/">'), "conflicting canonical")
        self.err_after("guide/a.html", md_page(f'<link rel="canonical" href="{BASE}guide/a.html">' * 2),
                       "conflicting canonical")
        self.err_after("guide/a.html", md_page(sp.NOINDEX_TAG), "already has a robots tag")
        self.err_after("404.html", md_page('<meta name="robots" content="noindex, nofollow">'), "conflicting robots")
        self.err_after("print.html", md_page(sp.NOINDEX_TAG * 2), "conflicting robots")
        self.err_after("toc.html", md_page(f'<link rel="canonical" href="{BASE}">', main=False),
                       "already has a canonical")

    # 이것을 실패시키는 것: canonical·robots 의 <head> 안/밖 구분(transform·verify 둘 다)
    def test_head_only_tags(self) -> None:
        own = f'<link rel="canonical" href="{BASE}guide/a.html">'
        self.err_after("guide/a.html", md_page(body="<p>x</p>" + own), "outside <head>")
        self.err_after("404.html", md_page(body="<p>x</p>" + sp.NOINDEX_TAG), "outside <head>")
        _, t = self.run_ok()
        plan = sp.PagePlan("guide/a.html", "mdbook-chapter", BASE + "guide/a.html", footer=True)
        moved = t["guide/a.html"].replace(own, "").replace("<h1>T</h1>", own + "<h1>T</h1>")
        self.raises("outside <head>", sp.verify_page, plan, moved)
        plan404 = sp.PagePlan("404.html", "mdbook-404", None, footer=True)
        moved404 = t["404.html"].replace(sp.NOINDEX_TAG, "", 1).replace("<h1>T</h1>", sp.NOINDEX_TAG + "<h1>T</h1>")
        self.raises("outside <head>", sp.verify_page, plan404, moved404)

    # 이것을 실패시키는 것: 삽입 지점 단일성 검사(</head>·</main> 개수, 순서)
    def test_insertion_points(self) -> None:
        self.err_after("guide/a.html", md_page(body="<p>a</p></main><main><p>b</p>"), "expected one <main>, found 2")
        self.err_after("guide/a.html", md_page(body="<p>a</p></main>"), "expected exactly one </main>")
        self.err_after("guide/a.html", md_page("</head><head>"), "expected exactly one </head>")
        self.err_after("guide/a.html", md_page(main=False), "expected one <main>")
        self.err_after("guide/a.html", md_page(body=FOOTER), "already has a home link")
        self.err_after("guide/a.html", md_page("<main></main>", main=False), "</main> comes before </head>")
        plan = sp.PagePlan("x.html", "t", BASE, footer=False)
        self.raises("expected exactly one </head>", sp.transform, plan, md_page("</head><head>"))

    # 이것을 실패시키는 것: 문자열 검색으로 삽입 지점을 찾는 것(주석·스크립트·대문자 끝 태그)
    def test_insertion_points_from_parser(self) -> None:
        variants = {
            "comment_main": md_page(body="<p>a</p><!-- </main> -->"),
            "script_head": md_page('<script>const x = "</head>";</script>'),
            "upper_tags": md_page().replace("</head>", "</HEAD>").replace("</main>", "</MAIN>"),
        }
        for name, text in variants.items():
            with self.subTest(name=name):
                out = self.ok_after("guide/a.html", text)
                self.assertEqual(canon(out), [BASE + "guide/a.html"])
                self.assertEqual(out.count(sp.MARKER), 1)
                self.assertRegex(out, re.escape(sp.MARKER) + r"</(?i:head)>", "marker before real </head>")
                self.assertEqual(out.count(FOOTER), 1)
                self.assertTrue(out.split(FOOTER, 1)[1].lstrip().lower().startswith("</main>"))
        # the fake </main> in the comment stays after the footer's real target
        out = self.ok_after("guide/a.html", variants["comment_main"])
        self.assertLess(out.index("<!-- </main> -->"), out.index(FOOTER))

    # 이것을 실패시키는 것: 표지 위치·버전 검사(rule 4)
    def test_marker(self) -> None:
        self.err_after("guide/a.html", md_page().replace("</head>", "<!-- ai-scream-postprocess v2 --></head>"),
                       "another version")
        self.err_after("guide/a.html", md_page().replace("<title>", sp.MARKER + "<title>"), "another version")
        self.raises("not processed", sp.verify, build_tree(self.fresh()), STANDARD_SUMMARY)

    # 이것을 실패시키는 것: 쓰기 전에 모든 파일을 검증하지 않고 파일마다 바로 쓰는 것(rule 2)
    def test_preflight_then_write(self) -> None:
        # CHANGELOG.html sorts before the broken guide/a.html, so a write-as-you-go
        # implementation would already have modified it.
        book = self.mutated("guide/a.html", md_page(body="</main><main>"))
        before = snapshot(book)
        self.raises("expected one <main>", sp.postprocess, book, STANDARD_SUMMARY)
        self.assertEqual(snapshot(book), before, "a file was written before validation finished")

    # 이것을 실패시키는 것: 표지 no-op 부재(두 번째 실행 재삽입), 파일 모드 유실(rule 5)
    def test_idempotent_and_mode(self) -> None:
        book = build_tree(self.fresh())
        os.chmod(book / "guide/a.html", 0o644)
        before = snapshot(book)
        self.assertGreater(sp.postprocess(book, STANDARD_SUMMARY), 0)
        once = snapshot(book)
        self.assertNotEqual(once, before)
        self.assertEqual(sp.postprocess(book, STANDARD_SUMMARY), 0)
        self.assertEqual(snapshot(book), once, "second run changed bytes")
        self.assertEqual(once["guide/a.html"][1] & 0o777, 0o644, "mode changed")
        self.assertEqual([p for p in book.rglob(".*") if p.is_file()], [], "temp file left behind")

    # 이것을 실패시키는 것: 세그먼트별 퍼센트 인코딩(quote safe=""), 경로 탈출 거부(rule 6)
    def test_url_encoding(self) -> None:
        self.assertEqual(sp.url_for("guide/HwpForge_x.html"), BASE + "guide/HwpForge_x.html")
        self.assertEqual(sp.url_for("가이드/a.html"), BASE + "%EA%B0%80%EC%9D%B4%EB%93%9C/a.html")
        self.assertEqual(sp.url_for("a b.html"), BASE + "a%20b.html")
        self.assertEqual(sp.url_for("a#b?.html"), BASE + "a%23b%3F.html")
        for rel in ("", "a//b.html", "./a.html", "a/../b.html", "a/"):
            with self.subTest(rel=rel):
                self.raises("empty or dot path segment", sp.url_for, rel)
        summary = "# S\n\n[I](README.md)\n\n- [G](가이드/새 장.md)\n"
        _, t = self.run_ok(summary, {"index.html": md_page(), "가이드/새 장.html": md_page()})
        self.assert_row(t["가이드/새 장.html"],
                        BASE + "%EA%B0%80%EC%9D%B4%EB%93%9C/%EC%83%88%20%EC%9E%A5.html", footer=True)


# --------------------------------------------------------------------------
# --verify DOM assertion
# --------------------------------------------------------------------------


class DomAssertion(SiteCase):
    # 이것을 실패시키는 것: 개수·부모·마지막 자식·href·text·금지 클래스 검사
    def test_verify_rejects(self) -> None:
        book, texts = self.run_ok()
        good = texts["guide/a.html"]
        good_rd = texts["api/alpha/struct.X.html"]
        home = f'href="{sp.HOME_URL}"'
        bad = {
            "duplicate": ("guide/a.html", good.replace(FOOTER, FOOTER * 2), "2 home links"),
            "not_last": ("guide/a.html", good.replace(FOOTER + "</main>", "</main>").replace(
                "<h1>T</h1>", "<h1>T</h1>" + FOOTER), "not the last element"),
            "text_after": ("guide/a.html", good.replace(FOOTER, FOOTER + "tail"), "not the last element"),
            "outside_main": ("guide/a.html", good.replace(FOOTER + "</main>", "</main>" + FOOTER),
                             "not a <p> child"),
            "nested": ("api/alpha/struct.X.html", good_rd.replace(FOOTER + "</main>", "</main>").replace(
                "</section>", FOOTER + "</section>"), "not a <p> child"),
            "href": ("guide/a.html", good.replace(home, 'href="https://x/"'), "exactly one <a"),
            "text": ("guide/a.html", good.replace(f">{sp.HOME_TEXT}<", ">by someone<"), "exactly one <a"),
            "two_a": ("guide/a.html", good.replace(f"{sp.HOME_TEXT}</a>", f'{sp.HOME_TEXT}</a><a href="#">x</a>'),
                      "exactly one <a"),
            "wrapped_text": ("guide/a.html", good.replace('<p class="ai-scream-home">', '<p class="ai-scream-home">x'),
                             "text differs"),
            "div_class": ("guide/a.html", good.replace('<p class="ai-scream-home">', '<div class="ai-scream-home">')
                          .replace("</a></p>", "</a></div>"), "not a <p> child"),
            "two_mains": ("guide/a.html", good.replace("<h1>T</h1>", "<main></main>"), "2 <main>"),
            "on_toc": ("toc.html", texts["toc.html"].replace("<ol>", FOOTER + "<ol>"), "must not carry"),
            "on_redirect": (REDIRECT, texts[REDIRECT].replace("</body>", FOOTER + "</body>"),
                            "partial redirect signature"),
            "no_style": ("api/alpha/struct.X.html", good_rd.replace(sp.RUSTDOC_STYLE, ""), "style missing"),
            "canonical_changed": ("guide/a.html", good.replace(BASE + "guide/a.html", BASE), "want canonical"),
        }
        for name, (rel, text, fragment) in bad.items():
            with self.subTest(name=name):
                case_book = self.fresh() / "book"
                shutil.copytree(book, case_book)
                (case_book / rel).write_text(text, encoding="utf-8")
                self.raises(fragment, sp.verify, case_book, STANDARD_SUMMARY)


if __name__ == "__main__":
    unittest.main()
