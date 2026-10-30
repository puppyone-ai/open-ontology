//! Run explicitly against the isolated fixture installed with tools/deploy.py.
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROJECT: &str = "0a117abb-1111-4111-8111-111111111111";

fn command() -> Command {
    let container = std::env::var("OO_JOBS_HEARTBEAT_CONTAINER")
        .expect("set OO_JOBS_HEARTBEAT_CONTAINER to an owned fixture");
    assert!(container.starts_with("oo-cb-K-heartbeat-"));
    let mut command = Command::new("docker");
    command.args([
        "exec",
        "-i",
        &container,
        "psql",
        "-X",
        "-qAt",
        "-v",
        "ON_ERROR_STOP=1",
        "-U",
        "oo_cb_k_admin",
        "-d",
        "oo_cb_k_heartbeat",
    ]);
    command
}

fn sql(text: &str) -> String {
    let mut child = command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("psql");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn task() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let kind = format!("heartbeat_{nonce}");
    sql(&format!(
        "SELECT task_id FROM jobs.enqueue('{PROJECT}', '{kind}', 1, '{{}}', \
         'cfg', repeat('a',64), 'heartbeat-{nonce}', clock_timestamp(), 3);"
    ));
    sql(&format!(
        "SELECT task_id FROM jobs.claim('{PROJECT}', ARRAY['{kind}'], 'worker');"
    ))
}

#[test]
#[ignore = "requires an isolated PostgreSQL fixture; see migrations/README.md"]
fn heartbeat_uses_the_clock_after_a_loader_row_lock() {
    let task = task();
    let application = format!("heartbeat_holder_{}", task.replace('-', ""));
    let mut holder = command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write!(
        holder.stdin.take().unwrap(),
        "SET application_name='{application}'; BEGIN; \
         SELECT id FROM jobs.tasks WHERE id='{task}' FOR UPDATE; \
         DO $loader$ BEGIN FOR i IN 1..32 LOOP \
           PERFORM pg_sleep(1); \
           PERFORM jobs.heartbeat('{PROJECT}','{task}','worker',1); \
         END LOOP; END $loader$; \
         SELECT status FROM jobs.heartbeat('{PROJECT}','{task}','worker',1); COMMIT;"
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while sql(&format!(
        "SELECT EXISTS(SELECT 1 FROM pg_stat_activity \
         WHERE application_name='{application}' AND wait_event='PgSleep');"
    )) != "t"
    {
        assert!(
            Instant::now() < deadline,
            "holder did not take the task lock"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        sql(&format!(
            "SELECT status FROM jobs.heartbeat('{PROJECT}','{task}','worker',1);"
        )),
        "extended",
    );
    let holder = holder.wait_with_output().unwrap();
    assert!(
        holder.status.success(),
        "{}",
        String::from_utf8_lossy(&holder.stderr)
    );
    let remaining: f64 = sql(&format!(
        "SELECT extract(epoch FROM lease_until-clock_timestamp()) \
         FROM jobs.tasks WHERE id='{task}';"
    ))
    .parse()
    .unwrap();
    assert!(
        remaining > 29.0,
        "blocked heartbeat shortened the renewed lease: {remaining}s"
    );
}

#[test]
#[ignore = "requires an isolated PostgreSQL fixture; see migrations/README.md"]
fn heartbeat_still_refuses_expired_and_mismatched_claims() {
    let task = task();
    for (worker, generation) in [("another-worker", 1), ("worker", 2)] {
        assert_eq!(
            sql(&format!(
                "SELECT status FROM jobs.heartbeat('{PROJECT}','{task}','{worker}',{generation});"
            )),
            "lost_lease"
        );
    }
    sql(&format!(
        "UPDATE jobs.tasks SET lease_until=clock_timestamp()-interval '1 second' WHERE id='{task}';"
    ));
    assert_eq!(
        sql(&format!(
            "SELECT status FROM jobs.heartbeat('{PROJECT}','{task}','worker',1);"
        )),
        "lost_lease"
    );
}

#[test]
#[ignore = "requires an isolated PostgreSQL fixture; see migrations/README.md"]
fn a_claim_that_expires_while_waiting_is_not_revived() {
    let task = task();
    sql(&format!(
        "UPDATE jobs.tasks SET lease_until=clock_timestamp()+interval '1 second' WHERE id='{task}';"
    ));
    let mut holder = command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    write!(
        holder.stdin.take().unwrap(),
        "BEGIN; SELECT id FROM jobs.tasks WHERE id='{task}' FOR UPDATE; SELECT pg_sleep(2); COMMIT;"
    )
    .unwrap();
    thread::sleep(Duration::from_millis(200));
    assert_eq!(
        sql(&format!(
            "SELECT status FROM jobs.heartbeat('{PROJECT}','{task}','worker',1);"
        )),
        "lost_lease"
    );
    assert!(holder.wait_with_output().unwrap().status.success());
}
