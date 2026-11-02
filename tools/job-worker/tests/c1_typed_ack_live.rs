use job_worker::c1_typed_ack::acknowledge;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[tokio::test]
#[ignore = "only the owned C1 20261027 fixture after c1_typed_owned test"]
async fn real_checkpoint_cas_precedes_ack() {
    let home = PathBuf::from(std::env::var("OO_C1_TYPED_HOME").expect("owned fixture opt-in"));
    let evidence = PathBuf::from(std::env::var("OO_C1_TYPED_EVIDENCE").expect("receipt directory"));
    let receipt: Value =
        serde_json::from_slice(&fs::read(evidence.join("last-receipt.json")).unwrap()).unwrap();
    let result: Value =
        serde_json::from_slice(&fs::read(evidence.join("live-result.json")).unwrap()).unwrap();
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(25487)
        .user("postgres")
        .password(
            fs::read_to_string(home.join("secrets/pg-super.pw"))
                .unwrap()
                .trim(),
        )
        .dbname(result["target_database"].as_str().unwrap());
    let (client, connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
    let task = tokio::spawn(async move {
        connection.await.unwrap();
    });
    client
        .batch_execute("SET ROLE oo_pipeline_runtime")
        .await
        .unwrap();
    let mut bad = receipt.clone();
    bad["digest"] = json!("0".repeat(64));
    assert_eq!(
        acknowledge(&client, &bad).await.unwrap_err(),
        "typed_checkpoint_conflict"
    );
    bad = receipt.clone();
    bad["generation"] = json!(2);
    assert_eq!(
        acknowledge(&client, &bad).await.unwrap_err(),
        "typed_checkpoint_unknown"
    );
    let ack = acknowledge(&client, &receipt).await.unwrap();
    assert_eq!(ack["protocol"], "oo.c1.typed-ack/v3");
    assert_eq!(ack["digest"], receipt["digest"]);
    assert_eq!(acknowledge(&client, &receipt).await.unwrap(), ack);
    fs::write(
        evidence.join("worker-ack.json"),
        serde_json::to_vec_pretty(&ack).unwrap(),
    )
    .unwrap();
    drop(client);
    task.await.unwrap();
}

#[tokio::test]
#[ignore = "owned Oracle buffer fixture; live claimed task required"]
async fn oracle_buffer_checkpoint_cas_precedes_ack() {
    use job_worker::c1_typed_ack::acknowledge_oracle;
    let receipt_path=PathBuf::from(std::env::var("OO_ORACLE_ACK_RECEIPT").unwrap());
    let receipt:Value=serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    let password=fs::read_to_string(std::env::var("OO_ORACLE_ACK_PASSWORD_FILE").unwrap()).unwrap();
    let mut config=tokio_postgres::Config::new();
    config.host("127.0.0.1").port(25487).user(std::env::var("OO_ORACLE_ACK_ROLE").unwrap())
        .password(password.trim()).dbname(std::env::var("OO_ORACLE_ACK_DATABASE").unwrap());
    let (client,connection)=config.connect(tokio_postgres::NoTls).await.unwrap();
    let task=tokio::spawn(async move { connection.await.unwrap(); });
    let source=receipt["source"].as_str().unwrap();
    let id=receipt["task"].as_str().unwrap();
    let generation=receipt["generation"].as_i64().unwrap();
    let epoch=receipt["epoch"].as_i64().unwrap() as i32;
    let sequence=receipt["sequence"].as_i64().unwrap();
    let digest=receipt["digest"].as_str().unwrap();
    // The evidence query itself refuses a target-only receipt before ACK CAS.
    assert!(client.query_one("SELECT _oo.c1_typed_checkpoint_evidence($1,$2::text::uuid,$3,$4,$5,$6)",
        &[&source,&id,&generation,&epoch,&sequence,&digest]).await.is_err());
    let mut bad=receipt.clone();bad["digest"]=json!("0".repeat(64));
    assert_eq!(acknowledge_oracle(&client,&bad).await.unwrap_err(),"typed_checkpoint_conflict");
    bad=receipt.clone();bad["generation"]=json!(generation+1);
    assert_eq!(acknowledge_oracle(&client,&bad).await.unwrap_err(),"typed_checkpoint_unknown");
    let ack=acknowledge_oracle(&client,&receipt).await.unwrap();
    assert_eq!(ack["checkpoint_kind"],"buffer_consumption");
    assert_eq!(ack["source_offset_write"],false);
    assert_eq!(ack["buffer_cursor"]["transaction_ordinal"],receipt["position"]);
    assert_eq!(acknowledge_oracle(&client,&receipt).await.unwrap(),ack);
    fs::write(receipt_path.with_file_name("worker-buffer-ack.json"),serde_json::to_vec_pretty(&ack).unwrap()).unwrap();
    drop(client);task.await.unwrap();
}
