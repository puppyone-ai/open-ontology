//! Full selected source/target equality, independent of capture and Loader output.
use crate::{io::*, mysql::source_sql};
use serde_json::{Value, json};

fn normalized(v: &Value, kind: &str) -> Result<Value> {
    if v.is_null() {
        return Ok(Value::Null);
    }
    let s = v.as_str().ok_or("source_cell_text")?;
    Ok(match kind {
        "decimal" => json!(decimal(s)?),
        "json" => serde_json::from_str(s)?,
        "time" => json!(time(s)?),
        "varbinary" | "binary" => json!(s.to_ascii_lowercase()),
        "date" | "datetime" | "timestamp" => json!(if s.contains('.') {
            s.replace('T', " ")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        } else {
            s.replace('T', " ")
        }),
        _ => json!(s),
    })
}
fn decimal(s: &str) -> Result<String> {
    let negative = s.starts_with('-');
    let s = s
        .strip_prefix('-')
        .or_else(|| s.strip_prefix('+'))
        .unwrap_or(s);
    let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
    require(
        !whole.is_empty()
            && whole
                .bytes()
                .chain(fraction.bytes())
                .all(|b| b.is_ascii_digit()),
        "decimal_text",
    )?;
    let whole = whole.trim_start_matches('0');
    let fraction = fraction.trim_end_matches('0');
    let mut result = if whole.is_empty() {
        "0".into()
    } else {
        whole.to_owned()
    };
    if !fraction.is_empty() {
        result.push('.');
        result.push_str(fraction);
    }
    if negative && result != "0" {
        result.insert(0, '-');
    }
    Ok(result)
}
fn time(s: &str) -> Result<i128> {
    let sign = if s.starts_with('-') { -1 } else { 1 };
    let s = s.trim_start_matches('-');
    // PostgreSQL interval textual output preserves negative MySQL hours.
    let s = s.strip_prefix('+').unwrap_or(s);
    let p = s.split(':').collect::<Vec<_>>();
    require(p.len() == 3, "time_text")?;
    let (seconds, fraction) = p[2].split_once('.').unwrap_or((p[2], ""));
    require(
        fraction.len() <= 6 && fraction.bytes().all(|b| b.is_ascii_digit()),
        "time_precision",
    )?;
    let hours = p[0].parse::<i128>()?;
    let minutes = p[1].parse::<i128>()?;
    let seconds = seconds.parse::<i128>()?;
    require(
        (0..60).contains(&minutes) && (0..60).contains(&seconds),
        "time_range",
    )?;
    let micros = format!("{fraction:0<6}").parse::<i128>()?;
    Ok(sign * ((hours * 3600 + minutes * 60 + seconds) * 1_000_000 + micros))
}
pub fn compare(c: &Value, catalog: &Value, result: &Value, calls: &mut Calls) -> Result<Value> {
    let schema = ident(text(c, "database")?)?;
    let mut checked = Vec::new();
    let mut total = 0u64;
    let before = source_sql(c, calls, "SELECT @@GLOBAL.gtid_executed")?
        .trim()
        .to_owned();
    require(result["l_end"] == before, "oracle_source_changed")?;
    for (e, n) in array(catalog, "entries")?.iter().zip(array(
        &load(calls.out.join("native/request.json"))?,
        "native_requests",
    )?) {
        let t = &array(e, "tables")?[0];
        let table = ident(text(t, "logical_table")?)?;
        let types = source_sql(
            c,
            calls,
            &format!(
                "SELECT COLUMN_NAME,DATA_TYPE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{schema}' AND TABLE_NAME='{table}' ORDER BY ORDINAL_POSITION"
            ),
        )?;
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut kinds = Vec::new();
        let columns = array(t, "columns")?;
        for line in types.lines() {
            let mut pair = line.split('\t');
            let name = ident(pair.next().ok_or("column")?)?;
            let kind = pair.next().ok_or("kind")?;
            let Some(col) = columns.iter().find(|col| {
                col["logical_name"] == name || col["name"] == name || col["logical_column"] == name
            }) else {
                continue;
            };
            let physical = ident(text(col, "stored_physical")?)?;
            let quoted = format!("`{name}`");
            let mut expression = format!("CAST({quoted} AS CHAR)");
            if ["binary", "varbinary"].contains(&kind) {
                expression = format!("HEX({quoted})");
            }
            if ["date", "datetime"].contains(&kind) {
                expression = format!(
                    "CASE WHEN CAST({quoted} AS CHAR) REGEXP '^(0000-[0-9]{{2}}-[0-9]{{2}}|[0-9]{{4}}-00-[0-9]{{2}}|[0-9]{{4}}-[0-9]{{2}}-00)( |$)' THEN NULL ELSE {expression} END"
                );
            }
            left.push(expression);
            right.push(if ["binary", "varbinary"].contains(&kind) {
                format!("encode(\"{physical}\",'hex')")
            } else {
                format!("\"{physical}\"::text")
            });
            kinds.push(kind.to_owned());
        }
        require(
            left.len() == columns.len() && !left.is_empty(),
            "oracle_column_coverage",
        )?;
        let physical = ident(text(t, "stored_physical")?)?;
        let source_rows = source_sql(
            c,
            calls,
            &format!(
                "SELECT JSON_ARRAY({}) FROM `{schema}`.`{table}`",
                left.join(",")
            ),
        )?;
        let target_sql = format!(
            "SELECT json_build_array({}) FROM _oo.\"{physical}\"",
            right.join(",")
        );
        let target_rows = String::from_utf8(calls.docker(
            "typed-target",
            text(&n["target"], "host")?,
            &strings(&[
                "psql",
                "-U",
                "oo_admin",
                "-d",
                text(&n["target"], "database")?,
                "-Atc",
                &target_sql,
            ]),
            None,
        )?)?;
        let normalize_rows = |raw: &str| -> Result<Vec<String>> {
            let mut rows = Vec::new();
            for line in raw.lines() {
                let row: Value = serde_json::from_str(line)?;
                let cells = row.as_array().ok_or("row")?;
                require(cells.len() == kinds.len(), "row_width")?;
                let normalized = cells
                    .iter()
                    .zip(&kinds)
                    .map(|(cell, kind)| normalized(cell, kind))
                    .collect::<Result<Vec<_>>>()?;
                rows.push(serde_json::to_string(&normalized)?);
            }
            rows.sort();
            Ok(rows)
        };
        let left = normalize_rows(&source_rows)?;
        let right = normalize_rows(&target_rows)?;
        require(left == right, "all_cell_typed_difference")?;
        total += left.len() as u64;
        checked.push(json!({"dataset":format!("{schema}.{table}"),"rows":left.len(),"columns":kinds.len(),"different_cells":0}));
    }
    require(
        before == source_sql(c, calls, "SELECT @@GLOBAL.gtid_executed")?.trim(),
        "oracle_source_changed",
    )?;
    Ok(
        json!({"protocol":"oo.mysql.full-typed-oracle/v1","source_id":result["source_id"],"source_generation":result["source_generation"],"source_gtid":before,"tables":checked,"rows":total,"different_cells":0}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimal_and_signed_mysql_time() {
        assert_eq!(decimal("-000.0000").unwrap(), "0");
        assert_eq!(
            decimal("99999999999999999999.123400").unwrap(),
            "99999999999999999999.1234"
        );
        assert_eq!(time("-25:10:00.123").unwrap(), -90_600_123_000);
        assert_ne!(
            normalized(&Value::Null, "decimal").unwrap(),
            normalized(&json!("0"), "decimal").unwrap()
        );
    }
}
