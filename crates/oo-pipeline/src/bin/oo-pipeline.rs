use std::ffi::OsStr;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let _argv0 = args.next();
    match args.next() {
        Some(command)
            if command == OsStr::new("install-native-c5-privileged-observer")
                && args.next().is_none() =>
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match rt.block_on(oo_pipeline::native_update::c5_observer_install_stdio()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("install-native-c5-widening-rebase")
                && args.next().is_none() =>
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match rt.block_on(oo_pipeline::native_update::c5_widening_install_stdio()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("native-oracle-c1-materialize") && args.next().is_none() =>
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match rt.block_on(oo_pipeline::c1_oracle_materialize::stdio()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command) if command == OsStr::new("check-native-runtime") && args.next().is_none() => {
            match oo_pipeline::native_runtime::stdio() {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command) if command == OsStr::new("import-pg") && args.next().is_none() => import_pg(),
        Some(command) if command == OsStr::new("catalog-pg") && args.next().is_none() => {
            catalog_pg()
        }
        Some(command) if command == OsStr::new("catalog-import") && args.next().is_none() => {
            catalog_import()
        }
        Some(command) if command == OsStr::new("catalog-import-pg") && args.next().is_none() => {
            catalog_import_pg()
        }
        Some(command)
            if command == OsStr::new("install-sealed-catalog") && args.next().is_none() =>
        {
            install_sealed_catalog()
        }
        Some(command) if command == OsStr::new("import-native") && args.next().is_none() => {
            import_native()
        }
        Some(command)
            if command == OsStr::new("catalog-import-native") && args.next().is_none() =>
        {
            catalog_import_native()
        }
        Some(command)
            if command == OsStr::new("install-native-sealed-catalog") && args.next().is_none() =>
        {
            install_native_sealed_catalog()
        }
        Some(command)
            if command == OsStr::new("import-jsonb-document") && args.next().is_none() =>
        {
            import_jsonb_document()
        }
        Some(command) if command == OsStr::new("import-native-update") && args.next().is_none() => {
            import_native_update()
        }
        Some(command)
            if command == OsStr::new("import-native-transaction") && args.next().is_none() =>
        {
            import_native_transaction()
        }
        Some(command)
            if command == OsStr::new("import-native-c1-baseline") && args.next().is_none() =>
        {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match runtime.block_on(oo_pipeline::native_update::import_native_c1_baseline()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("import-native-c1-baseline-v2") && args.next().is_none() =>
        {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match runtime.block_on(oo_pipeline::native_update::import_native_c1_baseline_v2()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("import-native-c1-baseline-v3") && args.next().is_none() =>
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match rt.block_on(oo_pipeline::native_update::import_native_c1_baseline_v3()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if (command == OsStr::new("native-c5-part-stage")
                || command == OsStr::new("install-native-c5-range-parts"))
                && args.next().is_none() =>
        {
            let install = command == OsStr::new("install-native-c5-range-parts");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match rt.block_on(oo_pipeline::native_update::c5_part_stage_stdio(install)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("native-c1-typed-stage") && args.next().is_none() =>
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match rt.block_on(oo_pipeline::c1_typed_cli::stdio(false)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("install-native-c1-typed") && args.next().is_none() =>
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match rt.block_on(oo_pipeline::c1_typed_cli::stdio(true)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("native-c1-publication") && args.next().is_none() =>
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match rt.block_on(oo_pipeline::c1_publication_cli::stdio()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("native-c1-baseline-stage") && args.next().is_none() =>
        {
            native_c1_baseline_stage()
        }
        Some(command)
            if command == OsStr::new("native-c1-baseline-stage-v2") && args.next().is_none() =>
        {
            native_c1_baseline_stage_v2()
        }
        Some(command) if command == OsStr::new("ack-native-partition") && args.next().is_none() => {
            ack_native_partition()
        }
        Some(command)
            if command == OsStr::new("recover-native-partition-mirror")
                && args.next().is_none() =>
        {
            recover_native_partition_mirror()
        }
        Some(command)
            if command == OsStr::new("sync-native-task-mirror") && args.next().is_none() =>
        {
            sync_native_task_mirror()
        }
        Some(command)
            if command == OsStr::new("install-native-mirror-rebind") && args.next().is_none() =>
        {
            install_native_mirror_rebind()
        }
        Some(command)
            if command == OsStr::new("install-native-claim-selected") && args.next().is_none() =>
        {
            install_native_claim_selected()
        }
        Some(command)
            if command == OsStr::new("install-native-update") && args.next().is_none() =>
        {
            install_native_update()
        }
        Some(command) if command == OsStr::new("native-c5-state") && args.next().is_none() => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match runtime.block_on(oo_pipeline::native_c5_state::stdio()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("native-library-prepare") && args.next().is_none() =>
        {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            match runtime.block_on(oo_pipeline::native_library::stdio()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => ExitCode::from(code),
            }
        }
        Some(command)
            if command == OsStr::new("install-native-library-c1") && args.next().is_none() =>
        {
            install_native_extra(ExtraInstall::C1Library)
        }
        Some(command)
            if command == OsStr::new("install-native-c5-oracle-range") && args.next().is_none() =>
        {
            install_native_extra(ExtraInstall::OracleRange)
        }
        Some(command)
            if command == OsStr::new("install-native-c5-oracle-publication")
                && args.next().is_none() =>
        {
            install_native_extra(ExtraInstall::OraclePublication)
        }
        Some(command)
            if command == OsStr::new("install-native-oracle-c5-catalog")
                && args.next().is_none() =>
        {
            install_native_extra(ExtraInstall::OracleCatalog)
        }
        Some(_) | None => {
            eprintln!("oo-pipeline: usage");
            ExitCode::from(2)
        }
    }
}

enum ExtraInstall {
    C1Library,
    OracleRange,
    OraclePublication,
    OracleCatalog,
}

#[tokio::main(flavor = "current_thread")]
async fn install_native_extra(kind: ExtraInstall) -> ExitCode {
    use std::io::{Read, Write};
    let mut bytes = Vec::new();
    if std::io::stdin()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 1_048_576
    {
        return ExitCode::from(2);
    }
    let request: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return ExitCode::from(2),
    };
    if request["protocol"] != "oo.pipeline.install-native-update/v1"
        || request.as_object().is_none_or(|o| o.len() != 2)
        || !request["target"].is_object()
    {
        return ExitCode::from(2);
    }
    use oo_pipeline::target_installer as installer;
    let (result, version, digest) = match kind {
        ExtraInstall::OracleRange => (
            installer::install_native_c5_oracle_range_target(&request["target"]).await,
            installer::VERSION_NATIVE_C5_ORACLE_RANGE,
            installer::compiled_native_c5_oracle_range_digest(),
        ),
        ExtraInstall::OraclePublication => (
            installer::install_native_c5_oracle_publication_target(&request["target"]).await,
            installer::VERSION_NATIVE_C5_ORACLE_PUBLICATION,
            installer::compiled_native_c5_oracle_publication_digest(),
        ),
        ExtraInstall::OracleCatalog => (
            installer::install_native_oracle_c5_catalog_target(&request["target"]).await,
            installer::VERSION_NATIVE_ORACLE_C5_CATALOG,
            installer::compiled_native_oracle_c5_catalog_digest(),
        ),
        ExtraInstall::C1Library => (
            installer::install_native_library_c1_target(&request["target"]).await,
            installer::VERSION_NATIVE_LIBRARY_C1,
            installer::compiled_native_library_c1_digest(),
        ),
    };
    let output = match result {
        Ok(()) => serde_json::json!({"installed":true,
            "version":version,"digest":digest}),
        Err(reason) => serde_json::json!({"installed":false,"reason":reason}),
    };
    if writeln!(std::io::stdout(), "{output}").is_err() || output["installed"] != true {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[tokio::main(flavor = "current_thread")]
async fn import_pg() -> ExitCode {
    match oo_pipeline::import_cli::import_pg().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn catalog_pg() -> ExitCode {
    match oo_pipeline::catalog_cli::catalog_pg().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn catalog_import() -> ExitCode {
    match oo_pipeline::catalog_import::catalog_import().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn catalog_import_pg() -> ExitCode {
    match oo_pipeline::catalog_import::catalog_import_pg().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn install_sealed_catalog() -> ExitCode {
    match oo_pipeline::catalog_import::install_sealed_catalog_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn import_native() -> ExitCode {
    match oo_pipeline::native_arrow::import_native().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn catalog_import_native() -> ExitCode {
    match oo_pipeline::native_catalog::catalog_import_native().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn install_native_sealed_catalog() -> ExitCode {
    match oo_pipeline::native_catalog::install_native_sealed_catalog_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn import_jsonb_document() -> ExitCode {
    match oo_pipeline::jsonb_document::import_jsonb_document_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn import_native_update() -> ExitCode {
    match oo_pipeline::native_update::import_native_update().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn import_native_transaction() -> ExitCode {
    match oo_pipeline::native_update::import_native_transaction().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn native_c1_baseline_stage() -> ExitCode {
    match oo_pipeline::native_c1_baseline::run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn native_c1_baseline_stage_v2() -> ExitCode {
    match oo_pipeline::native_c1_baseline::run_v2().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn ack_native_partition() -> ExitCode {
    match oo_pipeline::native_update::ack_native_partition().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn recover_native_partition_mirror() -> ExitCode {
    match oo_pipeline::native_update::recover_native_partition_mirror().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn sync_native_task_mirror() -> ExitCode {
    match oo_pipeline::native_update::sync_native_task_mirror().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn install_native_mirror_rebind() -> ExitCode {
    match oo_pipeline::native_update::install_native_mirror_rebind_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn install_native_claim_selected() -> ExitCode {
    match oo_pipeline::native_update::install_native_claim_selected_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn install_native_update() -> ExitCode {
    match oo_pipeline::native_update::install_native_update_cli().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}
