//! Decode-only resource observation.
#[path = "support/resource_runner.rs"]
mod resource_runner;
use hwpforge_core::resource_experiment::{Experiment, Scope, Timing};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 8 {
        return Err("expected input entrypoint timing scope budget output case_id".into());
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
    let mut output = std::fs::File::create(&args[6])?;
    let config = Experiment { timing, scope, budget: args[5].parse()? };

    let rows = resource_runner::run(&bytes, &args[2], config, &args[7])?;
    serde_json::to_writer(&mut output, &rows)?;
    output.sync_all()?;
    Ok(())
}
