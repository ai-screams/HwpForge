# List Acceptance Visual

This bundle contains the curated list-acceptance showcase outputs and the Rust
generator snapshot that produced them.

Artifacts:

- `00_all_in_one.hwpx`: combined acceptance sheet
- `01_bullet.hwpx`: plain bullet depth, interruption, and resume
- `02_numbered_outline.hwpx`: numbered and outline depth cases
- `03_checkable.hwpx`: checked, unchecked, and nested checkable bullets
- `04_checkable_continuation.hwpx`: continuation paragraph inside one task item
- `05_mixed_transition.hwpx`: mixed transitions and ordered-parent/task-child cases
- generator snapshot: `list_acceptance_visual.rs`
- shared helper snapshot: `_support/list_visual.rs`

What to inspect:

- bullet glyphs remain plain bullets
- numbered depth follows level formatting
- outline semantics do not collapse into numbered semantics
- checkable bullets render unchecked and checked states correctly
- continuation paragraphs do not get a second checkbox marker
- mixed transitions resume the expected list semantics

Notes:

- The bundled Rust files are copied snapshots of the canonical generators under
  `crates/hwpforge-smithy-hwpx/examples/`.
- They are preserved as-is, including their original output-path assumptions.
