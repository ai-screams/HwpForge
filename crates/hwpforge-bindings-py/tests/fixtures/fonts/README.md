# Test fonts

`HwpForgeTest-Regular.ttf` is a HwpForge-made synthetic face, drawn from scratch by
`crates/hwpforge-smithy-pdf/tests/fonts/generate_test_fonts.py` and copied here byte for byte. It contains no
Hancom glyphs and no redistributed outlines: only the Latin set `A-Za-z0-9.,:-`, the space, and the fourteen
Hangul syllables 가나다라마바사아자차카타파하, each drawn as a plain rectangle with fixed metrics.

It exists so `synthetic_face.hwpx` renders on any checkout with nothing installed. That document names exactly one
face, `HwpForge Test`, which this file's `name` table already declares, so nothing about the font is rewritten.
`crates/hwpforge-convert/tests/ops_to_pdf.rs` asserts the copy stays byte-identical to its source and that the pair
still renders.
