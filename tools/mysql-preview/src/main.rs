//! `mysql-preview run --config PATH --out DIR`
//! `mysql-preview manifest --access PATH --seal PATH --out PATH`

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use mysql_preview::{
    baseline_stream, baseline_stream_v2, capture_native, audit_source, audit_target, guard_source, install_column_acl, load_fixture, load_logistics, prepare_random_gold, run, run_customer, write_manifest,
};

fn masked_error(error: &(dyn std::error::Error + 'static)) -> String {
    if let Some(error) = error.downcast_ref::<oo_connectors::mysql::MysqlError>() {
        return format!("connector:{}:{}", error.code, error.fact);
    }
    if let Some(mysql_async::Error::Server(error)) = error.downcast_ref::<mysql_async::Error>() {
        return format!("mysql_server:{}:{}", error.code, error.state);
    }
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        return format!("io:{:?}", error.kind());
    }
    if error.is::<tokio::time::error::Elapsed>() { return "client_timeout".into(); }
    let message = error.to_string();
    let code = message.split(':').next().unwrap_or("");
    if ["SCHEMA_DRIFT","PROJECTION_DRIFT","KEY_ORDER_DRIFT","budget","source facts","plaintext source facts",
        "occurrence/authority mismatch","config changed","pin changed","needs_bootstrap"].contains(&code)
        || code.starts_with("C5_") && code.len() <= 96 && code.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    { return code.into(); }
    "internal_refusal".into()
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn error_codes_never_include_server_messages_or_connection_secrets() {
        let error = mysql_async::Error::Server(mysql_async::ServerError {
            code: 1064, state: "42000".into(), message: "secret customer value".into(),
        });
        assert_eq!(masked_error(&error), "mysql_server:1064:42000");
        let error: Box<dyn std::error::Error + Send + Sync> = "password=private".into();
        assert_eq!(masked_error(error.as_ref()), "internal_refusal");
        let error = oo_connectors::mysql::MysqlError::new("catalog", "columns");
        assert_eq!(masked_error(&error), "connector:catalog:columns");
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    match command.as_str() {
        "capture-native" => capture_cmd(&mut args),
        "c5-inspect" => {
            let (Some(flag), Some(path), None) = (args.next(), args.next(), args.next()) else {return ExitCode::from(2);};
            if flag != "--config" {return ExitCode::from(2);}
            let runtime = tokio::runtime::Runtime::new().expect("runtime");
            match runtime.block_on(mysql_preview::c5_inspect::run(std::path::Path::new(&path))) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("mysql-preview: c5 inspection refused code={}", masked_error(error.as_ref()));
                    ExitCode::from(1)
                }
            }
        }
        "c5-layered" => {
            let (Some(flag), Some(path), None) = (args.next(), args.next(), args.next()) else {return ExitCode::from(2);};
            if flag != "--config" {return ExitCode::from(2);}
            let runtime=tokio::runtime::Runtime::new().expect("runtime");
            match runtime.block_on(mysql_preview::c5_layered::run_stdio(std::path::Path::new(&path))) {
                Ok(())=>ExitCode::SUCCESS,
                Err(error)=>{
                    if error.to_string()=="SCHEMA_DRIFT" {eprintln!("mysql-preview: SCHEMA_DRIFT");}
                    else {eprintln!("mysql-preview: layered producer refused code={}", masked_error(error.as_ref()));}
                    ExitCode::from(1)
                }
            }
        }
        "arrow-cells" => {
            let (Some(flag),Some(path),None)=(args.next(),args.next(),args.next())else{return ExitCode::from(2);};
            if flag!="--arrow" {return ExitCode::from(2);}
            match arrow_cells(std::path::Path::new(&path)) {Ok(())=>ExitCode::SUCCESS,Err(_)=>{eprintln!("mysql-preview: arrow cells refused");ExitCode::from(1)}}
        }
        "capture-full" => {
            let (Some(config), Some(out)) = pair(&mut args) else {
                return ExitCode::from(2);
            };
            block_on(mysql_preview::capture_full(&config, &out))
        }
        "manifest" => manifest(&mut args),
        "run" => run_cmd(&mut args),
        "customer" => customer_cmd(&mut args),
        "dba-load" => dba_load(&mut args),
        "dba-fixture" => fixture_cmd(&mut args),
        "install-column-acl" => column_acl_cmd(&mut args),
        "scale-audit" => scale_audit(&mut args),
        "scale-target" => scale_target(&mut args),
        "scale-guard" => scale_guard(&mut args),
        "c1-dba" => c1_dba(&mut args),
        "c1-prove" => c1_prove(&mut args),
        "c1-expire" => c1_expire(&mut args),
        "c1-native" => c1_native(&mut args),
        "c1-onboarding-prepare" => c1_onboarding_prepare(&mut args),
        "c1-handoff-preflight" => {
            let (Some(flag),Some(path),None)=(args.next(),args.next(),args.next()) else {return ExitCode::from(2);};
            if flag!="--config" {return ExitCode::from(2);}
            block_on(mysql_preview::c1_handoff::preflight(std::path::Path::new(&path)))
        }
        "c1-baseline-stream" => c1_baseline_stream(&mut args),
        "c1-baseline-stream-v2" => c1_baseline_stream_v2(&mut args),
        "c1-baseline-stream-v3" => {
            let (Some(config),Some(out))=pair(&mut args) else {return ExitCode::from(2);};
            block_on(mysql_preview::c1_typed_source::run_stdio(&config,&out))
        }
        "c1-materialize-v2" => c1_materialize_v2(&mut args),
        "c1-random-gold" => c1_random_gold(&mut args),
        "source-sql" => block_on(mysql_preview::source_sql::run_stdio()),
        _ => {
            eprintln!("mysql-preview: usage");
            ExitCode::from(2)
        }
    }
}

fn c1_onboarding_prepare(args: &mut impl Iterator<Item=String>)->ExitCode {
    let mut format=None;
    let mut rest=Vec::new();
    while let Some(arg)=args.next() {
        if arg=="--c1-baseline-format" {
            let Some(value)=args.next() else {return ExitCode::from(2);};
            format=Some(value);
        } else {
            rest.push(arg);
        }
    }
    let (Some(config),Some(out))=pair(&mut rest.into_iter()) else {return ExitCode::from(2);};
    block_on(mysql_preview::prepare_onboarding(&config,&out,format.as_deref()))
}

fn c1_baseline_stream(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(baseline_stream(&config, &out))
}

fn c1_baseline_stream_v2(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(baseline_stream_v2(&config, &out))
}

/// `c1-materialize-v2 --out DIR --context PATH`. Prints the final-load request as one JSON line.
fn c1_materialize_v2(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut out = None;
    let mut context = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            ("--context", Some(path)) => context = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(out), Some(context)) = (out, context) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    match mysql_preview::c1_materialize_v2(&out, &context) {
        Ok(request) => {
            println!("{request}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("mysql-preview: {} {}", err.code, err.fact);
            ExitCode::from(exit_code(err.code))
        }
    }
}

fn run_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(run(&config, &out))
}

fn customer_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(run_customer(&config, &out))
}

fn fixture_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(load_fixture(&config, &out))
}

fn column_acl_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(install_column_acl(&config, &out))
}

fn pair(args: &mut impl Iterator<Item = String>) -> (Option<PathBuf>, Option<PathBuf>) {
    let mut config = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => return (None, None),
        }
    }
    (config, out)
}

fn dba_load(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(load_logistics(&config, &out))
}

fn scale_audit(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut out = None;
    let mut previous = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            ("--previous", Some(path)) => previous = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(audit_source(&config, &out, previous.as_deref()))
}

fn scale_target(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut outcome = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--outcome", Some(path)) => outcome = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(outcome), Some(out)) = (config, outcome, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(audit_target(&config, &outcome, &out))
}

fn scale_guard(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut config = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    block_on(guard_source(&config, &out))
}

fn block_on(work: impl std::future::Future<Output = Result<(), mysql_preview::PreviewError>>) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("mysql-preview: io");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(work) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mysql-preview: {} {}", err.code, err.fact);
            ExitCode::from(exit_code(err.code))
        }
    }
}

fn manifest(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let mut access = None;
    let mut seal = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--access", Some(path)) => access = Some(PathBuf::from(path)),
            ("--seal", Some(path)) => seal = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => {
                eprintln!("mysql-preview: usage");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(access), Some(seal), Some(out)) = (access, seal, out) else {
        eprintln!("mysql-preview: usage");
        return ExitCode::from(2);
    };
    match write_manifest(&access, &seal, &out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mysql-preview: {} {}", err.code, err.fact);
            ExitCode::from(exit_code(err.code))
        }
    }
}

fn exit_code(code: &str) -> u8 {
    match code {
        "config" | "budget" | "ident" => 3,
        "connect" | "tls" => 4,
        "privilege" | "session" => 5,
        "version" => 6,
        "catalog" => 7,
        "value" | "batch" | "protocol" => 8,
        "target" => 9,
        _ => 1,
    }
}

fn capture_cmd(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(capture_native(&config, &out))
}

fn c1_dba(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(mysql_preview::configure_dba(&config, &out))
}

fn c1_prove(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(mysql_preview::prove_c1(&config, &out))
}

fn c1_native(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(mysql_preview::capture_business(&config, &out))
}

fn c1_random_gold(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(config), Some(out)) = pair(args) else { return ExitCode::from(2); };
    block_on(prepare_random_gold(&config, &out))
}

fn c1_expire(args: &mut impl Iterator<Item = String>) -> ExitCode {
    let (Some(from), Some(out)) = c1_expire_args(args) else { return ExitCode::from(2); };
    block_on(mysql_preview::expire_c1(&from.0, &from.1, &out))
}

fn c1_expire_args(args: &mut impl Iterator<Item = String>) -> (Option<(PathBuf, PathBuf)>, Option<PathBuf>) {
    let mut config = None;
    let mut from = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next();
        match (flag.as_str(), value) {
            ("--config", Some(path)) => config = Some(PathBuf::from(path)),
            ("--from", Some(path)) => from = Some(PathBuf::from(path)),
            ("--out", Some(path)) => out = Some(PathBuf::from(path)),
            _ => return (None, None),
        }
    }
    match (config, from) {
        (Some(config), Some(from)) => (Some((config, from)), out),
        _ => (None, None),
    }
}

fn arrow_cells(path:&std::path::Path)->Result<(),Box<dyn std::error::Error>> {
    use std::io::{Read,Write};
    let mut bytes=Vec::new();std::fs::File::open(path)?.take(33554433).read_to_end(&mut bytes)?;
    if bytes.len()>33554432{return Err("part_bytes".into());}
    let decoded=oo_pipeline::native_arrow::decode_arrow_stream(&bytes).map_err(|_|"arrow")?;
    let mut out=std::io::BufWriter::new(std::io::stdout().lock());
    for row in 0..decoded.rows {
        let cells=decoded.columns.iter().map(|column|oo_pipeline::multipart_cells::text_cell(&column[row])).collect::<Result<Vec<_>,_>>()?;
        serde_json::to_writer(&mut out,&cells)?;out.write_all(b"\n")?;
    }
    out.flush()?;Ok(())
}
