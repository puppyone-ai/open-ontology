//! Explicit scheduled-cycle intake. A sync command must never become a snapshot.
use std::collections::BTreeMap;

pub(super) fn arguments(args: impl Iterator<Item = String>) -> (bool, Vec<String>) {
    let mut args = args.peekable();
    let sync = args.peek().is_some_and(|arg| arg == "sync");
    if sync {
        args.next();
    }
    (sync, args.collect())
}

pub(super) fn validate(sync: bool, opts: &BTreeMap<String, String>) -> Result<(), &'static str> {
    if let Some(id) = opts.get("--run-id") {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err("sync_run_id");
        }
    }
    if sync {
        let modes = ["--sync", "--c5", "--handoff"]
            .iter()
            .filter(|mode| opts.contains_key(**mode))
            .count();
        if modes != 1 || !opts.contains_key("--run-id") {
            return Err("sync_mode_and_run_id_required");
        }
    }
    Ok(())
}

pub(super) fn validate_engine(
    engine: &str,
    opts: &BTreeMap<String, String>,
) -> Result<(), &'static str> {
    if matches!(
        engine,
        "mongodb"
            | "opensearch"
            | "elasticsearch"
            | "db2"
            | "kingbase"
            | "sqlserver"
            | "mariadb"
            | "tidb"
            | "oceanbase"
    ) && opts.contains_key("--c5")
        && !opts.contains_key("--sync")
        && !opts.contains_key("--handoff")
    {
        return Ok(());
    }
    if (opts.contains_key("--handoff") && engine != "mysql")
        || (["--c5", "--sync"]
            .iter()
            .any(|mode| opts.contains_key(*mode))
            && !matches!(engine, "postgres" | "mysql" | "oracle"))
    {
        return Err("sync_engine_not_supported");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciliation_engines_accept_only_explicit_c5() {
        for engine in [
            "mongodb",
            "opensearch",
            "elasticsearch",
            "db2",
            "kingbase",
            "sqlserver",
            "mariadb",
            "tidb",
            "oceanbase",
        ] {
            let mut opts = BTreeMap::from([("--c5".into(), "cycle.json".into())]);
            assert!(validate_engine(engine, &opts).is_ok());
            opts.insert("--sync".into(), "sync.json".into());
            assert!(validate_engine(engine, &opts).is_err());
            opts.remove("--sync");
            opts.insert("--handoff".into(), "handoff.json".into());
            assert!(validate_engine(engine, &opts).is_err());
        }
    }

    #[test]
    fn only_the_leading_subcommand_is_consumed() {
        for (input, expected, rest) in [
            (
                vec!["sync", "--stack", "sync"],
                true,
                vec!["--stack", "sync"],
            ),
            (vec!["--stack", "sync"], false, vec!["--stack", "sync"]),
        ] {
            let (sync, args) = arguments(input.into_iter().map(str::to_owned));
            assert_eq!(sync, expected);
            assert_eq!(args, rest);
        }
    }

    #[test]
    fn scheduled_cycles_require_one_explicit_mode_and_identity() {
        let mut options = BTreeMap::new();
        assert!(validate(true, &options).is_err());
        assert!(validate(false, &options).is_ok());
        options.insert("--run-id".into(), "cycle-01".into());
        assert!(validate(true, &options).is_err());
        for mode in ["--c5", "--sync", "--handoff"] {
            options.insert(mode.into(), "mode.json".into());
            assert!(validate(true, &options).is_ok());
            options.remove(mode);
        }
        options.insert("--c5".into(), "c5.json".into());
        options.insert("--sync".into(), "c1.json".into());
        assert!(validate(true, &options).is_err());
    }

    #[test]
    fn run_id_is_bounded_and_cannot_contain_paths_or_control_text() {
        for id in ["", "../cycle", "cycle\n1", "cycle 1", &"a".repeat(129)] {
            let options = BTreeMap::from([("--run-id".into(), id.into())]);
            assert!(validate(false, &options).is_err(), "{id:?}");
        }
    }

    #[test]
    fn snapshot_only_engines_cannot_silently_accept_sync_flags() {
        for mode in ["--c5", "--sync", "--handoff"] {
            let opts = BTreeMap::from([(mode.into(), "mode.json".into())]);
            for engine in ["auto", "unknown"] {
                assert_eq!(
                    validate_engine(engine, &opts),
                    Err("sync_engine_not_supported")
                );
            }
            assert!(validate_engine("mysql", &opts).is_ok());
            for engine in ["postgres", "oracle"] {
                assert_eq!(validate_engine(engine, &opts).is_ok(), mode != "--handoff");
            }
        }
        assert!(validate_engine("sqlserver", &BTreeMap::new()).is_ok());
    }
}
