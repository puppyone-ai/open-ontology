use crate::native_arrow::{DecodedStream, NativeCell};
use crate::native_c5_range::{KeyComponent, RangeScope, TypedKey};

impl RangeScope {
    pub fn validate_decoded_keys(
        &self,
        decoded: &DecodedStream,
        indexes: &[usize],
    ) -> Result<(), &'static str> {
        if u64::try_from(decoded.rows).ok() != Some(self.row_count) {
            return Err("c5_range_members");
        }
        self.validate_keys(&Self::decoded_keys(decoded, indexes)?)
    }

    pub fn decoded_keys(
        decoded: &DecodedStream,
        indexes: &[usize],
    ) -> Result<Vec<TypedKey>, &'static str> {
        if indexes.is_empty()
            || indexes.iter().any(|index| {
                decoded
                    .columns
                    .get(*index)
                    .is_none_or(|column| column.len() != decoded.rows)
            })
            || indexes
                .iter()
                .enumerate()
                .any(|(i, index)| indexes[..i].contains(index))
        {
            return Err("c5_range_members");
        }
        let mut keys: Vec<TypedKey> = Vec::with_capacity(decoded.rows);
        for row in 0..decoded.rows {
            let mut key = Vec::with_capacity(indexes.len());
            for index in indexes {
                let cell = decoded
                    .columns
                    .get(*index)
                    .filter(|column| column.len() == decoded.rows)
                    .and_then(|column| column.get(row))
                    .ok_or("c5_range_key")?;
                key.push(match cell {
                    NativeCell::Int { value, .. } => KeyComponent::Int(*value),
                    NativeCell::Utf8(value) => KeyComponent::Text(encode_hex(value.as_bytes())),
                    NativeCell::Binary(value) => KeyComponent::Bytes(encode_hex(value)),
                    _ => return Err("c5_range_key"),
                });
            }
            keys.push(key);
        }
        Ok(keys)
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
