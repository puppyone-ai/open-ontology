#![forbid(unsafe_code)]

mod analyze;
mod csv;
mod decimal;
mod fault;
mod json;
mod limits;
mod lineage;
mod manifest;
mod path;
mod publish;
mod request;
mod sha256;
mod source;
mod svg;

use std::path::Path;

use analyze::Operation;

pub use fault::Fault;
pub use sha256::sha256_hex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    Citable,
    Uncited,
}

pub fn run(request_path: &Path) -> Result<Written, Fault> {
    let request = request::load_request(request_path)?;
    if request.parquet {
        return Err(Fault::parquet());
    }
    if request.xlsx {
        return Err(Fault::xlsx());
    }
    if request.duckdb {
        return Err(Fault::engine());
    }
    reject_if_canceled(request.cancel_file.as_deref())?;
    let export = source::load_export(Path::new(&request.export_dir))?;
    let lineage = lineage::load_lineage(Path::new(&request.lineage_file))?;
    if !source::facts_agree(&export.facts, &lineage.facts) {
        return Err(Fault::lineage());
    }
    if request.group_column >= export.width || request.value_column >= export.width {
        return Err(Fault::request());
    }
    publish::ensure_available(Path::new(&request.output_dir))?;
    reject_if_canceled(request.cancel_file.as_deref())?;
    let analysis = analyze::analyze(
        &export.rows,
        request.operation,
        request.group_column,
        request.value_column,
        &request.group_header,
        &request.value_header,
    )?;
    let chart = svg::render(
        &request.chart_title,
        &lineage.query_id,
        export.clean,
        &analysis.groups,
    )?;
    let manifest = manifest::render(&manifest::Input {
        export: &export,
        lineage: &lineage,
        analysis: &analysis,
        svg: &chart.bytes,
        operation: operation_name(request.operation),
        group_column: request.group_column,
        value_column: request.value_column,
        label_omitted: chart.label_omitted,
    })?;
    let cancel = request.cancel_file.as_deref().map(Path::new);
    publish::publish(
        Path::new(&request.output_dir),
        &analysis.csv,
        &chart.bytes,
        &manifest,
        cancel,
    )?;
    Ok(if export.clean {
        Written::Citable
    } else {
        Written::Uncited
    })
}

fn reject_if_canceled(cancel: Option<&str>) -> Result<(), Fault> {
    if let Some(path) = cancel {
        if path::cancel_requested(Path::new(path))? {
            return Err(Fault::canceled());
        }
    }
    Ok(())
}

fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Sum => "sum",
        Operation::Count => "count",
    }
}
