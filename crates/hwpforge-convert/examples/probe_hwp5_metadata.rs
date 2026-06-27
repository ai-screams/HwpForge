//! Probe — convert a Hancom native .hwp through hwp5_to_hwpx_bytes and
//! inspect the resulting HWPX content.hpf metadata block. Wave 12o
//! Phase 3 end-to-end check.

fn main() {
    let path = "examples/hwp5_review/sample-field-docsummary.hwp";
    let bytes = std::fs::read(path).expect("read sample");
    let (hwpx_bytes, warnings) = hwpforge_convert::hwp5_to_hwpx_bytes(&bytes).expect("convert");
    println!("warnings: {}", warnings.len());
    for w in &warnings {
        if format!("{w:?}").contains("Summary") || format!("{w:?}").contains("metadata") {
            println!("  → {w:?}");
        }
    }

    // Extract content.hpf via zip-rs (already in deps via smithy-hwpx)
    let cursor = std::io::Cursor::new(&hwpx_bytes);
    let mut archive = zip::ZipArchive::new(cursor).expect("zip open");
    let mut entry = archive.by_name("Contents/content.hpf").expect("content.hpf");
    use std::io::Read;
    let mut xml = String::new();
    entry.read_to_string(&mut xml).expect("read");
    if let Some(start) = xml.find("<opf:metadata>") {
        if let Some(end) = xml.find("</opf:metadata>") {
            println!("\n=== HWPX content.hpf <opf:metadata> (Wave 12o end-to-end) ===");
            for chunk in xml[start..end + "</opf:metadata>".len()].split('>') {
                println!(">{}", chunk);
            }
        }
    }
}
