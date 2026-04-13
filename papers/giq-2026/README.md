# GIQ 2026 — Evaluation Scripts and Data

Evaluation materials for the paper:

> **Uncovering the Document Infrastructure Gap: Deterministic Document Infrastructure for Digital Sovereignty in AI-Ready Government**

Submitted to Government Information Quarterly (GIQ).

## Structure

```
papers/giq-2026/
├── eval/
│   ├── e1_bulk_conversion.py     # E1: Large-scale document conversion (5,411 docs)
│   ├── e2_run_experiment.py      # E2: AI policy query accuracy (25 questions × 3 models)
│   ├── input_preparer.py         # Input data preparation utility
│   └── config.example.py         # Configuration template (copy to config.py)
├── questions/
│   ├── e2_questions_v2.md        # 25 evaluation questions + reference answers
│   └── e2_experiment_v2_design.md # Experiment design rationale
└── results/
    └── e2_results_latest.json    # Final E2 experiment results
```

## Reproducing the Experiments

### Prerequisites

- HwpForge CLI (`hwpforge`) built from this repository
- Python 3.10+
- API keys for Claude, DeepSeek, Gemini (for E2)

### E1: Document Conversion at Scale

```bash
cp eval/config.example.py eval/config.py
# Edit config.py with your corpus path
python eval/e1_bulk_conversion.py
```

### E2: AI Policy Query Accuracy

```bash
# Edit config.py with your API keys
python eval/e2_run_experiment.py
```

## Corpus

The 5,411 government documents used in E1 were obtained from publicly accessible repositories of ten Korean central government agencies (2016–2026). Due to redistribution restrictions, the corpus is not included. Documents can be re-collected from the same public sources.

## License

Evaluation scripts follow the repository's dual license (MIT / Apache-2.0).
