use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeploymentMode {
    #[default]
    PrimaryReplica,
    AllInOne,
}

impl DeploymentMode {
    pub fn from_installed(value: &Value) -> Result<Self, &'static str> {
        let mode = match value.get("deployment_mode") {
            None => Self::PrimaryReplica,
            Some(Value::String(mode)) if mode == "primary_replica" => Self::PrimaryReplica,
            Some(Value::String(mode)) if mode == "all_in_one" => Self::AllInOne,
            _ => return Err("deployment_mode"),
        };
        if mode == Self::AllInOne
            && ["replica_pg", "replica", "standby", "replica_endpoint"]
                .iter()
                .any(|key| value.get(key).is_some())
        {
            return Err("all_in_one_replica_conflict");
        }
        Ok(mode)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrimaryReplica => "primary_replica",
            Self::AllInOne => "all_in_one",
        }
    }

    pub fn readonly_replica(self) -> bool {
        self == Self::PrimaryReplica
    }

    pub fn query_limits(self, value: &Value) -> Result<(usize, u64), &'static str> {
        let (concurrency, timeout) = match self {
            Self::PrimaryReplica => (256, 30_000),
            Self::AllInOne => (4, 5_000),
        };
        let limit = |key: &str, default: u64| match value.get(key) {
            None => Ok(default),
            Some(value) => value.as_u64().filter(|v| *v > 0).ok_or("data_limits"),
        };
        let concurrency = limit("data_query_concurrency", concurrency)?;
        let timeout = limit("data_query_timeout_ms", timeout)?;
        if concurrency > 256 || timeout > 30_000 {
            return Err("data_limits");
        }
        if self == Self::AllInOne && (concurrency >= 256 || timeout >= 30_000) {
            return Err("all_in_one_limits");
        }
        Ok((
            usize::try_from(concurrency).map_err(|_| "data_limits")?,
            timeout,
        ))
    }

    pub fn readiness_record(
        self,
        role: &str,
        source_generation: u64,
        pack_digest: &str,
        gateway_container: &str,
        private_root: &str,
    ) -> Value {
        serde_json::json!({"ready":true,"deployment_mode":self.as_str(),"readonly_replica":self.readonly_replica(),"role":role,"source_generation":source_generation,"pack_digest":pack_digest,"gateway_container":gateway_container,"private_root":private_root})
    }

    pub fn matches_ready(self, ready: &Value) -> bool {
        ready["ready"] == true
            && ready["readonly_replica"] == self.readonly_replica()
            && match ready.get("deployment_mode") {
                None => self == Self::PrimaryReplica,
                Some(value) => value == self.as_str(),
            }
    }
}

#[cfg(test)]
mod deployment_tests {
    use super::DeploymentMode;
    use serde_json::json;
    #[test]
    fn installed_modes_are_closed_and_replica_conflicts_are_rejected() {
        assert_eq!(
            DeploymentMode::from_installed(&json!({})).unwrap(),
            DeploymentMode::PrimaryReplica
        );
        assert_eq!(
            DeploymentMode::from_installed(&json!({"deployment_mode":"primary_replica"})).unwrap(),
            DeploymentMode::PrimaryReplica
        );
        assert_eq!(
            DeploymentMode::from_installed(&json!({"deployment_mode":"all_in_one"})).unwrap(),
            DeploymentMode::AllInOne
        );
        for value in [
            json!({"deployment_mode":"unknown"}),
            json!({"deployment_mode":null}),
            json!({"deployment_mode":"all_in_one","replica_pg":{}}),
        ] {
            assert!(DeploymentMode::from_installed(&value).is_err());
        }
    }
    #[test]
    fn all_in_one_defaults_and_overrides_are_stricter() {
        let primary = DeploymentMode::PrimaryReplica
            .query_limits(&json!({}))
            .unwrap();
        let single = DeploymentMode::AllInOne.query_limits(&json!({})).unwrap();
        assert!(single.0 < primary.0 && single.1 < primary.1);
        assert_eq!(
            DeploymentMode::AllInOne
                .query_limits(&json!({"data_query_concurrency":1,"data_query_timeout_ms":500}))
                .unwrap(),
            (1, 500)
        );
        assert!(
            DeploymentMode::AllInOne
                .query_limits(&json!({"data_query_concurrency":256}))
                .is_err()
        );
        assert!(
            DeploymentMode::AllInOne
                .query_limits(&json!({"data_query_timeout_ms":30000}))
                .is_err()
        );
    }
}

#[cfg(test)]
mod deployment_readiness_tests {
    use super::DeploymentMode;
    use serde_json::json;
    #[test]
    fn readiness_cannot_cross_deployment_modes() {
        let replica =
            json!({"ready":true,"deployment_mode":"primary_replica","readonly_replica":true});
        let primary = json!({"ready":true,"deployment_mode":"all_in_one","readonly_replica":false});
        for mode in [DeploymentMode::PrimaryReplica, DeploymentMode::AllInOne] {
            let record = mode.readiness_record(
                "oo_r_reader",
                1,
                "digest",
                "oo-wf-allinone-gateway",
                "/private",
            );
            assert_eq!(record.as_object().unwrap().len(), 8);
            assert!(mode.matches_ready(&record));
            assert_eq!(record["deployment_mode"], mode.as_str());
            assert_eq!(record["readonly_replica"], mode.readonly_replica());
        }
        assert!(DeploymentMode::PrimaryReplica.matches_ready(&replica));
        assert!(!DeploymentMode::PrimaryReplica.matches_ready(&primary));
        assert!(DeploymentMode::AllInOne.matches_ready(&primary));
        assert!(!DeploymentMode::AllInOne.matches_ready(&replica));
    }
}
