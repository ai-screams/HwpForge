//! Decode-only resource observation.
#[path = "support/resource_runner.rs"]
mod resource_runner;
use hwpforge_core::resource_experiment::{Experiment, Scope, Timing};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(args.len() == 8 || (args.len() == 9 && args[8] == "--error-detail")) {
        return Err(
            "expected input entrypoint timing scope budget output case_id [--error-detail]".into(),
        );
    }
    let timing = match args[3].as_str() {
        "measured" | "M" => Timing::Measured,
        "post" | "P" => Timing::Post,
        _ => return Err("invalid timing".into()),
    };
    let scope = match args[4].as_str() {
        "local" | "L" => Scope::Local,
        "document" | "D" => Scope::Document,
        _ => return Err("invalid scope".into()),
    };
    let bytes = std::fs::read(&args[1])?;
    let config = Experiment { timing, scope, budget: args[5].parse()? };

    let rows = resource_runner::run(&bytes, &args[2], config, &args[7])?;
    resource_runner::write_output(
        &rows,
        std::path::Path::new(&args[6]),
        &args[7],
        args.len() == 9,
        || resource_runner::error_detail(&bytes, &args[2], config),
    )?;
    Ok(())
}
