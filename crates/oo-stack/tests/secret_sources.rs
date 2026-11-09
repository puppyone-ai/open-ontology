use oo_stack::{secret::SecretStore, stack::secret_reference};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};

#[test]
fn explicit_sources_reject_ambiguous_or_traversing_references() {
    for value in ["model-key", "env:MODEL_KEY", "file:/run/secrets/key"] {
        assert!(secret_reference(value));
    }
    for value in [
        "env:",
        "env:1KEY",
        "env:KEY=value",
        "file:relative",
        "file:/a/../b",
        "file:/",
    ] {
        assert!(!secret_reference(value));
    }
}

#[test]
fn reads_environment_without_materializing_or_echoing_it() {
    const MARKER: &str = "OO_TEST_SECRET_SOURCE_CHILD";
    if let Some(root) = std::env::var_os(MARKER) {
        let store = SecretStore::open(root).unwrap();
        assert_eq!(
            store.read("env:OO_TEST_EXPLICIT_SECRET").unwrap(),
            "fixture-value"
        );
        assert_eq!(
            store.path("env:OO_TEST_EXPLICIT_SECRET").unwrap_err(),
            "SECRET_PATH_REQUIRED"
        );
        assert_eq!(
            store.read("env:OO_TEST_ABSENT_SECRET").unwrap_err(),
            "SECRET_ENV"
        );
        return;
    }
    let root = std::env::temp_dir().join(format!("oo-env-source-{}", std::process::id()));
    SecretStore::create(&root).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "reads_environment_without_materializing_or_echoing_it",
        ])
        .env(MARKER, &root)
        .env("OO_TEST_EXPLICIT_SECRET", "fixture-value")
        .env_remove("OO_TEST_ABSENT_SECRET")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-value"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-value"));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_dir(root).unwrap();
}

#[test]
fn external_files_are_read_only_and_never_copied() {
    let root = std::env::temp_dir().join(format!("oo-file-source-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let store = SecretStore::create(root.join("store")).unwrap();
    let file = root.join("external");
    fs::write(&file, "fixture-value").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o400)).unwrap();
    let reference = format!("file:{}", file.display());
    assert_eq!(store.read(&reference).unwrap(), "fixture-value");
    assert_eq!(store.path(&reference).unwrap(), file);
    assert_eq!(fs::read_dir(root.join("store")).unwrap().count(), 0);
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o400
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(store.read(&reference).unwrap_err(), "SECRET_FILE");
    let link = root.join("link");
    symlink(&file, &link).unwrap();
    assert!(store.read(&format!("file:{}", link.display())).is_err());
    fs::remove_dir_all(root).unwrap();
}
