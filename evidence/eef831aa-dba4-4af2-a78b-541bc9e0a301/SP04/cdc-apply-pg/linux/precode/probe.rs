fn main() {
    let limit: u32 = 200_000;
    let segment_cap: u32 = 500_000;
    assert_eq!(limit, 200_000);
    assert!(limit < segment_cap);
    let low = 0xB6E40u64;
    assert_eq!(low, 749_120);
    let lsn = (0x16u64 << 32) + low;
    assert_eq!(lsn, 94_490_029_632);
    let position = (1u128 << 64) + u128::from(lsn);
    assert_eq!(position.to_string(), "18446744168199581248");
    println!("msrv_smoke_ok limit={limit} position={position}");
}
