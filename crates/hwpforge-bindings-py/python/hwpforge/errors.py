"""The single exception type every HwpForge operation raises."""

from __future__ import annotations

from typing import Any

__all__ = ["HwpForgeError"]


class HwpForgeError(Exception):
    """An operation refused the work it was given.

    The extension module constructs this class directly, so its four-argument
    signature is part of the contract between the Rust and Python layers.

    Attributes:
        code: The stable code for the failure, for example ``"DECODE_FAILED"``
            or ``"ENCODE_SEMANTIC_LOSS"``. The same strings the command line
            tool and the MCP server report.
        message: What went wrong, in one sentence.
        hint: What to do about it, when the operation knows. ``None`` otherwise.
        cause: A second, narrower classification from the library that
            refused, when the failure classifies twice. Only rendering
            does: a ``"PDF_RENDER_FAILED"`` carries
            ``{"stage", "code", "kind", "location"}``, the same keys the
            command line prints under ``cause``, where ``code`` is the
            renderer's own spelling such as ``"FONT_UNRESOLVED"``, and
            ``kind`` and ``location`` are absent unless the failure carries
            them. ``None`` for every other failure.
        details: The failure's structured payload, when the class of failure
            has one. A fail-closed refusal
            (``"ENCODE_SEMANTIC_LOSS"``) carries
            ``{"warnings": [...], "others": [...]}``, where ``warnings`` are
            the semantic losses that caused the refusal and ``others`` the
            remaining warnings of the same encode. Each entry has the same
            ``{"code", "message"}`` shape as a report warning. ``None`` for
            every other failure.
    """

    code: str
    message: str
    hint: str | None
    cause: dict[str, str] | None
    details: dict[str, Any] | None

    def __init__(
        self,
        code: str,
        message: str,
        hint: str | None = None,
        cause: dict[str, str] | None = None,
        details: dict[str, Any] | None = None,
    ) -> None:
        """Build the error from what the operation reported."""
        super().__init__(message)
        self.code = code
        self.message = message
        self.hint = hint
        self.cause = cause
        self.details = details

    def __str__(self) -> str:
        """Render as ``"CODE: message"``, with the hint on a second line if there is one."""
        rendered = f"{self.code}: {self.message}"
        if self.hint is not None:
            rendered = f"{rendered}\nhint: {self.hint}"
        return rendered

    def __repr__(self) -> str:
        """Render as a call that would rebuild this error."""
        return (
            f"HwpForgeError(code={self.code!r}, message={self.message!r}, "
            f"hint={self.hint!r}, cause={self.cause!r}, details={self.details!r})"
        )

    def __reduce__(self) -> tuple[Any, ...]:
        """Keep copy and pickle working even though ``args`` holds only the message."""
        return (self.__class__, (self.code, self.message, self.hint, self.cause, self.details))
