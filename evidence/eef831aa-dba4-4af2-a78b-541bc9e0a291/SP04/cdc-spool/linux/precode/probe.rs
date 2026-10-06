fn main() {
    let active: u32 = 1;
    let events: u32 = 500_000;
    let chunk_events: u32 = 8_192;
    let chunk_bytes: u32 = 4 * 1024 * 1024;
    let files: u32 = 96;
    let disk: u64 = 256 * 1024 * 1024;
    let line: u64 = 1024 * 1024;
    let ordinal_max: u32 = 32_767;
    assert_eq!(active, 1);
    assert_eq!(events, 500_000);
    assert_eq!(chunk_events, 8_192);
    assert_eq!(chunk_bytes, 4_194_304);
    assert_eq!(files, 96);
    assert_eq!(disk, 268_435_456);
    assert_eq!(line, 1_048_576);
    assert_eq!(ordinal_max, 32_767);
    let low = 0xB6E40u64;
    assert_eq!(low, 749_120);
    let lsn = (0x16u64 << 32) + low;
    assert_eq!(lsn, 94_490_029_632);
    let position = (1u128 << 64) + u128::from(lsn);
    assert_eq!(position.to_string(), "18446744168199581248");
    println!(
        "msrv_smoke_ok active={active} events={events} chunk_events={chunk_events} chunk_bytes={chunk_bytes} files={files} disk={disk} line={line} ordinal_max={ordinal_max} position={position}"
    );
}
