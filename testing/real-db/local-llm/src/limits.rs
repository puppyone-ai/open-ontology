use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_header_bytes: usize,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub max_response_chunks: usize,
    pub deadline: Duration,
    pub max_concurrency: usize,
}

impl Limits {
    pub const fn standard() -> Self {
        Self {
            max_header_bytes: 8192,
            max_request_bytes: 65536,
            max_response_bytes: 65536,
            max_response_chunks: 32,
            deadline: Duration::from_millis(30_000),
            max_concurrency: 2,
        }
    }
}
