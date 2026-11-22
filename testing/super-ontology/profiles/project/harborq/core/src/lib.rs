//! Harbor Queue core.
use std::collections::HashMap;

pub mod queue;
pub mod retry;
pub mod worker;

/// Settings of a queue.
pub struct Config {
    pub path: String,
    pub max_attempts: u32,
}

pub enum Status {
    Pending,
    Running,
    Dead,
}

pub trait Store {
    fn load(&self, id: u64) -> Option<String>;
}

pub fn default_config() -> Config {
    // fn commented_out() {}
    let note = "fn not_a_function() {}";
    let _ = (note, HashMap::<u32, u32>::new());
    Config { path: "jobs.db".into(), max_attempts: 5 }
}

#[cfg(test)]
mod tests {
    fn hidden_helper() {}
}
