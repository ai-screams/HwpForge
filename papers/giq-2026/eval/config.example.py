"""Shared configuration for evaluation scripts.

Copy this file to config.py and adjust paths as needed.
"""

from pathlib import Path

# ── Paths ──────────────────────────────────────────────────────────
PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent  # HwpForge/
HWPFORGE_BIN = PROJECT_ROOT / "target" / "release" / "hwpforge"

# Corpus (download separately — not included in this repo)
CORPUS_DIR = Path("path/to/your/corpus")

# Output
OUTPUT_DIR = Path(__file__).resolve().parent.parent / "results"

# ── E2 Settings ────────────────────────────────────────────────────
MODELS = {
    "claude": "claude-sonnet-4-20250514",
    "deepseek": "deepseek-chat",
    "gemini": "gemini-2.5-flash",
}
TEMPERATURE = 0
INPUT_CHAR_LIMIT = 180000
