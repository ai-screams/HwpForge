"""Read `_hwpforge.pyi` as data.

The stub is hand-written, so the tests treat it as a specification and parse it
with `ast` rather than importing it: a stub is not importable at runtime, and
`typing.get_type_hints` would need the annotations evaluated.
"""

from __future__ import annotations

import ast
from functools import lru_cache
from pathlib import Path

STUB_PATH = Path(__file__).resolve().parent.parent / "python" / "hwpforge" / "_hwpforge.pyi"


@lru_cache(maxsize=1)
def _module() -> ast.Module:
    return ast.parse(STUB_PATH.read_text(encoding="utf-8"), filename=str(STUB_PATH))


@lru_cache(maxsize=1)
def function_names() -> tuple[str, ...]:
    """Every function the stub declares, in the order it declares them."""
    return tuple(node.name for node in _module().body if isinstance(node, ast.FunctionDef))


@lru_cache(maxsize=1)
def _typed_dicts() -> dict[str, tuple[frozenset[str], frozenset[str]]]:
    """Map each `TypedDict` name to its (required, optional) key sets.

    A key annotated `NotRequired[...]` is optional, matching a Rust field with
    `#[serde(skip_serializing_if = ...)]`. Base classes are followed, so a
    `class X(_XRequired, total=False)` pair reads as one dict.
    """
    found: dict[str, tuple[frozenset[str], frozenset[str]]] = {}
    bases: dict[str, list[str]] = {}
    for node in _module().body:
        if not isinstance(node, ast.ClassDef):
            continue
        total = True
        base_names = []
        for base in node.bases:
            if isinstance(base, ast.Name) and base.id != "TypedDict":
                base_names.append(base.id)
        for keyword in node.keywords:
            if keyword.arg == "total" and isinstance(keyword.value, ast.Constant):
                total = bool(keyword.value.value)
        required: set[str] = set()
        optional: set[str] = set()
        for statement in node.body:
            if not isinstance(statement, ast.AnnAssign) or not isinstance(
                statement.target, ast.Name
            ):
                continue
            name = statement.target.id
            if not total or _is_not_required(statement.annotation):
                optional.add(name)
            else:
                required.add(name)
        found[node.name] = (frozenset(required), frozenset(optional))
        bases[node.name] = base_names

    resolved: dict[str, tuple[frozenset[str], frozenset[str]]] = {}

    def resolve(name: str) -> tuple[frozenset[str], frozenset[str]]:
        if name in resolved:
            return resolved[name]
        required, optional = found[name]
        for base in bases.get(name, ()):
            if base in found:
                base_required, base_optional = resolve(base)
                required = required | base_required
                optional = optional | base_optional
        resolved[name] = (required, optional)
        return resolved[name]

    for name in found:
        resolve(name)
    return resolved


def _is_not_required(annotation: ast.expr) -> bool:
    return (
        isinstance(annotation, ast.Subscript)
        and isinstance(annotation.value, ast.Name)
        and annotation.value.id == "NotRequired"
    )


def typed_dict_keys(name: str) -> tuple[frozenset[str], frozenset[str]]:
    """Return the (required, optional) keys the stub declares for `name`."""
    dicts = _typed_dicts()
    if name not in dicts:
        raise AssertionError(f"the stub declares no TypedDict named {name!r}")
    return dicts[name]
