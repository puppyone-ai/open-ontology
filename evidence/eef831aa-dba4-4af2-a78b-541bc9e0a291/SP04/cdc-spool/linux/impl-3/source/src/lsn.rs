//! Canonical PostgreSQL commit LSN and `epoch << 64 | commit_lsn` position.

use crate::model::FaultCode;

pub fn parse_commit_lsn(text: &str) -> Result<u64, FaultCode> {
    let Some((high, low)) = text.split_once('/') else {
        return Err(FaultCode::CommitLsnInvalid);
    };
    if low.contains('/') {
        return Err(FaultCode::CommitLsnInvalid);
    }
    let high = parse_side(high)?;
    let low = parse_side(low)?;
    let shifted = u64::from(high)
        .checked_shl(32)
        .ok_or(FaultCode::CommitLsnInvalid)?;
    shifted
        .checked_add(u64::from(low))
        .ok_or(FaultCode::CommitLsnInvalid)
}

pub fn encode_position(epoch: u32, commit_lsn: u64) -> Result<String, FaultCode> {
    let shifted = u128::from(epoch)
        .checked_shl(64)
        .ok_or(FaultCode::CommitPositionOverflow)?;
    let value = shifted
        .checked_add(u128::from(commit_lsn))
        .ok_or(FaultCode::CommitPositionOverflow)?;
    let text = value.to_string();
    if text.is_empty() || text.len() > 38 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FaultCode::CommitPositionOverflow);
    }
    Ok(text)
}

fn parse_side(side: &str) -> Result<u32, FaultCode> {
    if side.is_empty() || side.len() > 8 {
        return Err(FaultCode::CommitLsnInvalid);
    }
    if side.len() > 1 && side.starts_with('0') {
        return Err(FaultCode::CommitLsnInvalid);
    }
    if !side
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'A'..=b'F'))
    {
        return Err(FaultCode::CommitLsnInvalid);
    }
    u32::from_str_radix(side, 16).map_err(|_| FaultCode::CommitLsnInvalid)
}

#[cfg(test)]
mod tests {
    use super::{encode_position, parse_commit_lsn};

    #[test]
    fn hand_lsn_and_position() {
        assert_eq!(parse_commit_lsn("16/B6E40").unwrap(), 94_490_029_632);
        assert_eq!(
            encode_position(1, 94_490_029_632).unwrap(),
            "18446744168199581248"
        );
        assert_eq!(encode_position(0, 0).unwrap(), "0");
    }
}
