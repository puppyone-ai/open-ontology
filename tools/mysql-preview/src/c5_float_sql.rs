//! MySQL 5.7 expression matching native_c5_float::canonical for FLOAT/DOUBLE.
//! This takes a field identifier, never an expression supplied by a caller.

pub fn canonical_expression(column: &str) -> Result<String, &'static str> {
    if column.is_empty()
        || column.len() > 64
        || !column
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("c5_float_identifier");
    }
    let field = format!("`{column}`");
    // JSON serialization retains a round-trippable double, unlike CAST AS CHAR.
    let atom = format!("JSON_EXTRACT(JSON_ARRAY({field}), '$[0]')");
    let text = format!("(CONVERT(LOWER(JSON_UNQUOTE({atom})) USING ascii) COLLATE ascii_bin)");
    let mantissa = format!("SUBSTRING_INDEX({text}, 'e', 1)");
    let digits = format!("REPLACE(REPLACE({mantissa}, '-', ''), '.', '')");
    let significant = format!("TRIM(BOTH '0' FROM {digits})");
    let exponent = format!(
        "(IF(LOCATE('e', {text}) > 0, CAST(SUBSTRING_INDEX({text}, 'e', -1) AS SIGNED), 0)\
         - IF(LOCATE('.', {mantissa}) > 0, LENGTH({mantissa}) - LOCATE('.', {mantissa}), 0)\
         + LENGTH({digits}) - LENGTH(TRIM(TRAILING '0' FROM {digits})))"
    );
    // Invalid non-NULL input raises a server error, never a NULL digest cell.
    // Keep the error branch row-dependent, rather than a constant-folded error.
    let refuse = format!(
        "(CONVERT(JSON_UNQUOTE(JSON_EXTRACT(CONCAT(_ascii'invalid-c5-float:', LENGTH({text})), '$[0]')) \
         USING ascii) COLLATE ascii_bin)"
    );
    Ok(format!(
        "(CASE WHEN {field} IS NULL THEN NULL \
         WHEN JSON_TYPE({atom}) <> 'DOUBLE' OR LENGTH({text}) > 64 \
           OR NOT (CAST({text} AS BINARY) REGEXP '^-?(0|[1-9][0-9]*)([.][0-9]+)?(e[+-]?[0-9]+)?$') \
           OR LENGTH({significant}) > 17 THEN {refuse} \
         WHEN {significant} = '' THEN CONCAT(IF(LEFT({text}, 1) = '-', _ascii'-', _ascii''), _ascii'0e0') \
         ELSE CONCAT(IF(LEFT({text}, 1) = '-', _ascii'-', _ascii''), {significant}, _ascii'e', \
           CAST({exponent} AS CHAR CHARACTER SET ascii) COLLATE ascii_bin) END)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_closed_field_identifiers_are_accepted() {
        for name in ["value", "FLOAT_VALUE_1", "_v", "0", &"a".repeat(64)] {
            let sql = canonical_expression(name).unwrap();
            assert!(sql.contains(&format!("`{name}`")));
            assert!(!sql.contains("FORMAT("));
            assert!(!sql.contains("ROUND("));
            assert!(!sql.contains("REGEXP_REPLACE"));
        }
        for name in [
            "",
            "a.b",
            "`a`",
            "a`);DROP TABLE x;--",
            "a b",
            "a\n",
            &"a".repeat(65),
        ] {
            assert_eq!(canonical_expression(name), Err("c5_float_identifier"));
        }
    }
}
