//! Target storage used by the chunk loader, not the legacy COPY importer.
use crate::{SourceError, types::ArrowPhysical};

pub(crate) fn target_type(physical: &ArrowPhysical) -> Result<String, SourceError> {
    use ArrowPhysical::*;
    Ok(match physical {
        Boolean => "boolean".into(),
        Int16 => "smallint".into(),
        Int32 => "integer".into(),
        Int64 => "bigint".into(),
        Float64 => "double precision".into(),
        Utf8 | DecimalUtf8 { .. } => "text".into(),
        Binary => "bytea".into(),
        Date32 => "date".into(),
        DurationMicros => "interval".into(),
        TimestampMicrosNaive => "timestamp".into(),
        TimestampMicrosUtc => "timestamptz".into(),
        Decimal128 { precision, scale } | Decimal256 { precision, scale } => {
            if *scale < 0 {
                format!("numeric({},0)", i16::from(*precision) - i16::from(*scale))
            } else {
                format!("numeric({precision},{scale})")
            }
        }
        List(item) if **item == Utf8 => "json".into(),
        _ => return Err(SourceError::catalog("unsupported_target_type")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_storage_does_not_preserve_source_varchar_limits() {
        assert_eq!(
            target_type(&ArrowPhysical::Decimal256 {
                precision: 40,
                scale: 12
            })
            .unwrap(),
            "numeric(40,12)"
        );
        assert_eq!(target_type(&ArrowPhysical::Utf8).unwrap(), "text");
        assert_eq!(target_type(&ArrowPhysical::Int32).unwrap(), "integer");
        assert_eq!(target_type(&ArrowPhysical::Int64).unwrap(), "bigint");
        assert_eq!(
            target_type(&ArrowPhysical::Decimal128 {
                precision: 19,
                scale: 0
            })
            .unwrap(),
            "numeric(19,0)"
        );
        assert_eq!(
            target_type(&ArrowPhysical::Decimal128 {
                precision: 10,
                scale: -2
            })
            .unwrap(),
            "numeric(12,0)"
        );
    }
}
