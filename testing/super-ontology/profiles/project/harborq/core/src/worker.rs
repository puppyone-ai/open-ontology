use tokio::time::sleep;
use tokio::sync::mpsc;

use crate::queue::Queue;
use crate::retry::next_delay;

pub struct Worker {
    pub name: String,
}

impl Worker {
    pub async fn run(&self, queue: &Queue) {
        let _ = (queue, next_delay);
        sleep(std::time::Duration::from_millis(200)).await;
    }
}

pub fn spawn_all(count: usize) -> Vec<Worker> {
    let (_tx, _rx) = mpsc::channel::<u8>(1);
    (0..count).map(|i| Worker { name: format!("w{i}") }).collect()
}
