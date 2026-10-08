#![forbid(unsafe_code)]

mod analyze;
mod columnar;
mod csv;
mod decimal;
mod duck;
mod fault;
mod fontfile;
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
mod xlsx;

use std::path::Path;

use analyze::Operation;
use limits::{OUT_ARROW, OUT_CSV, OUT_FONT, OUT_MANIFEST, OUT_PARQUET, OUT_SVG, OUT_XLSX};

pub use duck::prove_native_cancel;
pub use fault::Fault;
pub use sha256::sha256_hex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    Citable,
    Uncited,
}

pub fn run(request_path: &Path) -> Result<Written, Fault> {
    let request = request::load_request(request_path)?;
    let cli_sha = if request.duckdb {
        let Some(path) = request.duckdb_cli.as_deref() else {
            return Err(Fault::engine());
        };
        Some(duck::require_cli(Path::new(path))?)
    } else {
        None
    };
    if request.duckdb && request.font_file.is_none() {
        return Err(Fault::engine());
    }
    let font = match request.font_file.as_deref() {
        Some(path) => Some(fontfile::load(Path::new(path))?),
        None => None,
    };
    reject_if_canceled(request.cancel_file.as_deref())?;
    let export = source::load_export(Path::new(&request.export_dir))?;
    let lineage = lineage::load_lineage(Path::new(&request.lineage_file))?;
    if !source::facts_agree(&export.facts, &lineage.facts) {
        return Err(Fault::lineage());
    }
    if request.group_column >= export.width || request.value_column >= export.width {
        return Err(Fault::request());
    }
    let write_columns = request.parquet || request.duckdb;
    let mut names = vec![OUT_CSV, OUT_SVG, OUT_MANIFEST];
    if write_columns {
        names.push(OUT_PARQUET);
        names.push(OUT_ARROW);
    }
    if request.xlsx {
        names.push(OUT_XLSX);
    }
    if font.is_some() {
        names.push(OUT_FONT);
    }
    publish::ensure_available(Path::new(&request.output_dir), &names)?;
    reject_if_canceled(request.cancel_file.as_deref())?;
    let analysis = analyze::analyze(
        &export.rows,
        request.operation,
        request.group_column,
        request.value_column,
        &request.group_header,
        &request.value_header,
    )?;
    let columns = if write_columns {
        Some(columnar::write_columns(&analysis)?)
    } else {
        None
    };
    let cancel = request.cancel_file.as_deref().map(Path::new);
    if request.duckdb {
        let cli = request.duckdb_cli.as_deref().ok_or(Fault::engine())?;
        duck::aggregate(
            Path::new(cli),
            columns.as_ref().ok_or(Fault::engine())?,
            &analysis.groups,
            cancel,
        )?;
    }
    let xlsx_bytes = if request.xlsx {
        Some(xlsx::write(
            &analysis,
            &request.group_header,
            &request.value_header,
        )?)
    } else {
        None
    };
    let chart = svg::render(
        &request.chart_title,
        &lineage.query_id,
        export.clean,
        &analysis.groups,
        request.duckdb,
        font.is_some(),
    )?;
    let parquet_sha = columns.as_ref().map(|item| sha256_hex(&item.parquet));
    let arrow_sha = columns.as_ref().map(|item| sha256_hex(&item.arrow));
    let xlsx_sha = xlsx_bytes.as_ref().map(|item| sha256_hex(item));
    let font_sha = font.as_ref().map(|item| item.sha256.clone());
    let warnings = warning_names(WarnFlags {
        duckdb: request.duckdb,
        columns: columns.is_some(),
        xlsx: xlsx_bytes.is_some(),
        font: font.is_some(),
        null_group: analysis.null_groups > 0,
        null_value: analysis.null_values > 0,
        uncited: !export.clean,
        omitted: chart.label_omitted,
    });
    let manifest = manifest::render(&manifest::Input {
        export: &export,
        lineage: &lineage,
        analysis: &analysis,
        svg: &chart.bytes,
        operation: operation_name(request.operation),
        group_column: request.group_column,
        value_column: request.value_column,
        engine: if request.duckdb { "duckdb" } else { "rust_exact" },
        duckdb: if request.duckdb { "pinned_unaudited" } else { "not_invoked" },
        arrow: if columns.is_some() { "pinned_unaudited" } else { "not_written" },
        parquet: if columns.is_some() { "pinned_unaudited" } else { "not_written" },
        xlsx: if xlsx_bytes.is_some() { "pinned_unaudited" } else { "not_written" },
        cjk_font: if font.is_some() { "pinned_unaudited" } else { "not_bundled" },
        warnings: &warnings,
        parquet_sha256: parquet_sha.as_deref(),
        arrow_sha256: arrow_sha.as_deref(),
        xlsx_sha256: xlsx_sha.as_deref(),
        font_sha256: font_sha.as_deref(),
        duckdb_binary_sha256: cli_sha.as_deref(),
    })?;
    let mut files: Vec<(&str, &[u8])> = vec![
        (OUT_CSV, &analysis.csv),
        (OUT_SVG, &chart.bytes),
        (OUT_MANIFEST, &manifest),
    ];
    if let Some(item) = &columns {
        files.push((OUT_PARQUET, &item.parquet));
        files.push((OUT_ARROW, &item.arrow));
    }
    if let Some(bytes) = &xlsx_bytes {
        files.push((OUT_XLSX, bytes));
    }
    if let Some(item) = &font {
        files.push((OUT_FONT, &item.bytes));
    }
    publish::publish(Path::new(&request.output_dir), &files, cancel)?;
    Ok(if export.clean {
        Written::Citable
    } else {
        Written::Uncited
    })
}

struct WarnFlags {
    duckdb: bool,
    columns: bool,
    xlsx: bool,
    font: bool,
    null_group: bool,
    null_value: bool,
    uncited: bool,
    omitted: bool,
}

fn warning_names(flags: WarnFlags) -> Vec<&'static str> {
    let mut names = vec![
        "admission_not_audited",
        if flags.duckdb { "duckdb_pinned_unaudited" } else { "duckdb_not_invoked" },
        if flags.columns { "arrow_pinned_unaudited" } else { "arrow_not_written" },
        if flags.columns { "parquet_pinned_unaudited" } else { "parquet_not_written" },
        if flags.xlsx { "xlsx_pinned_unaudited" } else { "xlsx_not_written" },
        if flags.font { "cjk_font_pinned_unaudited" } else { "cjk_font_not_bundled" },
    ];
    if flags.null_group {
        names.push("null_group");
    }
    if flags.null_value {
        names.push("null_value");
    }
    if flags.uncited {
        names.push("uncited");
    }
    if flags.omitted {
        names.push("svg_label_omitted");
    }
    names
}

fn reject_if_canceled(cancel: Option<&str>) -> Result<(), Fault> {
    if let Some(path) = cancel
        && path::cancel_requested(Path::new(path))?
    {
        return Err(Fault::canceled());
    }
    Ok(())
}

fn operation_name(operation: Operation) -> &'static str {
    match operation {
        Operation::Sum => "sum",
        Operation::Count => "count",
    }
}
