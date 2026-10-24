"""Common operation errors and OS-compatible file errors."""

from __future__ import annotations

from typing import Any

__all__ = ["HwpForgeError", "HwpForgeFileNotFoundError", "HwpForgeIOError"]


class HwpForgeError(Exception):
    """An operation refused the work it was given.

    The extension module constructs this class directly, so its five-argument
    signature (``code, message, hint, cause, details``) is part of the contract
    between the Rust and Python layers.

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
        """Render the concrete class and its diagnostic fields."""
        os_fields = ""
        if isinstance(self, OSError):
            os_fields = "".join(
                f", {name}={getattr(self, name)!r}"
                for name in ("errno", "strerror", "filename", "filename2", "winerror")
                if hasattr(self, name)
            )
        return (
            f"{type(self).__name__}(code={self.code!r}, message={self.message!r}, "
            f"hint={self.hint!r}, cause={self.cause!r}, details={self.details!r}{os_fields})"
        )

    def __reduce__(self) -> tuple[Any, ...]:
        """Keep copy and pickle working even though ``args`` holds only the message."""
        return (self.__class__, (self.code, self.message, self.hint, self.cause, self.details))


class HwpForgeIOError(HwpForgeError, OSError):
    """A coded file error, also catchable as :class:`OSError`.

    The original OS fields are copied explicitly; HwpForge's five arguments
    are never passed to the OSError constructor.
    """

    def __init__(
        self,
        code: str,
        message: str,
        hint: str | None = None,
        cause: dict[str, str] | None = None,
        details: dict[str, Any] | None = None,
        os_error: OSError | None = None,
    ) -> None:
        """Build a coded error while retaining the OS diagnostic fields."""
        HwpForgeError.__init__(self, code, message, hint, cause, details)
        for name in ("errno", "strerror", "filename", "filename2"):
            setattr(self, name, getattr(os_error, name, None))
        if hasattr(os_error, "winerror"):
            self.winerror = os_error.winerror

    def __reduce__(self) -> tuple[Any, ...]:
        """Restore the exact subclass, five HwpForge fields and OS fields."""
        state = {
            name: getattr(self, name) for name in ("errno", "strerror", "filename", "filename2")
        }
        if hasattr(self, "winerror"):
            state["winerror"] = self.winerror
        return (
            self.__class__,
            (self.code, self.message, self.hint, self.cause, self.details),
            state,
        )


class HwpForgeFileNotFoundError(HwpForgeIOError, FileNotFoundError):
    """An absent input file, also catchable as :class:`FileNotFoundError`."""
