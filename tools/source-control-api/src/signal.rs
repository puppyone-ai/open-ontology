//! Signal a process group this service spawned, including direct children
//! that called `setpgid` and left the parent's group.

use std::process::Command;

pub fn signal_tree(pid: i32) {
    if pid <= 1 {
        return;
    }
    let children = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap_or_default();
    for child in children.split_whitespace() {
        if let Ok(child_pid) = child.parse::<i32>() {
            signal_tree(child_pid);
        }
    }
    let _ = Command::new("/bin/kill").arg("-TERM").arg(format!("-{pid}")).status();
}
