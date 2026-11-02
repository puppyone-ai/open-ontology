//! Build cross-package producer CLIs once, rather than depending on an operator's PATH.
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

pub fn program(variable: &str) -> String {
    if let Ok(path) = std::env::var(variable) {
        return path;
    }
    if variable.ends_with("_REVIEWED_POLICY") {
        return env!("CARGO_BIN_EXE_reviewed-policy").to_string();
    }
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    let directory = BUILT.get_or_init(|| {
        let executable = std::env::current_exe().expect("test executable");
        let directory = executable.parent().unwrap().parent().unwrap().to_path_buf();
        let profile = directory.file_name().unwrap().to_str().unwrap();
        let mut command = Command::new(env!("CARGO"));
        command.args([
            "build",
            "--locked",
            "-p",
            "oo-builder",
            "--bin",
            "oo-builder",
            "--bin",
            "reviewed-effective-oos",
            "--profile",
            if profile == "debug" { "dev" } else { profile },
        ]);
        command.current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
        assert!(command.status().expect("build producer CLIs").success());
        directory
    });
    let name = if variable.ends_with("_OO_BUILDER") {
        "oo-builder"
    } else {
        assert!(
            variable.ends_with("_REVIEWED_EFFECTIVE"),
            "unknown product binary {variable}"
        );
        "reviewed-effective-oos"
    };
    let path = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(path.is_file(), "missing built producer {}", path.display());
    path.to_str().unwrap().to_string()
}
