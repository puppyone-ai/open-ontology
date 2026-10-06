fn main() {
    let idle_ms: u64 = 1_800_000;
    let absolute_ms: u64 = 28_800_000;
    let turn_ms: u64 = 600_000;
    let tools: u32 = 60;
    assert_eq!(idle_ms, 30 * 60 * 1000);
    assert_eq!(absolute_ms, 8 * 60 * 60 * 1000);
    assert_eq!(turn_ms, 10 * 60 * 1000);
    assert_eq!(tools, 60);
    println!("msrv_smoke_ok idle={idle_ms} absolute={absolute_ms} turn={turn_ms} tools={tools}");
}
