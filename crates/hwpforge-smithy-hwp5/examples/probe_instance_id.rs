//! Wave 12p Step 4 debug — extract CtrlHeader trailer instance IDs from a
//! HWP5 file and print them per control type. Used to verify whether the
//! `extract_ctrl_header_trailer_instance_id` heuristic correctly recovers
//! cross-ref target IDs for non-%xrf controls (Footnote/Endnote/etc).
//!
//! Usage:
//!   cargo run -p hwpforge-smithy-hwp5 --example probe_instance_id -- path/to/sample.hwp

use hwpforge_smithy_hwp5::Hwp5Decoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: probe_instance_id <path.hwp>")?;
    let bytes = std::fs::read(&path)?;
    let result = Hwp5Decoder::decode(&bytes)?;
    let doc = result.document;
    for (s_idx, section) in doc.sections().iter().enumerate() {
        for (p_idx, para) in section.paragraphs.iter().enumerate() {
            for (r_idx, run) in para.runs.iter().enumerate() {
                match &run.content {
                    hwpforge_core::run::RunContent::Control(boxed) => match &**boxed {
                        hwpforge_core::Control::Footnote { inst_id, .. } => {
                            println!("Section {s_idx} para {p_idx} run {r_idx}: Footnote inst_id={inst_id:?}");
                        }
                        hwpforge_core::Control::Endnote { inst_id, .. } => {
                            println!("Section {s_idx} para {p_idx} run {r_idx}: Endnote inst_id={inst_id:?}");
                        }
                        hwpforge_core::Control::Equation { inst_id, .. } => {
                            println!("Section {s_idx} para {p_idx} run {r_idx}: Equation inst_id={inst_id:?}");
                        }
                        _ => {}
                    },
                    hwpforge_core::run::RunContent::Image(img) => {
                        println!("Section {s_idx} para {p_idx} run {r_idx}: Image inst_id={:?}", img.inst_id);
                    }
                    hwpforge_core::run::RunContent::Table(t) => {
                        println!("Section {s_idx} para {p_idx} run {r_idx}: Table inst_id={:?}", t.inst_id);
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
