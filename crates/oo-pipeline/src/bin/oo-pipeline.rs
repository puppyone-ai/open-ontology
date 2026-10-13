use std::ffi::OsStr;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let _argv0 = args.next();
    match args.next() {
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
        Some(command) if command == OsStr::new("install-sealed-catalog") && args.next().is_none() => {
            install_sealed_catalog()
        }
        Some(command) if command == OsStr::new("import-native") && args.next().is_none() => {
            import_native()
        }
        Some(command) if command == OsStr::new("catalog-import-native") && args.next().is_none() => {
            catalog_import_native()
        }
        Some(command) if command == OsStr::new("install-native-sealed-catalog") && args.next().is_none() => {
            install_native_sealed_catalog()
        }
        Some(command) if command == OsStr::new("import-jsonb-document") && args.next().is_none() => {
            import_jsonb_document()
        }
        Some(command) if command == OsStr::new("import-native-update") && args.next().is_none() => {
            import_native_update()
        }
        Some(command) if command == OsStr::new("ack-native-partition") && args.next().is_none() => {
            ack_native_partition()
        }
        Some(command) if command == OsStr::new("recover-native-partition-mirror") && args.next().is_none() => {
            recover_native_partition_mirror()
        }
        Some(command) if command == OsStr::new("sync-native-task-mirror") && args.next().is_none() => {
            sync_native_task_mirror()
        }
        Some(command) if command == OsStr::new("install-native-mirror-rebind") && args.next().is_none() => {
            install_native_mirror_rebind()
        }
        Some(command) if command == OsStr::new("install-native-claim-selected") && args.next().is_none() => {
            install_native_claim_selected()
        }
        Some(command) if command == OsStr::new("install-native-update") && args.next().is_none() => {
            install_native_update()
        }
        Some(_) | None => {
            eprintln!("oo-pipeline: usage");
            ExitCode::from(2)
        }
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
