use crate::fail::Fail;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastRequest {
    pub source_precision: u8,
    pub source_scale: u8,
    pub target_precision: u8,
    pub target_scale: u8,
}

pub fn reject_narrow_cast(request: CastRequest) -> Result<(), Fail> {
    if request.target_precision < request.source_precision || request.target_scale < request.source_scale
    {
        return Err(Fail::new(
            "UNSAFE_CAST",
            format!(
                "DECIMAL({},{}) to DECIMAL({},{}) is blocked before SQL",
                request.source_precision,
                request.source_scale,
                request.target_precision,
                request.target_scale
            ),
        ));
    }
    Ok(())
}

pub fn reject_cursor(cursor: &str) -> Result<(), Fail> {
    let folded = cursor.trim().to_ascii_lowercase();
    if folded.is_empty() || folded == "none" || folded == "snapshot" {
        return Ok(());
    }
    Err(Fail::new("CURSOR_UNSUPPORTED", cursor.to_string()))
}

/// Returns the business statement only after the cursor and cast guards pass.
pub fn business_statement(cursor: &str, cast: Option<CastRequest>, statement: &str) -> Result<String, Fail> {
    reject_cursor(cursor)?;
    if let Some(cast) = cast {
        reject_narrow_cast(cast)?;
    }
    if statement.to_ascii_uppercase().contains("FINAL") {
        return Err(Fail::new("final_keyword", "Doris SELECT is not rewritten with FINAL"));
    }
    Ok(statement.to_string())
}

#[cfg(test)]
mod tests {
    use super::{business_statement, reject_cursor, reject_narrow_cast, CastRequest};

    fn narrow() -> CastRequest {
        CastRequest {
            source_precision: 38,
            source_scale: 18,
            target_precision: 10,
            target_scale: 2,
        }
    }

    #[test]
    fn blocks_narrow_cast_and_stream_cursor_before_sql() {
        let err = business_statement("snapshot", Some(narrow()), "SELECT id, d38 FROM dec_exact").unwrap_err();
        assert_eq!(err.code, "UNSAFE_CAST");
        let err = business_statement("stream", None, "SELECT id FROM dec_exact").unwrap_err();
        assert_eq!(err.code, "CURSOR_UNSUPPORTED");
        assert!(reject_cursor("binlog").unwrap_err().code == "CURSOR_UNSUPPORTED");
        assert!(reject_cursor("gtid:1").is_err());
        assert!(reject_narrow_cast(narrow()).is_err());
        let sql = business_statement("snapshot", None, "SELECT id, d38 FROM dec_exact").unwrap();
        assert_eq!(sql, "SELECT id, d38 FROM dec_exact");
    }
}
