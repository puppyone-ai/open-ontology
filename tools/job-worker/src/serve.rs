use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use crate::api::{worker_id, PROOF_PROJECT};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::session;
use crate::tls;
use crate::worker::{run_worker, Gate, WorkerIn};

pub struct ServeOpts {
    pub workers: u16,
    pub barrier_prefix: String,
    pub barrier_target: usize,
}

pub async fn serve(config: &Config, opts: ServeOpts) -> Result<()> {
    if !(1..=100).contains(&opts.workers) || opts.barrier_target > usize::from(opts.workers) {
        return Err(Error::new("input"));
    }
    if !opts.barrier_prefix.is_empty()
        && (!(1..=32).contains(&opts.barrier_prefix.len())
            || !opts
                .barrier_prefix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    {
        return Err(Error::new("input"));
    }
    let listen = tls::connect_listening(
        &config.host,
        config.port,
        &config.database,
        &config.runtime_user,
        &config.runtime_password,
        &config.ca_pem,
    )
    .await?;
    listen
        .client
        .batch_execute("LISTEN oo_jw_wake")
        .await
        .map_err(Error::sql)?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let (release_tx, release_rx) = watch::channel(false);
    let gate = Arc::new(Gate {
        target: opts.barrier_target,
        prefix: if opts.barrier_prefix.is_empty() {
            None
        } else {
            Some(opts.barrier_prefix)
        },
        arrived: AtomicUsize::new(0),
        release: release_tx,
    });
    let mut clients = Vec::with_capacity(usize::from(opts.workers));
    for _ in 0..opts.workers {
        clients.push(session::runtime(config).await?);
    }
    let (err_tx, mut err_rx) = tokio::sync::mpsc::channel(1);
    let mut handles = Vec::with_capacity(clients.len());
    for (index, mut client) in clients.into_iter().enumerate() {
        let input = WorkerIn {
            project_id: PROOF_PROJECT.to_string(),
            worker: worker_id(u16::try_from(index).map_err(|_| Error::new("input"))?),
            stop: stop_rx.clone(),
            wake: listen.wake.clone(),
            gate: Arc::clone(&gate),
            gate_view: release_rx.clone(),
        };
        let errors = err_tx.clone();
        handles.push(tokio::spawn(async move {
            if let Err(err) = run_worker(&mut client, input).await {
                let _ = errors.try_send(err);
            }
        }));
    }
    drop(err_tx);
    eprintln!("event=serve workers={}", opts.workers);
    let mut terminate = signal(SignalKind::terminate()).map_err(|_| Error::new("signal"))?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(|_| Error::new("signal"))?;
    let mut signal_stop = false;
    let mut failed = None;
    tokio::select! {
        _ = terminate.recv() => {
            signal_stop = true;
        }
        _ = interrupt.recv() => {
            signal_stop = true;
        }
        maybe = err_rx.recv() => {
            if let Some(err) = maybe {
                failed = Some(err);
            }
        }
    }
    let _ = stop_tx.send(true);
    let failed = join_workers(handles, failed).await;
    drop(listen);
    if signal_stop {
        eprintln!("event=serve_stop");
        return Ok(());
    }
    match failed {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

async fn join_workers(
    handles: Vec<tokio::task::JoinHandle<()>>,
    mut failed: Option<Error>,
) -> Option<Error> {
    for handle in handles {
        if handle.await.is_err() && failed.is_none() {
            failed = Some(Error::new("worker"));
        }
    }
    failed
}
