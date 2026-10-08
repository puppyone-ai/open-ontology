//! Post sealed frames to the collector. The only argument is `--config`.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use gateway_audit_wal::{deliver_sealed, load_send_config, DeliveryStop, DiskWal, WalConfig};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let flag = args.next();
    let path = args.next();
    if flag.as_deref() != Some("--config") || args.next().is_some() {
        eprintln!("usage");
        return ExitCode::from(2);
    }
    let Some(path) = path else {
        eprintln!("usage");
        return ExitCode::from(2);
    };
    let config = match load_send_config(Path::new(&path)) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("config={}", err.name());
            return ExitCode::from(1);
        }
    };
    let mut wal = match DiskWal::open(&config.spool, WalConfig::default()) {
        Ok(wal) => wal,
        Err(err) => {
            eprintln!("spool={}", err.name());
            return ExitCode::from(1);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("runtime=unavailable");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(deliver_sealed(&mut wal, &config)) {
        Ok(DeliveryStop::Delivered { acked }) => {
            println!("delivered={acked}");
            ExitCode::SUCCESS
        }
        Ok(DeliveryStop::Down) => {
            println!("retained");
            ExitCode::from(75)
        }
        Ok(DeliveryStop::Conflict) => {
            println!("conflict");
            ExitCode::from(76)
        }
        Err(err) => {
            eprintln!("send={}", err.name());
            ExitCode::from(1)
        }
    }
}
