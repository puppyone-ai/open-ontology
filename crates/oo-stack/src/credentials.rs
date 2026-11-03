//! First-start lite credentials. Never rotates or overwrites existing secrets.
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::secret::SecretStore;

const SERVICES: [&str; 8] = [
    "agent-db",
    "control-db",
    "worker",
    "builder",
    "gateway",
    "model-egress",
    "scheduler",
    "admin",
];
const MARKER: &str = "lite-credentials";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CredentialStatus {
    pub protocol: String,
    /// Exact OpenSSL notAfter value of the earliest CA or leaf.
    pub nearest_credential_expiry: String,
    pub nearest_credential: String,
    pub expires_within_14_days: bool,
    pub expired: bool,
    /// Database passwords are not Gateway tokens. Token expiry needs the store.
    pub token_expiry_measured: bool,
}

fn openssl(args: &[&str], input: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut child = Command::new("openssl")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "LITE_OPENSSL_START")?;
    let written = child
        .stdin
        .take()
        .ok_or("LITE_OPENSSL_STDIN")
        .and_then(|mut stdin| stdin.write_all(input).map_err(|_| "LITE_OPENSSL_STDIN"));
    if written.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("LITE_OPENSSL_STDIN");
    }
    let out = child.wait_with_output().map_err(|_| "LITE_OPENSSL_WAIT")?;
    if !out.status.success() {
        return Err("LITE_OPENSSL_FAILED");
    }
    Ok(out.stdout)
}

fn random_hex() -> Result<String, &'static str> {
    let mut bytes = [0; 32];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| "LITE_RANDOM")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn location(root: &Path, name: &str) -> Result<String, &'static str> {
    root.join(name)
        .to_str()
        .map(str::to_owned)
        .ok_or("LITE_SECRET_PATH")
}

fn checkend(root: &Path, name: &str, seconds: &str) -> Result<bool, &'static str> {
    let path = location(root, name)?;
    let out = Command::new("openssl")
        .args(["x509", "-in", &path, "-noout", "-checkend", seconds])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "LITE_OPENSSL_START")?;
    match out.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err("LITE_OPENSSL_FAILED"),
    }
}

fn date_key(value: &str) -> Result<(u32, usize, u32, String), &'static str> {
    let parts: Vec<_> = value.split_whitespace().collect();
    if parts.len() != 5 || parts[4] != "GMT" {
        return Err("LITE_CERTIFICATE_DATE");
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == parts[0])
    .ok_or("LITE_CERTIFICATE_DATE")?;
    Ok((
        parts[3].parse().map_err(|_| "LITE_CERTIFICATE_DATE")?,
        month,
        parts[1].parse().map_err(|_| "LITE_CERTIFICATE_DATE")?,
        parts[2].into(),
    ))
}

/// Inspect only; a missing or incomplete bootstrap is not healthy.
pub fn status(root: &Path) -> Result<CredentialStatus, &'static str> {
    let store = SecretStore::open(root)?;
    let marker: serde_json::Value =
        serde_json::from_str(&store.read(MARKER)?).map_err(|_| "LITE_CREDENTIAL_MARKER")?;
    if marker != serde_json::json!({"protocol":"oo.lite-credentials/v1","days":730}) {
        return Err("LITE_CREDENTIAL_MARKER");
    }
    for name in [
        "lite-ca-key",
        "lite-ca-cert",
        "lite-admin-password",
        "lite-bootstrap-lock",
    ] {
        store.read(name)?;
    }
    let mut certificates = vec!["lite-ca-cert".to_owned()];
    for service in SERVICES {
        store.read(&format!("lite-{service}-key"))?;
        store.read(&format!("lite-{service}-cert"))?;
        certificates.push(format!("lite-{service}-cert"));
    }
    let mut dates = Vec::new();
    for name in certificates {
        let key_name = name
            .strip_suffix("-cert")
            .ok_or("LITE_CERTIFICATE")?
            .to_owned()
            + "-key";
        let public_key = openssl(
            &["pkey", "-in", &location(root, &key_name)?, "-pubout"],
            b"",
        )?;
        let cert_key = openssl(
            &["x509", "-in", &location(root, &name)?, "-pubkey", "-noout"],
            b"",
        )?;
        if public_key != cert_key {
            return Err("LITE_CERTIFICATE_KEY_MISMATCH");
        }
        // Expired certificates remain reportable; broken trust does not.
        openssl(
            &[
                "verify",
                "-no_check_time",
                "-CAfile",
                &location(root, "lite-ca-cert")?,
                &location(root, &name)?,
            ],
            b"",
        )?;
        let expiry = openssl(
            &["x509", "-in", &location(root, &name)?, "-noout", "-enddate"],
            b"",
        )?;
        let expiry = String::from_utf8(expiry).map_err(|_| "LITE_CERTIFICATE")?;
        let expiry = expiry
            .trim()
            .strip_prefix("notAfter=")
            .ok_or("LITE_CERTIFICATE")?
            .to_owned();
        dates.push((date_key(&expiry)?, name, expiry));
    }
    dates.sort();
    let (_, name, expiry) = dates.first().ok_or("LITE_CERTIFICATE")?;
    Ok(CredentialStatus {
        protocol: "oo.lite-credentials/v1".into(),
        nearest_credential_expiry: expiry.clone(),
        nearest_credential: name.clone(),
        expires_within_14_days: checkend(root, name, "1209600")?,
        expired: checkend(root, name, "0")?,
        token_expiry_measured: false,
    })
}

/// Creates a private CA, distinct mTLS identities and random admin password.
/// A failed partial bootstrap deliberately requires operator recovery; retry
/// never rotates a CA underneath any component that may already trust it.
pub fn bootstrap(root: &Path) -> Result<CredentialStatus, &'static str> {
    let store = SecretStore::create(root)?;
    if root.join(MARKER).symlink_metadata().is_ok() {
        return status(root);
    }
    store.write_new(
        "lite-bootstrap-lock",
        b"in progress; do not rotate implicitly",
    )?;
    store.write_new(
        "lite-ca-key",
        &openssl(&["genpkey", "-algorithm", "ED25519"], b"")?,
    )?;
    let ca_key = location(root, "lite-ca-key")?;
    let ca_cert = location(root, "lite-ca-cert")?;
    store.write_new(
        "lite-ca-cert",
        &openssl(
            &[
                "req",
                "-new",
                "-x509",
                "-key",
                &ca_key,
                "-days",
                "730",
                "-subj",
                "/CN=Open Ontology lite internal CA",
                "-addext",
                "basicConstraints=critical,CA:TRUE,pathlen:0",
                "-addext",
                "keyUsage=critical,keyCertSign,cRLSign",
            ],
            b"",
        )?,
    )?;
    for service in SERVICES {
        let key = format!("lite-{service}-key");
        store.write_new(&key, &openssl(&["genpkey", "-algorithm", "ED25519"], b"")?)?;
        let csr = openssl(
            &[
                "req",
                "-new",
                "-key",
                &location(root, &key)?,
                "-subj",
                &format!("/CN={service}"),
            ],
            b"",
        )?;
        let extensions = format!(
            "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\n\
             extendedKeyUsage=serverAuth,clientAuth\nsubjectAltName=DNS:{service},DNS:localhost,IP:127.0.0.1\n"
        );
        let ext = format!("lite-{service}-extensions");
        store.write_new(&ext, extensions.as_bytes())?;
        let cert = openssl(
            &[
                "x509",
                "-req",
                "-CA",
                &ca_cert,
                "-CAkey",
                &ca_key,
                "-set_serial",
                &format!("0x{}", &random_hex()?[..32]),
                "-days",
                "730",
                "-extfile",
                &location(root, &ext)?,
            ],
            &csr,
        )?;
        store.write_new(&format!("lite-{service}-cert"), &cert)?;
    }
    store.write_new("lite-admin-password", random_hex()?.as_bytes())?;
    store.write_new(
        MARKER,
        br#"{"protocol":"oo.lite-credentials/v1","days":730}"#,
    )?;
    status(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn bootstrap_is_private_verifiable_and_idempotent() {
        let parent = std::env::temp_dir().join(format!("oo-lite-certs-{}", std::process::id()));
        fs::create_dir(&parent).unwrap();
        let root = parent.join("secrets");
        let status = bootstrap(&root).unwrap();
        assert!(!status.expired && !status.expires_within_14_days);
        assert!(!status.token_expiry_measured);
        let store = SecretStore::open(&root).unwrap();
        let key = store.read("lite-ca-key").unwrap();
        assert_eq!(bootstrap(&root).unwrap(), status);
        assert_eq!(store.read("lite-ca-key").unwrap(), key);
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for entry in fs::read_dir(&root).unwrap() {
            assert_eq!(
                entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        for service in SERVICES {
            assert!(!checkend(&root, &format!("lite-{service}-cert"), "62985600").unwrap());
            assert!(checkend(&root, &format!("lite-{service}-cert"), "63072001").unwrap());
            for purpose in ["sslserver", "sslclient"] {
                assert!(
                    openssl(
                        &[
                            "verify",
                            "-CAfile",
                            &location(&root, "lite-ca-cert").unwrap(),
                            "-purpose",
                            purpose,
                            &location(&root, &format!("lite-{service}-cert")).unwrap()
                        ],
                        b""
                    )
                    .is_ok()
                );
            }
        }
        let csr = openssl(
            &[
                "req",
                "-new",
                "-key",
                &location(&root, "lite-worker-key").unwrap(),
                "-subj",
                "/CN=worker",
            ],
            b"",
        )
        .unwrap();
        let short = openssl(
            &[
                "x509",
                "-req",
                "-CA",
                &location(&root, "lite-ca-cert").unwrap(),
                "-CAkey",
                &location(&root, "lite-ca-key").unwrap(),
                "-set_serial",
                "2",
                "-days",
                "1",
                "-extfile",
                &location(&root, "lite-worker-extensions").unwrap(),
            ],
            &csr,
        )
        .unwrap();
        fs::write(root.join("lite-worker-cert"), short).unwrap();
        let warning = super::status(&root).unwrap();
        assert_eq!(warning.nearest_credential, "lite-worker-cert");
        assert!(warning.expires_within_14_days && !warning.expired);
        // `ca` supports fixed validity dates on both OpenSSL 1.1 and 3+;
        // `x509 -days 0` is rejected by newer releases.
        fs::write(parent.join("index"), "").unwrap();
        fs::write(parent.join("serial"), "03\n").unwrap();
        let ca_config = format!(
            "[ca]\ndefault_ca=local\n[local]\ndatabase={0}/index\nserial={0}/serial\n\
             new_certs_dir={0}\nprivate_key={1}\ncertificate={2}\ndefault_md=sha256\n\
             policy=policy\n[policy]\ncommonName=supplied\n",
            parent.display(),
            location(&root, "lite-ca-key").unwrap(),
            location(&root, "lite-ca-cert").unwrap(),
        );
        fs::write(parent.join("ca.cnf"), ca_config).unwrap();
        let expired = openssl(
            &[
                "ca",
                "-batch",
                "-notext",
                "-in",
                "/dev/stdin",
                "-out",
                "/dev/stdout",
                "-config",
                parent.join("ca.cnf").to_str().unwrap(),
                "-startdate",
                "990101000000Z",
                "-enddate",
                "000101000000Z",
            ],
            &csr,
        )
        .unwrap();
        fs::write(root.join("lite-worker-cert"), expired).unwrap();
        assert!(super::status(&root).unwrap().expired);
        let original = store.read("lite-worker-key").unwrap();
        fs::write(
            root.join("lite-worker-key"),
            store.read("lite-admin-key").unwrap(),
        )
        .unwrap();
        assert_eq!(
            super::status(&root).unwrap_err(),
            "LITE_CERTIFICATE_KEY_MISMATCH"
        );
        fs::write(root.join("lite-worker-key"), original).unwrap();
        fs::remove_file(root.join("lite-worker-key")).unwrap();
        symlink(root.join("lite-ca-key"), root.join("lite-worker-key")).unwrap();
        assert!(bootstrap(&root).is_err());
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn partial_bootstrap_never_overwrites_or_recovers_implicitly() {
        let parent = std::env::temp_dir().join(format!("oo-lite-partial-{}", std::process::id()));
        fs::create_dir(&parent).unwrap();
        let root = parent.join("secrets");
        let store = SecretStore::create(&root).unwrap();
        store
            .write_new("lite-ca-key", b"existing-private-material")
            .unwrap();
        assert!(bootstrap(&root).is_err());
        assert_eq!(
            store.read("lite-ca-key").unwrap(),
            "existing-private-material"
        );
        assert!(status(&root).is_err());
        fs::remove_dir_all(parent).unwrap();
    }
}
