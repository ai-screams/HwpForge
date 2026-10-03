# GIQ 2026 — Evaluation Scripts and Data

Evaluation materials for the paper:

> **Uncovering the Document Infrastructure Gap: Deterministic Document Infrastructure for Digital Sovereignty in AI-Ready Government**

Submitted to Government Information Quarterly (GIQ).

## Structure

```
papers/giq-2026/
├── eval/
│   ├── e1_bulk_conversion.py     # E1: Large-scale document conversion (5,411 docs)
│   ├── e2_run_experiment.py      # E2: AI policy query accuracy (25 questions, 5 documents)
│   ├── input_preparer.py         # Input preparation utility (see Known issues)
│   └── config.example.py         # Configuration template (see Known issues)
├── questions/
│   ├── e2_questions_v2.md        # 25 evaluation questions + reference answers
│   └── e2_experiment_v2_design.md # Experiment design rationale
└── results/
    └── e2_results_latest.json    # Committed E2 results (50 records)
```

## Reproducing the Experiments

### Prerequisites

- HwpForge CLI (`hwpforge`) built from this repository in release mode (`target/release/hwpforge`; both scripts look for that path)
- Python 3.10+
- API key for each model you run in E2, as an environment variable (see below)

### E1: Document Conversion at Scale

`e1_bulk_conversion.py` does not read `config.py`. It reads the corpus from `ref/hwp/corpus/bulk` under `papers/giq-2026/` (the `BULK_DIR` constant in the script) and writes results to `plan/results` under the same directory. Neither directory is committed. Adjust the constants at the top of the script if your corpus lives elsewhere.

```bash
python eval/e1_bulk_conversion.py              # full run
python eval/e1_bulk_conversion.py --dry-run    # counts only
python eval/e1_bulk_conversion.py --ministry 행안부
```

### E2: AI Policy Query Accuracy

`e2_run_experiment.py` does not read `config.py` either. The questions are embedded in the script (`QUESTIONS`), and API keys come from environment variables. The default model is `claude`; pass `--models` to add others. The script compares two conditions (raw XML vs. Markdown from `hwpforge to-md`) per question.

| `--models` value | Model id                   | Environment variable |
| ---------------- | -------------------------- | -------------------- |
| `claude`         | `claude-sonnet-4-20250514` | `ANTHROPIC_API_KEY`  |
| `gpt4`           | `gpt-4o`                   | `OPENAI_API_KEY`     |
| `deepseek`       | `deepseek-chat`            | `DEEPSEEK_API_KEY`   |
| `groq`           | `llama-3.3-70b-versatile`  | `GROQ_API_KEY`       |
| `gemini`         | `gemini-2.5-flash`         | `GEMINI_API_KEY`     |

```bash
export ANTHROPIC_API_KEY=...
python eval/e2_run_experiment.py                      # claude only
python eval/e2_run_experiment.py --models claude,gpt4 # several models
python eval/e2_run_experiment.py --doc 1 --dry-run    # one document, no API calls
```

The script reads documents from `ref/hwp/corpus/{preview,hwpx}` and writes timestamped results to `plan/results/e2/` under `papers/giq-2026/` (also not committed).

## Results in this repository

`results/e2_results_latest.json` holds 50 records: 25 questions × 2 conditions (`raw_xml`, `ddi_markdown`), all from a single model, `claude-sonnet-4-20250514`. Results for the other models the script supports are not committed.

## Known issues

- `eval/input_preparer.py` runs `from config import CORPUS_HWPX, EXISTING_HWPX, HWPFORGE_BIN, INPUT_DIR`, but `eval/config.example.py` defines only `PROJECT_ROOT`, `HWPFORGE_BIN`, `CORPUS_DIR`, `OUTPUT_DIR`, `MODELS`, `TEMPERATURE` and `INPUT_CHAR_LIMIT`. Copying the template to `config.py` is therefore not enough for `input_preparer.py` to import.
- `eval/config.example.py` is not imported by `e1_bulk_conversion.py` or `e2_run_experiment.py`.
- The path comment in `e1_bulk_conversion.py` says `.docs/papers/`, but the constant resolves to `papers/giq-2026/`.

## Corpus

The 5,411 government documents used in E1 were obtained from publicly accessible repositories of ten Korean central government agencies (2016–2026). Due to redistribution restrictions, the corpus is not included. Documents can be re-collected from the same public sources.

## License

Evaluation scripts follow the repository's dual license (MIT / Apache-2.0).
