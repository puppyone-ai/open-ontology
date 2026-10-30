//! Read-only compatibility checks against the native Loader's compiled artifact policy.
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const PROTOCOL: &str = "oo.pipeline.native-runtime-preflight/v1";
const MAX_REQUEST: u64 = 65_536;
const HASH_BUFFER: usize = 65_536;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    driver: Artifact,
    validator: Artifact,
}

impl Artifact {
    fn valid(&self) -> bool {
        Path::new(&self.path).is_absolute()
            && !self.path.contains('\0')
            && self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
}

fn stream_hash(reader: impl Read, expected_len: u64) -> Result<String, &'static str> {
    let mut reader = reader.take(
        expected_len
            .checked_add(1)
            .ok_or("artifact_digest_mismatch")?,
    );
    let mut buffer = [0u8; HASH_BUFFER];
    let mut hash = Sha256::new();
    let mut count = 0u64;
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|_| "artifact_unreadable")?;
        if n == 0 {
            break;
        }
        count += n as u64;
        hash.update(&buffer[..n]);
    }
    if count != expected_len {
        return Err("artifact_digest_mismatch");
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn inspect(artifact: &Artifact, validator: bool) -> Value {
    let mut reasons = Vec::new();
    let pinned_approved = crate::native_binding::approved_admission_bin(&artifact.sha256);
    let mut result = json!({
        "path":artifact.path,"sha256":artifact.sha256,"size_bytes":null,
        "length_allowed":null,"actual_sha256":null,"digest_matches":null
    });
    if validator {
        result["pinned_digest_approved"] = json!(pinned_approved);
        result["actual_digest_approved"] = Value::Null;
    }
    let checked = (|| -> Result<(), &'static str> {
        let before = fs::metadata(&artifact.path).map_err(|_| "artifact_unreadable")?;
        if !before.is_file() {
            return Err("artifact_unreadable");
        }
        result["size_bytes"] = json!(before.len());
        let allowed = crate::native_binding::driver_length_allowed(&artifact.sha256, before.len());
        result["length_allowed"] = json!(allowed);
        if !allowed {
            return Err("driver_resource_limit");
        }
        let mut file = File::open(&artifact.path).map_err(|_| "artifact_unreadable")?;
        let opened = file.metadata().map_err(|_| "artifact_unreadable")?;
        if !opened.is_file() || opened.len() != before.len() {
            return Err("artifact_digest_mismatch");
        }
        let actual = stream_hash(&mut file, opened.len())?;
        let after = file.metadata().map_err(|_| "artifact_unreadable")?;
        if after.len() != opened.len() || after.modified().ok() != opened.modified().ok() {
            return Err("artifact_digest_mismatch");
        }
        result["actual_sha256"] = json!(actual);
        result["digest_matches"] = json!(actual == artifact.sha256);
        if validator {
            let approved = crate::native_binding::approved_admission_bin(&actual);
            result["actual_digest_approved"] = json!(approved);
        }
        if actual != artifact.sha256 {
            return Err("artifact_digest_mismatch");
        }
        Ok(())
    })();
    if let Err(reason) = checked {
        reasons.push(reason);
    }
    if validator
        && !result["actual_digest_approved"]
            .as_bool()
            .unwrap_or(pinned_approved)
    {
        reasons.push("validator_not_approved");
    }
    result["reasons"] = json!(reasons);
    result
}

fn invalid_request() -> Value {
    json!({"protocol":PROTOCOL,"compatible":false,"reasons":["invalid_request"]})
}

/// Does not open a database/source connection or execute either supplied artifact.
/// This diagnostic does not replace the Loader's checks at admission time.
pub fn check(document: &Value) -> Value {
    let Ok(request) = serde_json::from_value::<Request>(document.clone()) else {
        return invalid_request();
    };
    if request.protocol != PROTOCOL || !request.driver.valid() || !request.validator.valid() {
        return invalid_request();
    }
    let driver = inspect(&request.driver, false);
    let validator = inspect(&request.validator, true);
    let mut reasons = Vec::new();
    for artifact in [&driver, &validator] {
        for reason in artifact["reasons"].as_array().into_iter().flatten() {
            if !reasons.contains(reason) {
                reasons.push(reason.clone());
            }
        }
    }
    json!({"protocol":PROTOCOL,"compatible":reasons.is_empty(),"reasons":reasons,
        "driver":driver,"validator":validator})
}

pub fn stdio() -> Result<(), u8> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_REQUEST + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| 2u8)?;
    let result = if bytes.len() as u64 > MAX_REQUEST {
        invalid_request()
    } else {
        serde_json::from_slice(&bytes).map_or_else(|_| invalid_request(), |value| check(&value))
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &result).map_err(|_| 1u8)?;
    stdout.write_all(b"\n").map_err(|_| 1u8)?;
    stdout.flush().map_err(|_| 1u8)?;
    if result["compatible"] == true {
        Ok(())
    } else {
        eprintln!(
            "oo-pipeline: native_runtime_preflight {}",
            result["reasons"]
        );
        Err(if result["reasons"] == json!(["invalid_request"]) {
            2
        } else {
            1
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Cursor,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static SERIAL: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "oo-native-runtime-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn artifact(&self, name: &str, content: &[u8]) -> Value {
            let path = self.0.join(name);
            fs::write(&path, content).unwrap();
            json!({"path":path,"sha256":format!("{:x}",Sha256::digest(content))})
        }
        fn request(&self) -> Value {
            json!({"protocol":PROTOCOL,
                "driver":self.artifact("driver",b"owned-driver-fixture"),
                "validator":self.artifact("validator",b"owned-validator-fixture")})
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn hashes_files_without_executing_them_or_trusting_a_validator_pin() {
        let fixture = Fixture::new();
        let marker = fixture.0.join("executed");
        let script = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
        let artifact = fixture.artifact("must-not-execute", script.as_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                artifact["path"].as_str().unwrap(),
                fs::Permissions::from_mode(0o500),
            )
            .unwrap();
        }
        let request = json!({"protocol":PROTOCOL,"driver":artifact,"validator":artifact});
        let result = check(&request);
        assert_eq!(result["compatible"], false);
        assert_eq!(result["reasons"], json!(["validator_not_approved"]));
        for role in ["driver", "validator"] {
            assert_eq!(result[role]["actual_sha256"], artifact["sha256"]);
            assert_eq!(result[role]["digest_matches"], true);
            assert_eq!(result[role]["length_allowed"], true);
        }
        assert!(!marker.exists());
        assert_eq!(
            fs::read(artifact["path"].as_str().unwrap()).unwrap(),
            script.as_bytes()
        );
    }

    #[test]
    fn distinguishes_digest_mismatch_from_resource_and_validator_rejections() {
        let fixture = Fixture::new();
        let mut request = fixture.request();
        request["driver"]["sha256"] = json!("f".repeat(64));
        let result = check(&request);
        assert_eq!(
            result["driver"]["reasons"],
            json!(["artifact_digest_mismatch"])
        );
        assert_eq!(result["driver"]["digest_matches"], false);
        assert_eq!(
            result["validator"]["reasons"],
            json!(["validator_not_approved"])
        );
    }

    #[test]
    fn large_artifacts_are_streamed_and_still_require_matching_content_hashes() {
        let fixture = Fixture::new();
        let request = fixture.request();
        for role in ["driver", "validator"] {
            File::options()
                .write(true)
                .open(request[role]["path"].as_str().unwrap())
                .unwrap()
                .set_len(32 * 1024 * 1024 + 1)
                .unwrap();
        }
        let result = check(&request);
        for role in ["driver", "validator"] {
            assert_eq!(result[role]["length_allowed"], true);
            assert!(result[role]["actual_sha256"].is_string());
            assert_eq!(result[role]["digest_matches"], false);
            assert!(
                result[role]["reasons"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("artifact_digest_mismatch"))
            );
        }
        assert_eq!(result["compatible"], false);
    }

    #[test]
    fn approved_digest_still_requires_its_compiled_exact_length_and_real_content_hash() {
        let fixture = Fixture::new();
        let mut request = fixture.request();
        let approved = crate::native_binding::ADMISSION_BIN_SHA256_RELEASE_1;
        request["validator"]["sha256"] = json!(approved);
        let result = check(&request);
        let len = fs::metadata(request["validator"]["path"].as_str().unwrap())
            .unwrap()
            .len();
        assert_eq!(
            result["validator"]["pinned_digest_approved"],
            crate::native_binding::approved_admission_bin(approved)
        );
        assert_eq!(
            result["validator"]["length_allowed"],
            crate::native_binding::driver_length_allowed(approved, len)
        );
        assert_eq!(result["compatible"], false);
        // Match this existing approved artifact's length, not its bytes.
        assert!(crate::native_binding::driver_length_allowed(
            approved, 5_654_928
        ));
        File::options()
            .write(true)
            .open(request["validator"]["path"].as_str().unwrap())
            .unwrap()
            .set_len(5_654_928)
            .unwrap();
        let result = check(&request);
        assert_eq!(result["validator"]["length_allowed"], true);
        assert_eq!(result["validator"]["digest_matches"], false);
        assert_eq!(result["validator"]["actual_digest_approved"], false);
        assert!(
            result["validator"]["reasons"]
                .as_array()
                .unwrap()
                .contains(&json!("artifact_digest_mismatch"))
        );
    }

    #[test]
    fn hashing_has_a_fixed_buffer_and_a_metadata_length_bound() {
        struct Reader {
            read: usize,
            largest: usize,
        }
        impl Read for Reader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.largest = self.largest.max(buffer.len());
                buffer.fill(0);
                self.read += buffer.len();
                Ok(buffer.len())
            }
        }
        let mut reader = Reader {
            read: 0,
            largest: 0,
        };
        assert_eq!(
            stream_hash(&mut reader, 3 * HASH_BUFFER as u64),
            Err("artifact_digest_mismatch")
        );
        assert_eq!(reader.read, 3 * HASH_BUFFER + 1);
        assert!(reader.largest <= HASH_BUFFER);
        assert_eq!(
            stream_hash(Cursor::new(b"short"), 6),
            Err("artifact_digest_mismatch")
        );
        assert_eq!(
            stream_hash(Cursor::new(b"content"), 7).unwrap(),
            format!("{:x}", Sha256::digest(b"content"))
        );
    }

    #[test]
    fn malformed_or_unreadable_inputs_fail_closed() {
        let fixture = Fixture::new();
        let request = fixture.request();
        for (pointer, value) in [
            ("/protocol", json!("wrong")),
            ("/driver/path", json!("relative")),
            ("/driver/sha256", json!("A".repeat(64))),
            ("/validator/sha256", json!("not-a-digest")),
        ] {
            let mut invalid = request.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert_eq!(check(&invalid), invalid_request());
        }
        let mut invalid = request.clone();
        invalid["target"] = json!({"database":"must-not-connect"});
        assert_eq!(check(&invalid), invalid_request());
        invalid = request.clone();
        invalid["driver"]["args"] = json!(["must-not-execute"]);
        assert_eq!(check(&invalid), invalid_request());
        for path in [fixture.0.clone(), fixture.0.join("missing")] {
            let mut missing = request.clone();
            missing["driver"]["path"] = json!(path);
            let result = check(&missing);
            assert_eq!(result["driver"]["reasons"], json!(["artifact_unreadable"]));
            assert_eq!(result["compatible"], false);
        }
    }
}
