"""Writes definition_shapes_base.txt: the baseline CLI's to-md output for the
definition-shaped texts `lossy_definition_shapes` (src/encoder/guarded_tests.rs)
generates, in its order.

Baseline: commit bd3f6e67 (the commit before G4 touched the fallback and the
escapers). Build its CLI (`cargo build --release -p hwpforge-bindings-cli` in a
worktree of that commit; the file was written with the CLI of 643313cc, whose
smithy-md and core sources are the same), then from this directory:

    uv run --no-project python definition_shapes_base.py <hwpforge-bd3f6e67> > definition_shapes_base.txt

Each text goes in a one-paragraph HWPX, as body text and as a note's body,
made with the same CLI (convert, to-json, text replaced, from-json).
Standard library only.
"""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


def texts():
    for before in ["", "- x\n\n", "- x\n"]:
        for marker in ["-", "+", "1.", "1)", ">", ""]:
            for indent in ["\t" + " " * k for k in range(5)] + [
                " " * k for k in range(7)
            ]:
                for shape in [
                    "[r]: /u",
                    "[^r]: y",
                    "[r]\n]: /u",
                    "[^r]\n]: y",
                    "[r]",
                    "[",
                ]:
                    yield before + marker + indent + shape


def run(cli, *args):
    out = subprocess.run(
        [cli, "--json", *args], check=False, capture_output=True, text=True
    )
    if out.returncode != 0:
        sys.exit(f"{args}: {out.stderr}")
    return (out.stdout + out.stderr).count("MARKUP_WRITTEN_AS_TEXT")


def escape(s):
    return s.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t")


def main():
    cli = os.path.abspath(sys.argv[1])
    with tempfile.TemporaryDirectory() as tmp:
        p = lambda name: os.path.join(tmp, name)
        templates = {}
        for kind, md in [
            ("body", "PLACEHOLDER\n"),
            ("note", "a[^1]\n\n[^1]: PLACEHOLDER\n"),
        ]:
            Path(p(f"{kind}.md")).write_text(md)
            run(cli, "convert", p(f"{kind}.md"), "-o", p(f"{kind}.hwpx"))
            run(cli, "to-json", p(f"{kind}.hwpx"), "-o", p(f"{kind}.json"))
            templates[kind] = Path(p(f"{kind}.json")).read_text()
        print(
            "# Baseline: commit bd3f6e67's to-md (CLI of 643313cc, same smithy-md and core)."
        )
        print("# Regenerate from this directory with a CLI built from it:")
        print(
            "#   uv run --no-project python definition_shapes_base.py <hwpforge-bd3f6e67> > definition_shapes_base.txt"
        )
        print(
            "# One line per text: text, lossy output, its MarkupWrittenAsText count, styled"
        )
        print(
            "# output, its count, styled output with the text as a note body, its count."
        )
        print("# Tab separated; `\\\\`, `\\n` and `\\t` escaped.")
        for text in texts():
            row = [escape(text)]
            for kind, modes in [("body", ["lossy", "styled"]), ("note", ["styled"])]:
                doc = templates[kind].replace('"PLACEHOLDER"', json.dumps(text))
                Path(p("t.json")).write_text(doc)
                run(cli, "from-json", p("t.json"), "-o", p("t.hwpx"))
                for mode in modes:
                    warned = run(
                        cli, "to-md", p("t.hwpx"), "--mode", mode, "-o", p("t.md")
                    )
                    row += [
                        escape(Path(p("t.md")).read_text(encoding="utf-8")),
                        str(warned),
                    ]
            print("\t".join(row))


main()
