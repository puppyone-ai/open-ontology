fn main() {
    let active: u32 = 16;
    let txn_events: u32 = 200_000;
    let payload: u64 = 32 * 1024 * 1024;
    let line: u64 = 1024 * 1024;
    assert_eq!(active, 16);
    assert_eq!(txn_events, 200_000);
    assert_eq!(payload, 33_554_432);
    assert_eq!(line, 1_048_576);
    let low = 0xB6E40u64;
    assert_eq!(low, 749_120);
    let lsn = (22u64 << 32) + low;
    assert_eq!(lsn, 94_490_029_632);
    let position = (1u128 << 64) + u128::from(lsn);
    assert_eq!(position.to_string(), "18446744168199581248");
    let epoch_only = 1u128 << 64;
    assert_eq!(epoch_only.to_string(), "18446744073709551616");
    println!(
        "msrv_smoke_ok active={active} txn_events={txn_events} payload={payload} line={line} position={position}"
    );
}
