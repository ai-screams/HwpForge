# GIQ 2026 — Evaluation Scripts and Data

Evaluation materials for the paper:

> **Uncovering the Document Infrastructure Gap: Deterministic Document Infrastructure for Digital Sovereignty in AI-Ready Government**

Submitted to Government Information Quarterly (GIQ).

## Structure

```
papers/giq-2026/
├── eval/
│   ├── e1_bulk_conversion.py     # E1: Large-scale conversion with hwpforge (5,411 docs)
│   ├── e2_run_experiment.py      # E2: AI policy query accuracy (25 questions, 5 documents; calls model APIs only)
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

- HwpForge CLI (`hwpforge`) built from this repository in release mode (`target/release/hwpforge`). Only `e1_bulk_conversion.py` runs it; `e2_run_experiment.py` never calls `hwpforge`
- Python 3.10+
- API key for each model you run in E2, as an environment variable (see below)

### E1: Document Conversion at Scale

`e1_bulk_conversion.py` runs the `hwpforge` binary through `subprocess`: `to-md` for each `.hwpx`, and `convert-hwp5` followed by `to-md` for each `.hwp`. It does not read `config.py`. It reads the per-ministry folders `보도자료_<ministry>` under `ref/hwp/corpus/bulk` in `papers/giq-2026/` (the `BULK_DIR` constant in the script) and writes results to `plan/results` under the same directory. Neither directory is committed. Adjust the constants at the top of the script if your corpus lives elsewhere.

```bash
python eval/e1_bulk_conversion.py              # full run
python eval/e1_bulk_conversion.py --dry-run    # counts only
python eval/e1_bulk_conversion.py --ministry 행안부
```

### E2: AI Policy Query Accuracy

`e2_run_experiment.py` does not read `config.py` either. The questions are embedded in the script (`QUESTIONS`), and API keys come from environment variables. The default model is `claude`; pass `--models` to add others. The script does not run `hwpforge` and does not run `to-md`. For each of the five documents it builds two inputs per question: condition A is the raw XML (the `section*.xml` parts) extracted from the `.hwpx` zip, and condition B is a Markdown file that was made beforehand (`hwpforge to-md` output, read as-is from `preview/*.md`). It then sends both to the model API.

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

Under `papers/giq-2026/`, the script reads the `.hwpx` files from `ref/hwp/corpus/hwpx` and the matching pre-made Markdown from `ref/hwp/corpus/preview` (document 5 reads its `.hwpx` from `examples/interop/hwpx_md_convert/hwpx2md/` in this repository, with its Markdown still from `preview/`), and writes timestamped results to `plan/results/e2/` under `papers/giq-2026/` (also not committed).

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
