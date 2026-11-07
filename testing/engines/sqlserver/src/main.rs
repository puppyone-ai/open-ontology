use arrow_array::{Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use oo_sqlserver_conformance::{
    SourceError,
    engine::*,
    sqlserver::{ChangeMode, ConnectConfig, SqlServerEngine},
};
use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    port: u16,
    password_file: PathBuf,
    ca_file: PathBuf,
    output: PathBuf,
    mode: Option<ChangeMode>,
    prior: Option<PathBuf>,
    #[serde(default)]
    barrier: bool,
    table: Option<String>,
    database: Option<String>,
    user: Option<String>,
    #[serde(default)]
    position_only: bool,
}
struct Sink(Vec<RecordBatch>);
impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.0.push(batch);
        Ok(())
    }
}
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{}", serde_json::json!({"status":"failed","error":e}));
        std::process::exit(1);
    }
}
async fn run() -> Result<(), String> {
    let input: Input = serde_json::from_slice(
        &fs::read(std::env::args().nth(1).ok_or("config_required")?).map_err(|_| "config_read")?,
    )
    .map_err(|_| "config_decode")?;
    let table = TableRef {
        schema: "dbo".into(),
        name: input.table.unwrap_or_else(|| "fixture".into()),
    };
    let budget = ReadBudget {
        max_chunk_rows: 4,
        statement_timeout_ms: 60_000,
        ..Default::default()
    };
    let config = ConnectConfig {
        host: "localhost".into(),
        port: input.port,
        database: input.database.unwrap_or_else(|| "oo_fixture".into()),
        user: input.user.unwrap_or_else(|| "oo_reader".into()),
        password: zeroize::Zeroizing::new(
            fs::read_to_string(input.password_file).map_err(|_| "secret_read")?,
        ),
        ca_pem: fs::read(input.ca_file).map_err(|_| "ca_read")?,
        tables: vec![table.clone()],
        mode: input.mode,
    };
    let mut engine = SqlServerEngine::connect(config, budget.clone())
        .await
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(&input.output).map_err(|_| "output_create")?;
    let probe = engine.probe().await.map_err(|e| e.to_string())?;
    let position = engine.watermark().await.map_err(|e| e.to_string())?;
    fs::write(
        input.output.join("position.json"),
        serde_json::to_vec(&position).unwrap(),
    )
    .map_err(|_| "output_write")?;
    if input.position_only {
        println!("{}", serde_json::json!({"probe":probe,"position":position}));
        return Ok(());
    }
    if let Some(prior) = input.prior {
        let from: Watermark =
            serde_json::from_slice(&fs::read(prior).map_err(|_| "position_read")?)
                .map_err(|_| "position_decode")?;
        let batch = engine
            .next_batch(&from, &budget)
            .await
            .map_err(|e| e.to_string())?;
        println!("{}", serde_json::json!({"probe":probe,"changes":batch}));
        return Ok(());
    }
    let plan = engine
        .plan_chunks(&table, &budget)
        .await
        .map_err(|e| e.to_string())?;
    if input.barrier {
        fs::write(input.output.join("ready"), b"ready").map_err(|_| "barrier_write")?;
        let started = std::time::Instant::now();
        while !input.output.join("go").exists() {
            if started.elapsed().as_secs() > 60 {
                return Err("writer_barrier_timeout".into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
    let mut receipts = Vec::new();
    let mut digests = Vec::new();
    let mut values = Vec::new();
    for chunk in &plan.chunks {
        let mut sink = Sink(Vec::new());
        receipts.push(
            engine
                .read_chunk(chunk, &budget, &mut sink)
                .await
                .map_err(|e| e.to_string())?,
        );
        digests.push(
            engine
                .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .map_err(|e| e.to_string())?,
        );
        let file = fs::File::create(input.output.join(format!("{}.arrow", chunk.chunk_id)))
            .map_err(|_| "ipc_create")?;
        let mut writer =
            StreamWriter::try_new(file, &sink.0[0].schema()).map_err(|_| "ipc_schema")?;
        for batch in sink.0 {
            let ids = batch
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or("id_type")?;
            let texts = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or("value_type")?;
            for row in 0..batch.num_rows() {
                values.push(serde_json::json!([
                    ids.value(row),
                    if texts.is_null(row) {
                        None
                    } else {
                        Some(texts.value(row))
                    }
                ]));
            }
            writer.write(&batch).map_err(|_| "ipc_write")?;
        }
        writer.finish().map_err(|_| "ipc_finish")?;
    }
    let same_session_changes = match position {
        Some(ref from) => engine
            .next_batch(from, &budget)
            .await
            .map_err(|e| e.to_string())?,
        None => None,
    };
    println!(
        "{}",
        serde_json::json!({"probe":probe,"plan":plan,"receipts":receipts,"digests":digests,"budget":budget,"values":values,"resources":engine.resource_receipt(),"same_session_changes":same_session_changes})
    );
    Ok(())
}
