//! A C5 watermark cannot be replaced by a C1 baseline without its explicit barrier.
pub async fn final_assert(
    tx:&mut sqlx::Transaction<'_,sqlx::Postgres>,source:&str,task:&str,
    generation:i64,source_generation:i64,
)->Result<(),&'static str> {
    let installed:bool=sqlx::query_scalar(
        "SELECT to_regprocedure('_oo.mysql_handoff_final_assert(text,uuid,bigint,bigint)') IS NOT NULL")
        .fetch_one(&mut **tx).await.map_err(|_|"mysql_handoff_target")?;
    if installed {
        sqlx::query("SELECT _oo.mysql_handoff_final_assert($1,$2::uuid,$3,$4)")
            .bind(source).bind(task).bind(generation).bind(source_generation)
            .execute(&mut **tx).await.map_err(|_|"mysql_handoff_final_barrier")?;
    } else {
        let c5:bool=sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM _oo.source_watermark WHERE source=$1 AND position_codec='snapshot-zero/v1')")
            .bind(source).fetch_one(&mut **tx).await.map_err(|_|"mysql_handoff_target")?;
        if c5 {return Err("mysql_handoff_install_required");}
    }
    Ok(())
}
