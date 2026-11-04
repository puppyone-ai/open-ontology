use super::*;
use std::{
    os::unix::fs::DirBuilderExt,
    process::{Command, Stdio},
    time::Instant,
};

fn root(name: &str) -> std::path::PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../evidence")
        .join(format!("build-token-{name}-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    path
}

fn binding() -> Value {
    json!({"first_user_grant":{"project_id":"fixture","role":"reader"}})
}

fn response(request: &Value) -> Value {
    json!({"token":request["retry"]["token"],
        "token_sha256":request["retry"]["key"],"expires_at_ms":12345})
}

fn remote(root: &Path, request: &Value) -> Result<Value> {
    let committed = root.join("remote-request.json");
    if committed.exists() {
        assert!(review::read(&committed).ok().unwrap() == *request);
    } else {
        review::save_new(&committed, request).ok().unwrap();
        fs::write(root.join("issues"), b"1").unwrap();
    }
    Ok(response(request))
}

fn pause(root: &Path) {
    fs::write(root.join("ready"), b"1").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn child() {
    let Some(path) = std::env::var_os("OO_BUILD_TOKEN_CHILD") else {
        return;
    };
    let path = Path::new(&path);
    let boundary = std::env::var("OO_BUILD_TOKEN_BOUNDARY").unwrap();
    run_with(path, &binding(), |request| {
        if boundary == "request" {
            pause(path);
        }
        let response = remote(path, request)?;
        if boundary == "commit" {
            pause(path);
        }
        Ok(response)
    })
    .ok()
    .unwrap();
    pause(path);
}

#[test]
fn sigkill_replays_reserved_material_without_duplicate_issuance() {
    for boundary in ["request", "commit", "receipt"] {
        let path = root(boundary);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "args::admin::build_token::tests::child"])
            .env("OO_BUILD_TOKEN_CHILD", &path)
            .env("OO_BUILD_TOKEN_BOUNDARY", boundary)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.join("ready").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        child.kill().unwrap();
        let status = child.wait().unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9));
        assert!(path.join("ready").exists());
        run_with(&path, &binding(), |request| remote(&path, request))
            .ok()
            .unwrap();
        assert_eq!(fs::read(path.join("issues")).unwrap(), b"1");
        run_with(&path, &binding(), |_| {
            panic!("receipt must prevent dispatch")
        })
        .ok()
        .unwrap();
        assert!(build_tail::verify_token(&path).is_ok());
        assert!(run_with(&path, &json!({"changed":true}), |_| panic!()).is_err());
        fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn refuses_lost_legacy_intent_and_retries_a_lost_new_response() {
    let path = root("refusals");
    review::save_new(&path.join("first_token-intent.json"), &json!({}))
        .ok()
        .unwrap();
    assert!(run_with(&path, &binding(), |_| panic!("legacy uncertain")).is_err());
    fs::remove_file(path.join("first_token-intent.json")).unwrap();
    let result = run_with(&path, &binding(), |request| {
        remote(&path, request)?;
        Err(Failure::local("TEST_LOST_RESPONSE"))
    });
    assert!(result.is_err());
    let reserved = review::read(&path.join("first-token-request.json"))
        .ok()
        .unwrap();
    let token = reserved["request"]["retry"]["token"].as_str().unwrap();
    assert_eq!(token.len(), 48);
    // Canonical 32-byte base64url material has only sixteen possibilities in its last sextet.
    assert!(b"AEIMQUYcgkosw048".contains(token.as_bytes().last().unwrap()));
    assert!(run_with(&path, &json!({"changed":true}), |_| panic!()).is_err());
    assert!(run_with(&path, &binding(), |_| Ok(json!({"token":"wrong"}))).is_err());
    run_with(&path, &binding(), |request| remote(&path, request))
        .ok()
        .unwrap();
    fs::write(path.join("first-user.token"), b"changed").unwrap();
    assert!(run_with(&path, &binding(), |_| panic!()).is_err());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn rejects_plain_http_before_reading_tls_material() {
    let publication = json!({"gateway_url":"http://localhost",
        "first_user_grant":{"audience_url":"http://localhost"}});
    assert!(
        issue(
            &publication,
            &json!({"grant":publication["first_user_grant"]})
        )
        .is_err()
    );
}

#[test]
#[ignore = "owned mTLS Gateway fixture required"]
fn live_child() {
    let publication = review::read(Path::new(
        &std::env::var("OO_BUILD_TOKEN_PUBLICATION").unwrap(),
    ))
    .ok()
    .unwrap();
    let path = std::env::var("OO_BUILD_TOKEN_LIVE_ROOT").unwrap();
    let root = Path::new(&path);
    run_with(root, &publication, |request| {
        let _ = issue(&publication, request)?;
        // The real Gateway transaction committed; deliberately lose its response.
        pause(root);
        #[allow(unreachable_code)]
        Err(Failure::local("TEST_UNREACHABLE"))
    })
    .ok()
    .unwrap();
}

#[test]
#[ignore = "run token-store/test_issue_retry.py --cli"]
fn owned_gateway_first_token() {
    let file = std::env::var("OO_BUILD_TOKEN_PUBLICATION").unwrap();
    let publication = review::read(Path::new(&file)).ok().unwrap();
    let path = Path::new(&file).parent().unwrap().join("cli-state");
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "args::admin::build_token::tests::live_child",
        ])
        .env("OO_BUILD_TOKEN_LIVE_ROOT", &path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.join("ready").exists() && Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
    }
    let status = child.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        status.signal(),
        Some(9),
        "child did not reach the committed boundary"
    );
    assert!(path.join("ready").exists());
    let result = run(&path, &publication).ok().unwrap();
    let replay = run_with(&path, &publication, |_| {
        panic!("receipt must suppress dispatch")
    })
    .ok()
    .unwrap();
    assert_eq!(result, replay);
    assert!(build_tail::verify_token(&path).is_ok());
}
