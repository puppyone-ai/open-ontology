//! Independent review overlay for the local deployment lock and 8192-byte publish path.
//! This file is copied into an immutable archive of 3a251dea. It is not product source.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use object_store::ObjectStore;
use oo_control::{
    AdmissionContext, FileHost, FileHostError, GrantEvidence, HostLimits, ObjectManifest,
    PrincipalId, ProjectId, SessionId, UploadConfig, UploadSession, idempotency_key,
};
use sha2::{Digest, Sha256};

const EIGHT: &[u8] = b"12345678";
const ORIGINAL: &[u8] = b"original-bytes";
const REPLACEMENT: &[u8] = b"XXXXXXXXXXXXXX";
const MIB8: usize = 8 * 1024 * 1024;
const MIB32: usize = 32 * 1024 * 1024;
const RESIDENT_CAP: u64 = 2 * 1024 * 1024;

#[tokio::test]
async fn quota_ten_eight_byte_reservations() {
    if child_for("quota_ten_eight_byte_reservations") {
        quota_child().await;
        return;
    }

    let scratch = scratch_dir("quota");
    let root = scratch.join("store");
    fs::create_dir_all(&root).unwrap();
    let project = "projectq".to_owned();
    let principal = "userq".to_owned();
    let grant = "grant-one".to_owned();

    let distinct = run_begin_wave(BeginWave {
        root: root.clone(),
        project: project.clone(),
        principal: principal.clone(),
        grant: grant.clone(),
        same_display: false,
        label: "distinct",
        test_name: "quota_ten_eight_byte_reservations",
        participants: Vec::new(),
    })
    .await;
    let (ok, quota, other, sessions) = classify(&distinct);
    let reserved = reserved_bytes(&root, &project);
    let mut note = format!(
        "distinct_lines={}\ndistinct_ok={ok}\ndistinct_quota={quota}\ndistinct_other={other}\ndistinct_reserved={reserved}\ndistinct_session_ids={}\n",
        distinct.len(),
        sessions.len()
    );
    assert_eq!(other, 0, "{note}{distinct:?}");
    assert_eq!(ok, 1, "{note}{distinct:?}");
    assert_eq!(quota, distinct.len() - 1, "{note}{distinct:?}");
    assert_eq!(reserved, 8, "{note}");
    assert_eq!(quota_file_bodies(&root, &project), vec![b"8".to_vec()]);

    let winner = sessions[0].clone();
    let host = FileHost::open_local(&root, quota_limits()).unwrap();
    let ctx = admission(&project, &principal, &grant);
    host.abort(&ctx, &SessionId::parse(&winner).unwrap())
        .await
        .unwrap();
    let after_abort = reserved_bytes(&root, &project);
    note.push_str(&format!("after_distinct_abort={after_abort}\n"));
    assert_eq!(after_abort, 0, "{note}");

    let same = run_begin_wave(BeginWave {
        root: root.clone(),
        project: project.clone(),
        principal: principal.clone(),
        grant: grant.clone(),
        same_display: true,
        label: "same",
        test_name: "quota_ten_eight_byte_reservations",
        participants: Vec::new(),
    })
    .await;
    let (ok, quota, other, sessions) = classify(&same);
    let reserved = reserved_bytes(&root, &project);
    note.push_str(&format!(
        "same_ok={ok}\nsame_quota={quota}\nsame_other={other}\nsame_reserved={reserved}\nsame_unique_sessions={}\n",
        unique_count(&sessions)
    ));
    assert_eq!(other, 0, "{note}{same:?}");
    assert_eq!(quota, 0, "{note}{same:?}");
    assert_eq!(ok, same.len(), "{note}{same:?}");
    assert_eq!(unique_count(&sessions), 1, "{note}{same:?}");
    assert_eq!(reserved, 8, "{note}");
    assert_eq!(quota_file_bodies(&root, &project), vec![b"8".to_vec()]);

    let session_id = sessions[0].clone();
    let same_cfg = upload("same.bin", EIGHT, None);
    let key = idempotency_key(&ctx, &same_cfg).unwrap();
    let independent = idempotency_hex(
        &project,
        &principal,
        &hex_encode(&sha256(EIGHT)),
        EIGHT.len() as u64,
        "same.bin",
        "",
    );
    assert_eq!(key.as_str(), independent);
    let pointer = root.join(format!(
        "meta/idempotency/{project}/{principal}/{}",
        key.as_str()
    ));
    assert_eq!(fs::read(&pointer).unwrap(), session_id.as_bytes());

    let resumed = host
        .begin(&admission(&project, &principal, "grant-two"), &same_cfg)
        .await
        .unwrap();
    assert_eq!(resumed.session_id.as_str(), session_id);
    assert_eq!(reserved_bytes(&root, &project), 8);
    let stored = fs::read_to_string(root.join(format!("meta/session/{session_id}"))).unwrap();
    assert!(
        stored.contains("\"grant_evidence\":\"grant-one\""),
        "{stored}"
    );
    assert!(!stored.contains("grant-two"), "{stored}");
    note.push_str("grant_resume_same_session=1\ngrant_stored=grant-one\n");

    host.abort(&ctx, &SessionId::parse(&session_id).unwrap())
        .await
        .unwrap();
    assert_eq!(reserved_bytes(&root, &project), 0);
    let retried = host.begin(&ctx, &same_cfg).await.unwrap();
    assert_ne!(retried.session_id.as_str(), session_id);
    assert_eq!(reserved_bytes(&root, &project), 8);
    host.abort(&ctx, &retried.session_id).await.unwrap();
    let second = host.abort(&ctx, &retried.session_id).await.unwrap_err();
    assert_eq!(second, FileHostError::NotFound);
    assert_eq!(reserved_bytes(&root, &project), 0);
    note.push_str("abort_retry_reserved_cycle=8_then_0\nsecond_abort=NotFound\n");

    let conflict_root = scratch.join("conflict");
    fs::create_dir_all(&conflict_root).unwrap();
    let limits = HostLimits::try_new(64, 64, 8, 40).unwrap();
    let host = FileHost::open_local(&conflict_root, limits).unwrap();
    let ctx_a = admission("projectq", "usera", "grant-a");
    let ctx_b = admission("projectq", "userb", "grant-b");
    let session_a = host
        .begin(&ctx_a, &upload("a.bin", b"aaaaaaaa", None))
        .await
        .unwrap();
    host.put_part(&ctx_a, &session_a.session_id, 1, b"aaaaaaaa")
        .await
        .unwrap();
    let session_b = host
        .begin(&ctx_b, &upload("b.bin", b"bbbbbbbb", None))
        .await
        .unwrap();
    host.put_part(&ctx_b, &session_b.session_id, 1, b"bbbbbbbb")
        .await
        .unwrap();
    let notes = conflict_root.join(format!(
        "staging/{}/part/notes",
        session_a.session_id.as_str()
    ));
    fs::create_dir_all(notes.parent().unwrap()).unwrap();
    fs::write(&notes, b"keep-notes").unwrap();
    let conflict = host
        .put_part(&ctx_a, &session_a.session_id, 1, b"cccccccc")
        .await
        .unwrap_err();
    assert_eq!(conflict, FileHostError::Conflict);
    let part_a = conflict_root.join(format!(
        "staging/{}/part/00001",
        session_a.session_id.as_str()
    ));
    let part_b = conflict_root.join(format!(
        "staging/{}/part/00001",
        session_b.session_id.as_str()
    ));
    assert_eq!(fs::read(&part_a).unwrap(), b"aaaaaaaa");
    assert_eq!(fs::read(&part_b).unwrap(), b"bbbbbbbb");
    assert_eq!(fs::read(&notes).unwrap(), b"keep-notes");
    host.abort(&ctx_a, &session_a.session_id).await.unwrap();
    assert!(!part_a.exists());
    assert_eq!(fs::read(&part_b).unwrap(), b"bbbbbbbb");
    assert_eq!(fs::read(&notes).unwrap(), b"keep-notes");
    assert_eq!(reserved_bytes(&conflict_root, "projectq"), 8);
    host.abort(&ctx_b, &session_b.session_id).await.unwrap();
    assert_eq!(reserved_bytes(&conflict_root, "projectq"), 0);
    note.push_str("conflict_isolated=1\nabort_left_other_session=1\n");
    write_note("quota.txt", &note);
}

#[tokio::test]
async fn complete_part_writer_across_processes() {
    if child_for("complete_part_writer_across_processes") {
        match std::env::var("OO_A188_ROLE").unwrap().as_str() {
            "complete" => complete_child().await,
            "put" => put_child().await,
            other => panic!("role {other}"),
        }
        return;
    }
    assert_eq!(ORIGINAL.len(), REPLACEMENT.len());

    let scratch = scratch_dir("complete");
    let root = scratch.join("store");
    fs::create_dir_all(&root).unwrap();
    let ctx = admission("projectc", "userc", "grant-c");
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let cfg = upload("original.bin", ORIGINAL, Some("text/plain"));
    let session = host.begin(&ctx, &cfg).await.unwrap();
    host.put_part(&ctx, &session.session_id, 1, ORIGINAL)
        .await
        .unwrap();
    let expected_key = idempotency_hex(
        "projectc",
        "userc",
        &hex_encode(&sha256(ORIGINAL)),
        ORIGINAL.len() as u64,
        "original.bin",
        "text/plain",
    );
    assert_eq!(session.idempotency_key.as_str(), expected_key);

    let notes = root.join(format!(
        "staging/{}/part/notes",
        session.session_id.as_str()
    ));
    fs::create_dir_all(notes.parent().unwrap()).unwrap();
    fs::write(&notes, b"keep-notes").unwrap();
    let quota_notes = root.join("meta/quota/projectc/notes");
    fs::create_dir_all(quota_notes.parent().unwrap()).unwrap();
    fs::write(&quota_notes, b"keep-quota").unwrap();
    let outside = scratch.join("outside-the-store.bin");
    fs::write(&outside, b"keep-outside").unwrap();

    let barrier = scratch.join("barrier");
    fs::create_dir_all(&barrier).unwrap();
    let ids = ["c0", "c1", "writer"];
    let barrier_wait = barrier.clone();
    let releaser = tokio::task::spawn_blocking(move || {
        release_when_ready(&barrier_wait, &ids.map(str::to_owned));
    });
    let mut children = Vec::new();
    for id in ["c0", "c1"] {
        let result = barrier.join(format!("result-{id}"));
        children.push(spawn_role(
            "complete_part_writer_across_processes",
            "complete",
            &[
                ("OO_A188_ROOT", root.to_str().unwrap()),
                ("OO_A188_BARRIER", barrier.to_str().unwrap()),
                ("OO_A188_RESULT", result.to_str().unwrap()),
                ("OO_A188_ID", id),
                ("OO_A188_SESSION", session.session_id.as_str()),
            ],
        ));
    }
    let writer_result = barrier.join("result-writer");
    children.push(spawn_role(
        "complete_part_writer_across_processes",
        "put",
        &[
            ("OO_A188_ROOT", root.to_str().unwrap()),
            ("OO_A188_BARRIER", barrier.to_str().unwrap()),
            ("OO_A188_RESULT", writer_result.to_str().unwrap()),
            ("OO_A188_ID", "writer"),
            ("OO_A188_SESSION", session.session_id.as_str()),
        ],
    ));
    releaser.await.unwrap();
    for child in children {
        wait_child(child);
    }
    let c0 = fs::read_to_string(barrier.join("result-c0")).unwrap();
    let c1 = fs::read_to_string(barrier.join("result-c1")).unwrap();
    let writer = fs::read_to_string(&writer_result).unwrap();
    assert!(c0.starts_with("ok "), "{c0}");
    assert_eq!(c0, c1, "manifests diverged\n{c0}\n{c1}");
    assert!(
        writer == "err Conflict" || writer == "err Closed",
        "writer {writer}"
    );

    let object_path = root.join(session.object_key.as_str());
    let published = fs::read(&object_path).unwrap();
    assert_eq!(published, ORIGINAL);
    assert_eq!(sha256(&published), sha256(ORIGINAL));
    let part = root.join(format!(
        "staging/{}/part/00001",
        session.session_id.as_str()
    ));
    assert!(!part.exists());
    assert_eq!(fs::read(&notes).unwrap(), b"keep-notes");
    assert_eq!(fs::read(&quota_notes).unwrap(), b"keep-quota");
    assert_eq!(fs::read(&outside).unwrap(), b"keep-outside");

    let manifest_path = root.join(format!("meta/manifest/{}", session.session_id.as_str()));
    let manifest_bytes = fs::read(&manifest_path).unwrap();
    let again = host.complete(&ctx, &session.session_id).await.unwrap();
    assert_eq!(fs::read(&manifest_path).unwrap(), manifest_bytes);
    assert_eq!(again.contract, "oo.control.local-file-hosting/v1");
    assert_eq!(again.object_digest, sha256(ORIGINAL));
    assert_eq!(again.size, ORIGINAL.len() as u64);
    assert_eq!(again.part_count, 1);
    assert_eq!(hex_encode(&again.lineage_digest), lineage_hex(&again));
    assert_eq!(fs::read(&object_path).unwrap(), ORIGINAL);
    write_note(
        "complete.txt",
        &format!(
            "complete_lines_equal=1\nwriter={writer}\nobject_sha={}\nmanifest_bytes_unchanged=1\nlineage_independent=1\nforeign_unchanged=1\n",
            hex_encode(&sha256(ORIGINAL))
        ),
    );
}

#[tokio::test]
async fn lock_lifetime_cancel_timeout_exit() {
    if child_for("lock_lifetime_cancel_timeout_exit") {
        assert_eq!(std::env::var("OO_A188_ROLE").unwrap(), "hold");
        hold_lock();
        return;
    }

    let mut note = String::new();
    let scratch = scratch_dir("lock");
    let limits = HostLimits::try_new(64, 64, 4, 100).unwrap();

    let cancel_root = scratch.join("cancel");
    fs::create_dir_all(&cancel_root).unwrap();
    FileHost::open_local(&cancel_root, limits).unwrap();
    let held = scratch.join("cancel-held");
    let release = scratch.join("cancel-release");
    let mut holder = spawn_hold(&cancel_root, &held, &release);
    wait_file(&held);
    let root_task = cancel_root.clone();
    let handle = tokio::spawn(async move {
        let host = FileHost::open_local(&root_task, limits).unwrap();
        host.begin(
            &admission("projectl", "userl", "grant-l"),
            &upload("cancel.bin", EIGHT, None),
        )
        .await
    });
    tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(300)))
        .await
        .unwrap();
    assert!(
        !handle.is_finished(),
        "begin acquired the lock while the holder was alive"
    );
    let abort_at = Instant::now();
    handle.abort();
    let abort_elapsed = match handle.await {
        Err(joined) => {
            assert!(joined.is_cancelled());
            abort_at.elapsed()
        }
        Ok(result) => panic!("cancelled begin finished: {result:?}"),
    };
    assert!(abort_elapsed < Duration::from_secs(5), "{abort_elapsed:?}");
    fs::write(&release, b"release").unwrap();
    let holder_status = holder.wait().unwrap();
    assert!(holder_status.success(), "{holder_status:?}");
    let follow = Instant::now();
    let host = FileHost::open_local(&cancel_root, limits).unwrap();
    let begun = host
        .begin(
            &admission("projectl", "userl", "grant-l"),
            &upload("cancel.bin", EIGHT, None),
        )
        .await
        .unwrap();
    let follow_elapsed = follow.elapsed();
    assert!(
        follow_elapsed < Duration::from_secs(5),
        "{follow_elapsed:?}"
    );
    assert_eq!(reserved_bytes(&cancel_root, "projectl"), 8);
    host.abort(
        &admission("projectl", "userl", "grant-l"),
        &begun.session_id,
    )
    .await
    .unwrap();
    assert_eq!(reserved_bytes(&cancel_root, "projectl"), 0);
    note.push_str(&format!(
        "cancel_abort_ms={}\nfollow_up_begin_ms={}\ncancel_reserved_after_abort=0\n",
        abort_elapsed.as_millis(),
        follow_elapsed.as_millis()
    ));

    let timeout_root = scratch.join("timeout");
    fs::create_dir_all(&timeout_root).unwrap();
    FileHost::open_local(&timeout_root, limits).unwrap();
    let held = scratch.join("timeout-held");
    let release = scratch.join("timeout-release");
    let mut holder = spawn_hold(&timeout_root, &held, &release);
    wait_file(&held);
    let host = FileHost::open_local(&timeout_root, limits).unwrap();
    let started = Instant::now();
    let err = host
        .begin(
            &admission("projectl", "userl", "grant-l"),
            &upload("timeout.bin", EIGHT, None),
        )
        .await
        .unwrap_err();
    let elapsed = started.elapsed();
    assert_eq!(err, FileHostError::Unavailable);
    assert!(elapsed >= Duration::from_secs(29), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(45), "{elapsed:?}");
    assert!(
        files_under(&timeout_root).is_empty(),
        "{:?}",
        files_under(&timeout_root)
    );
    fs::write(&release, b"release").unwrap();
    assert!(holder.wait().unwrap().success());
    let host = FileHost::open_local(&timeout_root, limits).unwrap();
    let begun = host
        .begin(
            &admission("projectl", "userl", "grant-l"),
            &upload("timeout.bin", EIGHT, None),
        )
        .await
        .unwrap();
    assert_eq!(reserved_bytes(&timeout_root, "projectl"), 8);
    host.abort(
        &admission("projectl", "userl", "grant-l"),
        &begun.session_id,
    )
    .await
    .unwrap();
    note.push_str(&format!(
        "timeout_ms={}\ntimeout_error=Unavailable\ntimeout_store_files=0\n",
        elapsed.as_millis()
    ));

    let exit_root = scratch.join("exit");
    fs::create_dir_all(&exit_root).unwrap();
    FileHost::open_local(&exit_root, limits).unwrap();
    let held = scratch.join("exit-held");
    let release = scratch.join("exit-release");
    let mut holder = spawn_hold(&exit_root, &held, &release);
    wait_file(&held);
    let root_task = exit_root.clone();
    let handle = tokio::spawn(async move {
        let host = FileHost::open_local(&root_task, limits).unwrap();
        host.begin(
            &admission("projectl", "userl", "grant-l"),
            &upload("exit.bin", EIGHT, None),
        )
        .await
    });
    tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(200)))
        .await
        .unwrap();
    assert!(
        !handle.is_finished(),
        "begin finished before the holder was killed"
    );
    let killed = Instant::now();
    holder.kill().unwrap();
    let outcome = handle.await.unwrap();
    let kill_elapsed = killed.elapsed();
    assert!(
        kill_elapsed < Duration::from_secs(10),
        "{kill_elapsed:?} {outcome:?}"
    );
    let begun = outcome.expect("begin after holder exit");
    let status = holder.wait().unwrap();
    assert!(!status.success(), "killed holder still reported success");
    assert_eq!(reserved_bytes(&exit_root, "projectl"), 8);
    let host = FileHost::open_local(&exit_root, limits).unwrap();
    host.abort(
        &admission("projectl", "userl", "grant-l"),
        &begun.session_id,
    )
    .await
    .unwrap();
    assert_eq!(reserved_bytes(&exit_root, "projectl"), 0);
    let again = host
        .begin(
            &admission("projectl", "userl", "grant-l"),
            &upload("after-exit.bin", EIGHT, None),
        )
        .await
        .unwrap();
    assert_ne!(again.session_id.as_str(), begun.session_id.as_str());
    assert_eq!(reserved_bytes(&exit_root, "projectl"), 8);
    note.push_str(&format!(
        "kill_to_begin_ms={}\nprocess_exit_released_lock=1\nsecond_begin_after_exit=1\n",
        kill_elapsed.as_millis()
    ));
    write_note("lock.txt", &note);
}

#[tokio::test]
async fn trusted_directory_lock_scope() {
    if child_for("trusted_directory_lock_scope") {
        quota_child().await;
        return;
    }

    let scratch = scratch_dir("scope");
    let real = scratch.join("deploy-real");
    fs::create_dir_all(&real).unwrap();
    let alias = scratch.join("deploy-alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let via = real.join("..").join("deploy-real");
    FileHost::open_local(&real, quota_limits()).unwrap();
    let lock = expected_lock_path(&real);
    assert_eq!(expected_lock_path(&alias), lock);
    assert_eq!(expected_lock_path(&via), lock);
    assert!(!lock.starts_with(&real));
    assert!(lock.is_file());
    let decoy = real.join("deploy-real.oo-file-hosting.lock");
    fs::write(&decoy, b"decoy-not-the-lock").unwrap();

    let wave = run_begin_wave(BeginWave {
        root: real.clone(),
        project: "projectt".to_owned(),
        principal: "usert".to_owned(),
        grant: "grant-t".to_owned(),
        same_display: false,
        label: "canonical",
        test_name: "trusted_directory_lock_scope",
        participants: vec![
            Participant::Host {
                id: "real".to_owned(),
                root: real.clone(),
            },
            Participant::Host {
                id: "alias".to_owned(),
                root: alias.clone(),
            },
            Participant::Process {
                id: "via".to_owned(),
                root: via.clone(),
            },
            Participant::Process {
                id: "alias-proc".to_owned(),
                root: alias,
            },
        ],
    })
    .await;
    let (ok, quota, other, _) = classify(&wave);
    let reserved = reserved_bytes(&real, "projectt");
    assert_eq!(other, 0, "{wave:?}");
    assert_eq!(ok, 1, "{wave:?}");
    assert_eq!(quota, 3, "{wave:?}");
    assert_eq!(reserved, 8);
    assert_eq!(fs::read(&decoy).unwrap(), b"decoy-not-the-lock");
    assert!(!scratch.join("deploy-alias.oo-file-hosting.lock").exists());

    let elsewhere = scratch.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let other = elsewhere.join("deploy-real");
    let host = FileHost::open_local(&other, quota_limits()).unwrap();
    host.begin(
        &admission("projectt", "usert", "grant-t"),
        &upload("other.bin", EIGHT, None),
    )
    .await
    .unwrap();
    let other_lock = expected_lock_path(&other);
    assert_ne!(other_lock, lock);
    assert!(!other_lock.starts_with(&other));
    assert_eq!(reserved_bytes(&other, "projectt"), 8);
    assert_eq!(reserved_bytes(&real, "projectt"), 8);
    write_note(
        "scope.txt",
        &format!(
            "canonical_ok={ok}\ncanonical_quota={quota}\ncanonical_reserved={reserved}\ndecoy_unchanged=1\nsibling_basename_reserved=8\nlock_outside_store=1\n"
        ),
    );
}

#[tokio::test]
async fn generic_store_refuses_before_write() {
    let store: std::sync::Arc<dyn ObjectStore> =
        std::sync::Arc::new(object_store::memory::InMemory::new());
    let host = FileHost::new(std::sync::Arc::clone(&store), quota_limits());
    let ctx = admission("projectg", "userg", "grant-g");
    let cfg = upload("mem.bin", EIGHT, None);
    let session = SessionId::parse("0123456789abcdef0123456789abcdef").unwrap();
    let begin_err = host.begin(&ctx, &cfg).await.unwrap_err();
    let put_err = host.put_part(&ctx, &session, 1, EIGHT).await.unwrap_err();
    let complete_err = host.complete(&ctx, &session).await.unwrap_err();
    let abort_err = host.abort(&ctx, &session).await.unwrap_err();
    assert_eq!(begin_err, FileHostError::Unsupported);
    assert_eq!(put_err, FileHostError::Unsupported);
    assert_eq!(complete_err, FileHostError::Unsupported);
    assert_eq!(abort_err, FileHostError::Unsupported);
    let text = begin_err.to_string();
    assert_eq!(
        text,
        "object store cannot enforce a linearizable project quota"
    );
    assert!(!text.contains('/'));
    let debug = format!("{host:?}");
    assert!(!debug.contains("InMemory"));
    assert!(!debug.contains('/'));
    let listed = ObjectStore::list_with_delimiter(store.as_ref(), None)
        .await
        .unwrap();
    assert!(listed.objects.is_empty(), "generic store gained objects");
    assert!(
        listed.common_prefixes.is_empty(),
        "generic store gained prefixes"
    );
    write_note("generic.txt", "unsupported_before_write=1\nstore_keys=0\n");
}

#[tokio::test]
async fn bounded_publish_and_resident_complete() {
    if child_for("bounded_publish_and_resident_complete") {
        assert_eq!(std::env::var("OO_A188_ROLE").unwrap(), "memory");
        memory_child().await;
        return;
    }

    let scratch = scratch_dir("bounded");
    let root = scratch.join("store");
    fs::create_dir_all(&root).unwrap();
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("projectb", "userb", "grant-b");
    let empty_session = host
        .begin(&ctx, &upload("empty.bin", EIGHT, None))
        .await
        .unwrap();
    let empty = host
        .put_part(&ctx, &empty_session.session_id, 1, b"")
        .await
        .unwrap_err();
    assert_eq!(empty, FileHostError::Limit);
    let empty_part = root.join(format!(
        "staging/{}/part/00001",
        empty_session.session_id.as_str()
    ));
    assert!(!empty_part.exists());

    let mut note = String::from("empty_part=Limit\n");
    for (label, size) in [("exact8192", 8192usize), ("tail8193", 8193usize)] {
        let bytes = pattern(size);
        let cfg = upload(&format!("{label}.bin"), &bytes, None);
        let session = host.begin(&ctx, &cfg).await.unwrap();
        host.put_part(&ctx, &session.session_id, 1, &bytes)
            .await
            .unwrap();
        let part = root.join(format!(
            "staging/{}/part/00001",
            session.session_id.as_str()
        ));
        assert_eq!(fs::metadata(&part).unwrap().len(), size as u64);
        let manifest = host.complete(&ctx, &session.session_id).await.unwrap();
        let object = fs::read(root.join(session.object_key.as_str())).unwrap();
        assert_eq!(object.len(), size);
        assert_eq!(sha256(&object), sha256(&bytes));
        assert_eq!(manifest.object_digest, sha256(&bytes));
        assert_eq!(manifest.size, size as u64);
        assert!(!part.exists());
        note.push_str(&format!("{label}=ok size={size}\n"));
    }

    let mut deltas = Vec::new();
    for size in [MIB8, MIB32] {
        let metrics = scratch.join(format!("memory-{size}.txt"));
        let child = spawn_role(
            "bounded_publish_and_resident_complete",
            "memory",
            &[
                (
                    "OO_A188_ROOT",
                    scratch.join(format!("mem-{size}")).to_str().unwrap(),
                ),
                ("OO_A188_METRICS", metrics.to_str().unwrap()),
                ("OO_A188_SIZE", &size.to_string()),
            ],
        );
        wait_child(child);
        let text = fs::read_to_string(&metrics).unwrap();
        assert_memory(&text, size);
        deltas.push(metric_u64(&text, "memory_hwm_delta_bytes"));
        note.push_str(&text);
        if let Ok(dir) = std::env::var("OO_A188_NOTES") {
            let dir = PathBuf::from(dir);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(format!("memory-{size}.txt")), &text).unwrap();
        }
    }
    assert!(deltas[0] < RESIDENT_CAP, "8MB delta {}", deltas[0]);
    assert!(deltas[1] < RESIDENT_CAP, "32MB delta {}", deltas[1]);
    note.push_str(&format!(
        "resident_cap={RESIDENT_CAP}\ndelta_8={}\ndelta_32={}\nscale_with_part=0\n",
        deltas[0], deltas[1]
    ));
    write_note("bounded.txt", &note);
}

struct BeginWave {
    root: PathBuf,
    project: String,
    principal: String,
    grant: String,
    same_display: bool,
    label: &'static str,
    test_name: &'static str,
    participants: Vec<Participant>,
}

enum Participant {
    Host { id: String, root: PathBuf },
    Process { id: String, root: PathBuf },
}

async fn run_begin_wave(mut wave: BeginWave) -> Vec<String> {
    if wave.participants.is_empty() {
        for index in 0..4 {
            wave.participants.push(Participant::Host {
                id: format!("h{index}"),
                root: wave.root.clone(),
            });
            wave.participants.push(Participant::Process {
                id: format!("p{index}"),
                root: wave.root.clone(),
            });
        }
    }
    let barrier = wave.root.join(format!("barrier-{}", wave.label));
    fs::create_dir_all(&barrier).unwrap();
    let ids: Vec<String> = wave
        .participants
        .iter()
        .map(|participant| match participant {
            Participant::Host { id, .. } | Participant::Process { id, .. } => id.clone(),
        })
        .collect();
    let barrier_wait = barrier.clone();
    let ids_wait = ids.clone();
    let releaser =
        tokio::task::spawn_blocking(move || release_when_ready(&barrier_wait, &ids_wait));
    let mut children = Vec::new();
    let mut hosts = Vec::new();
    for participant in &wave.participants {
        match participant {
            Participant::Host { id, root } => {
                let display = display_for(wave.same_display, id);
                hosts.push(tokio::spawn(host_begin(HostBegin {
                    root: root.clone(),
                    barrier: barrier.clone(),
                    id: id.clone(),
                    project: wave.project.clone(),
                    principal: wave.principal.clone(),
                    grant: wave.grant.clone(),
                    display,
                })));
            }
            Participant::Process { id, root } => {
                let result = barrier.join(format!("result-{id}"));
                let display = display_for(wave.same_display, id);
                children.push((
                    result.clone(),
                    spawn_role(
                        wave.test_name,
                        "begin",
                        &[
                            ("OO_A188_ROOT", root.to_str().unwrap()),
                            ("OO_A188_BARRIER", barrier.to_str().unwrap()),
                            ("OO_A188_RESULT", result.to_str().unwrap()),
                            ("OO_A188_ID", id),
                            ("OO_A188_PROJECT", &wave.project),
                            ("OO_A188_PRINCIPAL", &wave.principal),
                            ("OO_A188_GRANT", &wave.grant),
                            ("OO_A188_DISPLAY", &display),
                        ],
                    ),
                ));
            }
        }
    }
    releaser.await.unwrap();
    let mut lines = Vec::new();
    for handle in hosts {
        lines.push(handle.await.unwrap());
    }
    for (result, child) in children {
        wait_child(child);
        lines.push(fs::read_to_string(result).unwrap());
    }
    lines.sort();
    lines
}

struct HostBegin {
    root: PathBuf,
    barrier: PathBuf,
    id: String,
    project: String,
    principal: String,
    grant: String,
    display: String,
}

async fn host_begin(spec: HostBegin) -> String {
    barrier_arrive(&spec.barrier, &spec.id).await;
    let host = FileHost::open_local(&spec.root, quota_limits()).unwrap();
    let ctx = admission(&spec.project, &spec.principal, &spec.grant);
    let result = host.begin(&ctx, &upload(&spec.display, EIGHT, None)).await;
    begin_line(&result)
}

async fn quota_child() {
    let id = std::env::var("OO_A188_ID").unwrap();
    let root = PathBuf::from(std::env::var("OO_A188_ROOT").unwrap());
    let barrier = PathBuf::from(std::env::var("OO_A188_BARRIER").unwrap());
    let result = PathBuf::from(std::env::var("OO_A188_RESULT").unwrap());
    barrier_arrive(&barrier, &id).await;
    let host = FileHost::open_local(&root, quota_limits()).unwrap();
    let ctx = admission(
        &std::env::var("OO_A188_PROJECT").unwrap(),
        &std::env::var("OO_A188_PRINCIPAL").unwrap(),
        &std::env::var("OO_A188_GRANT").unwrap(),
    );
    let display = std::env::var("OO_A188_DISPLAY").unwrap();
    let begun = host.begin(&ctx, &upload(&display, EIGHT, None)).await;
    fs::write(result, begin_line(&begun)).unwrap();
}

async fn complete_child() {
    let root = PathBuf::from(std::env::var("OO_A188_ROOT").unwrap());
    let barrier = PathBuf::from(std::env::var("OO_A188_BARRIER").unwrap());
    let result = PathBuf::from(std::env::var("OO_A188_RESULT").unwrap());
    let id = std::env::var("OO_A188_ID").unwrap();
    let session = SessionId::parse(std::env::var("OO_A188_SESSION").unwrap()).unwrap();
    barrier_arrive(&barrier, &id).await;
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("projectc", "userc", "grant-c");
    let line = match host.complete(&ctx, &session).await {
        Ok(manifest) => format!(
            "ok {} {} {} {}",
            manifest.session_id.as_str(),
            hex_encode(&manifest.object_digest),
            hex_encode(&manifest.lineage_digest),
            manifest.size
        ),
        Err(error) => format!("err {error:?}"),
    };
    fs::write(result, line).unwrap();
}

async fn put_child() {
    let root = PathBuf::from(std::env::var("OO_A188_ROOT").unwrap());
    let barrier = PathBuf::from(std::env::var("OO_A188_BARRIER").unwrap());
    let result = PathBuf::from(std::env::var("OO_A188_RESULT").unwrap());
    let session = SessionId::parse(std::env::var("OO_A188_SESSION").unwrap()).unwrap();
    barrier_arrive(&barrier, "writer").await;
    let host = FileHost::open_local(&root, HostLimits::standard()).unwrap();
    let ctx = admission("projectc", "userc", "grant-c");
    let line = match host.put_part(&ctx, &session, 1, REPLACEMENT).await {
        Ok(_) => "ok".to_owned(),
        Err(error) => format!("err {error:?}"),
    };
    fs::write(result, line).unwrap();
}

fn hold_lock() {
    let root = PathBuf::from(std::env::var("OO_A188_ROOT").unwrap());
    let held = PathBuf::from(std::env::var("OO_A188_HELD").unwrap());
    let release = PathBuf::from(std::env::var("OO_A188_RELEASE").unwrap());
    let path = expected_lock_path(&root);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.lock().unwrap();
    fs::write(&held, b"held").unwrap();
    let started = Instant::now();
    while !release.exists() {
        if started.elapsed() > Duration::from_secs(120) {
            panic!("holder release timeout");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(file);
}

async fn memory_child() {
    let root = PathBuf::from(std::env::var("OO_A188_ROOT").unwrap());
    let metrics_path = PathBuf::from(std::env::var("OO_A188_METRICS").unwrap());
    let size: usize = std::env::var("OO_A188_SIZE").unwrap().parse().unwrap();
    assert!(size == MIB8 || size == MIB32, "{size}");
    fs::create_dir_all(&root).unwrap();
    let digest = digest_pattern(size);
    let limits =
        HostLimits::try_new(64 * 1024 * 1024, 64 * 1024 * 1024, 4, 128 * 1024 * 1024).unwrap();
    let host = FileHost::open_local(&root, limits).unwrap();
    let ctx = admission("projectm", "userm", "grant-m");
    let session = host
        .begin(
            &ctx,
            &UploadConfig {
                display_name: "stream.bin".to_owned(),
                expected_sha256: digest,
                expected_size: size as u64,
                content_type: None,
            },
        )
        .await
        .unwrap();
    let part_path = root.join(format!(
        "staging/{}/part/00001",
        session.session_id.as_str()
    ));
    write_pattern(&part_path, size);
    let hwm_before = proc_field("VmHWM:");
    let rss_before = proc_field("VmRSS:");
    let peak_before = proc_field("VmPeak:");
    let threads_before = proc_field("Threads:");
    let manifest = host.complete(&ctx, &session.session_id).await.unwrap();
    let hwm_after = proc_field("VmHWM:");
    let rss_after = proc_field("VmRSS:");
    let peak_after = proc_field("VmPeak:");
    let threads_after = proc_field("Threads:");
    let hwm_delta = hwm_after.saturating_sub(hwm_before) * 1024;
    let object_path = root.join(manifest.object_key.as_str());
    let streamed = hash_file(&object_path);
    let part_gone = !part_path.exists();
    let sha_match = streamed == digest && manifest.object_digest == digest;
    let text = format!(
        "memory_bytes={size}\nmemory_hwm_before_kib={hwm_before}\nmemory_hwm_after_kib={hwm_after}\nmemory_hwm_delta_bytes={hwm_delta}\nmemory_rss_before_kib={rss_before}\nmemory_rss_after_kib={rss_after}\nmemory_vmpeak_before_kib={peak_before}\nmemory_vmpeak_after_kib={peak_after}\nmemory_vmpeak_delta_bytes={}\nmemory_threads_before={threads_before}\nmemory_threads_after={threads_after}\npart_gone={}\nobject_sha_match={}\nexpected_sha={}\nobject_path={}\nobject_size={}\n",
        peak_after.saturating_sub(peak_before) * 1024,
        u8::from(part_gone),
        u8::from(sha_match),
        hex_encode(&digest),
        object_path.display(),
        manifest.size
    );
    fs::write(&metrics_path, &text).unwrap();
    assert!(hwm_delta < RESIDENT_CAP, "{text}");
    assert!(sha_match, "{text}");
    assert!(part_gone, "{text}");
    assert_eq!(manifest.size, size as u64, "{text}");
    assert!(threads_after < 128, "{text}");
}

fn assert_memory(text: &str, size: usize) {
    let delta = metric_u64(text, "memory_hwm_delta_bytes");
    assert!(
        delta < RESIDENT_CAP,
        "resident delta {delta} for {size}\n{text}"
    );
    assert_eq!(metric_u64(text, "object_sha_match"), 1, "{text}");
    assert_eq!(metric_u64(text, "part_gone"), 1, "{text}");
    assert_eq!(metric_u64(text, "object_size"), size as u64, "{text}");
    let object = PathBuf::from(metric_line(text, "object_path"));
    let expected = metric_line(text, "expected_sha");
    assert_eq!(hex_encode(&hash_file(&object)), expected, "{text}");
}

fn child_for(test_name: &str) -> bool {
    std::env::var_os("OO_A188_CHILD").is_some()
        && std::env::var("OO_A188_TEST").as_deref() == Ok(test_name)
}

fn spawn_role(test_name: &str, role: &str, vars: &[(&str, &str)]) -> Child {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.arg(test_name)
        .arg("--exact")
        .arg("--test-threads=1")
        .env("OO_A188_CHILD", "1")
        .env("OO_A188_TEST", test_name)
        .env("OO_A188_ROLE", role)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in vars {
        cmd.env(key, value);
    }
    cmd.spawn().unwrap()
}

fn spawn_hold(root: &Path, held: &Path, release: &Path) -> Child {
    spawn_role(
        "lock_lifetime_cancel_timeout_exit",
        "hold",
        &[
            ("OO_A188_ROOT", root.to_str().unwrap()),
            ("OO_A188_HELD", held.to_str().unwrap()),
            ("OO_A188_RELEASE", release.to_str().unwrap()),
        ],
    )
}

fn wait_child(child: Child) {
    let output = child.wait_with_output().unwrap();
    if !output.status.success() {
        panic!(
            "child exit {:?}\nstdout {}\nstderr {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn wait_file(path: &Path) {
    let started = Instant::now();
    while !path.is_file() {
        if started.elapsed() > Duration::from_secs(20) {
            panic!("timeout waiting for {}", path.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

async fn barrier_arrive(dir: &Path, id: &str) {
    let dir = dir.to_path_buf();
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        fs::write(dir.join(format!("ready-{id}")), b"ready").unwrap();
        let go = dir.join("go");
        let started = Instant::now();
        while !go.is_file() {
            if started.elapsed() > Duration::from_secs(60) {
                panic!("barrier timeout for {id}");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })
    .await
    .unwrap();
}

fn release_when_ready(dir: &Path, ids: &[String]) {
    let started = Instant::now();
    loop {
        if ids
            .iter()
            .all(|id| dir.join(format!("ready-{id}")).is_file())
        {
            fs::write(dir.join("go"), b"go").unwrap();
            return;
        }
        if started.elapsed() > Duration::from_secs(60) {
            panic!("release timeout in {}", dir.display());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn display_for(same: bool, id: &str) -> String {
    if same {
        "same.bin".to_owned()
    } else {
        format!("{id}.bin")
    }
}

fn classify(lines: &[String]) -> (usize, usize, usize, Vec<String>) {
    let mut ok = 0;
    let mut quota = 0;
    let mut other = 0;
    let mut sessions = Vec::new();
    for line in lines {
        if let Some(id) = line.strip_prefix("ok ") {
            ok += 1;
            sessions.push(id.to_owned());
        } else if line == "err Quota" {
            quota += 1;
        } else {
            other += 1;
        }
    }
    (ok, quota, other, sessions)
}

fn unique_count(values: &[String]) -> usize {
    let mut copy = values.to_vec();
    copy.sort();
    copy.dedup();
    copy.len()
}

fn begin_line(result: &Result<UploadSession, FileHostError>) -> String {
    match result {
        Ok(session) => format!("ok {}", session.session_id.as_str()),
        Err(error) => format!("err {error:?}"),
    }
}

fn quota_limits() -> HostLimits {
    HostLimits::try_new(64, 64, 8, 10).unwrap()
}

fn admission(project: &str, principal: &str, grant: &str) -> AdmissionContext {
    AdmissionContext {
        project_id: ProjectId::new(project).unwrap(),
        principal_id: PrincipalId::new(principal).unwrap(),
        grant_evidence: GrantEvidence::new(grant).unwrap(),
    }
}

fn upload(name: &str, bytes: &[u8], content_type: Option<&str>) -> UploadConfig {
    UploadConfig {
        display_name: name.to_owned(),
        expected_sha256: sha256(bytes),
        expected_size: bytes.len() as u64,
        content_type: content_type.map(str::to_owned),
    }
}

fn reserved_bytes(root: &Path, project: &str) -> u64 {
    quota_file_bodies(root, project)
        .iter()
        .map(|body| std::str::from_utf8(body).unwrap().parse::<u64>().unwrap())
        .sum()
}

fn quota_file_bodies(root: &Path, project: &str) -> Vec<Vec<u8>> {
    let dir = root.join("meta/quota").join(project);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut bodies = Vec::new();
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_str().unwrap();
        if name.len() == 32
            && name
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            bodies.push(fs::read(entry.path()).unwrap());
        }
    }
    bodies.sort();
    bodies
}

fn files_under(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk_files(root, &mut out);
    out.sort();
    out
}

fn walk_files(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk_files(&path, out);
        } else {
            out.push(path.display().to_string());
        }
    }
}

fn expected_lock_path(root: &Path) -> PathBuf {
    let canonical = fs::canonicalize(root).unwrap();
    let mut name = canonical.file_name().unwrap().to_os_string();
    name.push(".oo-file-hosting.lock");
    canonical.parent().unwrap().join(name)
}

fn pattern(size: usize) -> Vec<u8> {
    let mut out = vec![0u8; size];
    fill_chunk(0, &mut out);
    out
}

fn fill_chunk(offset: usize, buf: &mut [u8]) {
    for (index, byte) in buf.iter_mut().enumerate() {
        let n = offset + index;
        *byte = n.wrapping_mul(167).wrapping_add(13) as u8;
    }
}

fn digest_pattern(size: usize) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut offset = 0;
    while offset < size {
        let n = (size - offset).min(8192);
        fill_chunk(offset, &mut buf[..n]);
        hasher.update(&buf[..n]);
        offset += n;
    }
    finalize(hasher)
}

fn write_pattern(path: &Path, size: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::File::create(path).unwrap();
    let mut buf = [0u8; 8192];
    let mut offset = 0;
    while offset < size {
        let n = (size - offset).min(8192);
        fill_chunk(offset, &mut buf[..n]);
        file.write_all(&buf[..n]).unwrap();
        offset += n;
    }
    file.sync_all().unwrap();
}

fn hash_file(path: &Path) -> [u8; 32] {
    let mut file = fs::File::open(path).unwrap();
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    finalize(hasher)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    finalize(hasher)
}

fn finalize(hasher: Sha256) -> [u8; 32] {
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn feed(hasher: &mut Sha256, value: &str) {
    hasher.update(value.as_bytes());
    hasher.update([0]);
}

fn idempotency_hex(
    project: &str,
    principal: &str,
    sha_hex: &str,
    size: u64,
    display: &str,
    content_type: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"oo.file-hosting/v1 idempotency\0");
    feed(&mut hasher, project);
    feed(&mut hasher, principal);
    feed(&mut hasher, sha_hex);
    feed(&mut hasher, &size.to_string());
    feed(&mut hasher, display);
    feed(&mut hasher, content_type);
    hex_encode(&finalize(hasher))
}

fn lineage_hex(manifest: &ObjectManifest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"oo.file-hosting/v1 lineage\0");
    feed(&mut hasher, manifest.session_id.as_str());
    feed(&mut hasher, manifest.project_id.as_str());
    feed(&mut hasher, manifest.principal_id.as_str());
    feed(&mut hasher, manifest.grant_evidence.as_str());
    feed(&mut hasher, manifest.object_key.as_str());
    feed(&mut hasher, &hex_encode(&manifest.object_digest));
    feed(&mut hasher, &manifest.size.to_string());
    feed(&mut hasher, &manifest.part_count.to_string());
    feed(&mut hasher, manifest.idempotency_key.as_str());
    feed(&mut hasher, &manifest.display_name);
    feed(&mut hasher, manifest.content_type.as_deref().unwrap_or(""));
    hex_encode(&finalize(hasher))
}

fn proc_field(field: &str) -> u64 {
    let text = fs::read_to_string("/proc/self/status").unwrap();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            return rest.split_whitespace().next().unwrap().parse().unwrap();
        }
    }
    panic!("missing {field}");
}

fn metric_u64(text: &str, key: &str) -> u64 {
    metric_line(text, key).parse().unwrap()
}

fn metric_line(text: &str, key: &str) -> String {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            return rest.to_owned();
        }
    }
    panic!("missing {key} in {text}");
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-a188-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_note(name: &str, text: &str) {
    println!("{text}");
    if let Ok(dir) = std::env::var("OO_A188_NOTES") {
        let dir = PathBuf::from(dir);
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(dir.join(name), text);
    }
}
