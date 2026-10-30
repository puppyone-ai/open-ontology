use job_worker::native_schedule::c5plus_policy::{Config, LayeredConfig, Progress};

fn config() -> Config {
    serde_json::from_value(serde_json::json!({
        "period_ms": [1000, 2000, 3000, 4000],
        "duty_per_mille": 250,
        "threads_running_max": 8,
        "execution_window_utc": [1380, 60]
    }))
    .unwrap()
}

fn layered_value() -> serde_json::Value {
    serde_json::json!({"policy":config(),"window_ms":60000,"overlap_ms":30000,
        "recent_column":"changed_at","server_timeout_ms":5000,
        "client_timeout_ms":10000,"execution_timezone":"UTC"})
}

#[test]
fn incremental_only_defaults_false_and_preserves_legacy_pins() {
    let legacy = layered_value();
    let config: LayeredConfig = serde_json::from_value(legacy.clone()).unwrap();
    assert!(!config.incremental_only);
    assert_eq!(serde_json::to_value(&config).unwrap(), legacy);
    assert_eq!(
        config.due(10_000, 0, 0, &Progress::default()).unwrap(),
        [true; 4]
    );
    let mut value = legacy.clone();
    value["incremental_only"] = serde_json::json!(false);
    let explicit: LayeredConfig = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(explicit, config);
    assert_eq!(serde_json::to_value(&explicit).unwrap(), legacy);
    value["incremental_only"] = serde_json::json!(true);
    let enabled: LayeredConfig = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&enabled).unwrap(), value);
    for bad in [
        serde_json::json!("true"),
        serde_json::json!(1),
        serde_json::Value::Null,
    ] {
        value["incremental_only"] = bad;
        assert!(serde_json::from_value::<LayeredConfig>(value.clone()).is_err());
    }
}

#[test]
fn incremental_only_preserves_l1_l3_deadlines_without_advancing_l4() {
    let mut value = layered_value();
    value["incremental_only"] = serde_json::json!(true);
    let config: LayeredConfig = serde_json::from_value(value).unwrap();
    let progress = Progress::default();
    assert_eq!(
        config.due(0, 0, 0, &progress).unwrap(),
        [true, true, true, false]
    );
    let progress = Progress {
        completed_at_ms: [Some(10_000), Some(10_000), Some(10_000), None],
    };
    let original = progress.clone();
    for (now, expected) in [
        (10_001, [false, false, false, false]),
        (11_000, [true, false, false, false]),
        (12_000, [true, true, false, false]),
        (13_000, [true, true, true, false]),
        (99_000, [true, true, true, false]),
    ] {
        assert_eq!(config.due(now, 0, 0, &progress).unwrap(), expected);
    }
    assert_eq!(progress, original);
    assert_eq!(progress.completed_at_ms[3], None);
    assert_eq!(
        config.due(99_000, 720, 0, &progress).unwrap(),
        [true, true, false, false]
    );
    assert_eq!(config.due(99_000, 0, 9, &progress).unwrap(), [false; 4]);
    let mut historical = progress.clone();
    historical.completed_at_ms[3] = Some(1);
    let original = historical.clone();
    assert!(!config.due(99_000, 0, 0, &historical).unwrap()[3]);
    assert_eq!(historical, original);
    let mut maintenance = config;
    maintenance.incremental_only = false;
    assert!(maintenance.due(99_000, 0, 0, &historical).unwrap()[3]);
}

#[test]
fn independent_deadlines_and_positive_hints_do_not_starve_scans() {
    let c = config();
    let p = Progress {
        completed_at_ms: [Some(9000), Some(8000), Some(0), Some(0)],
    };
    assert_eq!(c.due(10_000, 0, 8, &p).unwrap(), [true; 4]);
    let p = Progress {
        completed_at_ms: [Some(9999), Some(9999), None, None],
    };
    assert_eq!(c.due(10_000, 0, 8, &p).unwrap(), [false, false, true, true]);
}

#[test]
fn expensive_layers_obey_overnight_window_and_load() {
    let c = config();
    let p = Progress::default();
    assert_eq!(c.due(0, 1380, 8, &p).unwrap(), [true; 4]);
    assert_eq!(c.due(0, 59, 8, &p).unwrap(), [true; 4]);
    assert_eq!(c.due(0, 60, 8, &p).unwrap(), [true, true, false, false]);
    assert_eq!(c.due(0, 720, 8, &p).unwrap(), [true, true, false, false]);
    assert_eq!(c.due(0, 0, 9, &p).unwrap(), [false; 4]);
}

#[test]
fn duty_sleep_rounds_up_and_never_wraps() {
    let c = config();
    assert_eq!(c.rest_ms(500).unwrap(), 1500);
    let mut c = c;
    c.duty_per_mille = 300;
    assert_eq!(c.rest_ms(1).unwrap(), 3);
    assert!(c.rest_ms(u64::MAX).is_err());
}

#[test]
fn malformed_limits_and_future_checkpoints_fail_closed() {
    let mut c = config();
    let p = Progress {
        completed_at_ms: [Some(1), None, None, None],
    };
    assert!(c.due(0, 0, 0, &p).is_err());
    assert!(c.due(0, 1440, 0, &Progress::default()).is_err());
    c.period_ms[2] = 0;
    assert!(c.validate().is_err());
    c = config();
    c.execution_window_utc = [0, 0];
    assert!(c.validate().is_err());
    c = config();
    c.duty_per_mille = 0;
    assert!(c.validate().is_err());
    c.duty_per_mille = 1001;
    assert!(c.validate().is_err());
    c = config();
    c.threads_running_max = 0;
    assert!(c.validate().is_err());
}

#[test]
fn daytime_and_all_day_windows_have_explicit_endpoints() {
    let mut c = config();
    let p = Progress::default();
    c.execution_window_utc = [60, 120];
    assert_eq!(c.due(0, 59, 0, &p).unwrap(), [true, true, false, false]);
    assert_eq!(c.due(0, 60, 0, &p).unwrap(), [true; 4]);
    assert_eq!(c.due(0, 120, 0, &p).unwrap(), [true, true, false, false]);
    c.execution_window_utc = [0, 1440];
    assert_eq!(c.due(0, 0, 0, &p).unwrap(), [true; 4]);
    assert_eq!(c.due(0, 1439, 0, &p).unwrap(), [true; 4]);
    c.execution_window_utc = [1440, 0];
    assert!(c.validate().is_err());
    c.execution_window_utc = [0, 1441];
    assert!(c.validate().is_err());
}

#[test]
fn progress_round_trip_preserves_each_layer_deadline() {
    let c = config();
    let p = Progress {
        completed_at_ms: [Some(0), Some(1), None, Some(3)],
    };
    let restored: Progress = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(p, restored);
    assert_eq!(
        c.due(2001, 0, 0, &restored).unwrap(),
        [true, true, true, false]
    );
    assert!(
        serde_json::from_value::<Config>(serde_json::json!({
            "period_ms": [1, 2, 3, 4], "duty_per_mille": 250,
            "threads_running_max": 8, "execution_window_utc": [0, 1440],
            "untrusted_override": true
        }))
        .is_err()
    );
    let mut c = c;
    c.duty_per_mille = 1000;
    assert_eq!(c.rest_ms(u64::MAX).unwrap(), 0);
    c.duty_per_mille = 1;
    assert_eq!(c.rest_ms(1).unwrap(), 999);
}

#[test]
fn layered_installation_config_requires_explicit_timezone_and_budgets() {
    use job_worker::native_schedule::c5plus_policy::LayeredConfig;
    let mut value = serde_json::json!({"policy":config(), "window_ms":60000,
        "overlap_ms":30000,"recent_column":"changed_at", "server_timeout_ms":5000,
        "client_timeout_ms":10000,"execution_timezone":"UTC"});
    let c: LayeredConfig = serde_json::from_value(value.clone()).unwrap();
    assert!(c.validate().is_ok());
    value["recent_column"] = serde_json::json!("x".repeat(65));
    assert!(
        serde_json::from_value::<LayeredConfig>(value.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    value["recent_column"] = serde_json::json!("biz");
    value["execution_timezone"] = serde_json::json!("local");
    assert!(
        serde_json::from_value::<LayeredConfig>(value.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    value["execution_timezone"] = serde_json::json!("UTC");
    value["client_timeout_ms"] = serde_json::json!(4000);
    assert!(
        serde_json::from_value::<LayeredConfig>(value)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn binding_default_is_byte_compatible_and_layered_cannot_leak_into_c1() {
    use job_worker::native_schedule::Manifest;
    let base = serde_json::json!({"protocol":"oo.jobs.native-schedule/v1","binding":"00000000-0000-4000-8000-000000000001","project_id":"00000000-0000-4000-8000-000000000002","source_id":"fixture","method":"C5","cycle":"reconcile","period_ms":1000,"producer":{"path":"/producer","sha256":"a".repeat(64),"args":[]},"loader":{"path":"/loader","sha256":"b".repeat(64),"args":["import-native-update"]}});
    let mut manifest: Manifest = serde_json::from_value(base.clone()).unwrap();
    assert!(manifest.validate_c5().is_ok());
    assert_eq!(serde_json::to_value(&manifest).unwrap(), base);
    manifest.c5_mode = Some("layered".into());
    assert!(manifest.validate_c5().is_err());
    manifest.c5_layered=Some(serde_json::from_value(serde_json::json!({"policy":config(),"window_ms":60000,"overlap_ms":10000,"recent_column":"biz","server_timeout_ms":5000,"client_timeout_ms":10000,"execution_timezone":"UTC"})).unwrap());
    assert!(manifest.validate_c5().is_ok());
    manifest.method = "C1".into();
    assert!(manifest.validate_c5().is_err());
    manifest.method = "C5".into();
    manifest.c5_mode = Some("auto".into());
    assert!(manifest.validate_c5().is_err());
}
