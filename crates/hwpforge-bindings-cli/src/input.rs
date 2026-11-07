//! CLI input conversion and pre-write protection. Paths are assumed stable
//! during execution, as for the existing path-based CLI writers.
use crate::compat::{self, Command};
use crate::error::{CliCode, CliError};
use hwpforge_convert::ops::{open_as_hwpx, OpenOptions, OpenedHwpx, SourceFormat};
use hwpforge_foundation::OpsCode;
use serde::Serialize;
use std::ops::Deref;
use std::path::Path;

pub(crate) struct HwpxInput(OpenedHwpx<'static>);
impl Deref for HwpxInput {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0.bytes
    }
}

pub(crate) fn read_input_hwpx(path: &Path, json: bool, command: Command) -> HwpxInput {
    open_input_hwpx(crate::error::read_input(path, json, command), json, command)
}

pub(crate) fn open_input_hwpx(bytes: Vec<u8>, json: bool, command: Command) -> HwpxInput {
    try_open_input_hwpx(bytes, json)
        .unwrap_or_else(|error| compat::convert_error(command, error).exit(json, command))
}

pub(crate) fn try_open_input_hwpx(
    bytes: Vec<u8>,
    json: bool,
) -> Result<HwpxInput, hwpforge_convert::ops::ConvertOpsError> {
    let opened = open_as_hwpx(bytes, &OpenOptions::default())?;
    if !json {
        if let Some(report) = &opened.conversion {
            eprintln!(
                "[conversion] HWP5 입력을 HWPX로 변환했습니다. 문서 편집 결과는 .hwpx입니다."
            );
            for warning in report.meta().warnings {
                eprintln!(
                    "[{}] {}: {}{}",
                    warning.stage,
                    warning.code,
                    warning.location.map(|s| format!("{s}: ")).unwrap_or_default(),
                    warning.message
                );
            }
        }
    }
    Ok(HwpxInput(opened))
}

impl HwpxInput {
    pub(crate) fn metadata(&self) -> Option<hwpforge_convert::ops::ConversionMeta> {
        self.0.conversion.as_ref().map(|report| report.meta())
    }

    pub(crate) fn json(&self, value: &impl Serialize) -> serde_json::Result<String> {
        match self.metadata() {
            Some(conversion) => {
                let mut value = serde_json::to_value(value)?;
                value["conversion"] = serde_json::to_value(conversion)?;
                serde_json::to_string(&value)
            }
            None => serde_json::to_string(value),
        }
    }

    /// Check every artifact before the first write; never apply edit policy here.
    pub(crate) fn protect_source(&self, input: &Path, output: &Path, json: bool, command: Command) {
        if self.0.source == SourceFormat::Hwp5 {
            match same_existing_file(input, output) {
                Ok(true) => CliError::new(
                    CliCode::Ops(OpsCode::InvalidOutputPath),
                    format!("Output '{}' would overwrite the HWP5 input", output.display()),
                )
                .exit(json, command),
                Ok(false) => {}
                Err(error) => CliError::new(
                    CliCode::Ops(OpsCode::FileWriteFailed),
                    format!("Cannot verify output '{}': {error}", output.display()),
                )
                .exit(json, command),
            }
        }
    }

    /// Called only after the existing operation (including admission) succeeds.
    pub(crate) fn check_edit(
        &self,
        input: &Path,
        output: &Path,
        manifest: Option<&Path>,
        accept: bool,
        json: bool,
        command: Command,
    ) {
        if output.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("hwp")) {
            CliError::new(
                CliCode::Ops(OpsCode::InvalidExtension),
                "문서 편집 결과는 .hwpx입니다. .hwp 출력은 지원하지 않습니다.",
            )
            .exit(json, command);
        }
        for output in std::iter::once(output).chain(manifest) {
            self.protect_source(input, output, json, command);
        }
        if let Some(report) = &self.0.conversion {
            report
                .ensure_accepted(accept)
                .unwrap_or_else(|error| compat::convert_error(command, error).exit(json, command));
        }
    }
}

pub(crate) fn same_existing_file(input: &Path, output: &Path) -> std::io::Result<bool> {
    // Only regular files can alias each other here. Inspect both paths before
    // opening either one: opening a FIFO can block after its writer has exited.
    // No reservation or creation: missing targets belong to the existing writer.
    if !output.try_exists()?
        || !std::fs::metadata(output)?.is_file()
        || !std::fs::metadata(input)?.is_file()
    {
        return Ok(false);
    }
    let source = same_file::Handle::from_path(input)?;
    let target = same_file::Handle::from_path(output).or_else(|_| {
        // Opening for writing without create/truncate preserves write-only files.
        same_file::Handle::from_file(std::fs::OpenOptions::new().write(true).open(output)?)
    })?;
    Ok(source == target)
}
