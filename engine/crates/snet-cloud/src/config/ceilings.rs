//! The largest value each configured number may take. Every budget
//! and cost computation runs in checked `u128`, where a product of any two
//! ceilings still fits, and a running total that crosses its ceiling is an
//! error.

pub const MAX_OUTPUT_TOKENS_PER_REQUEST: u64 = 128_000;
pub const MAX_INPUT_TOKEN_ALLOWANCE: u64 = 1_000_000;
pub const MAX_BODY_BYTES_CAP: u64 = 16 * 1024 * 1024;
pub const MAX_CONNECT_TIMEOUT_SECONDS: u64 = 60;
pub const MAX_TOTAL_TIMEOUT_SECONDS: u64 = 1_800;
pub const MAX_PRICE_MICRO_PER_MILLION_TOKENS: u64 = 1_000_000_000_000;
pub const MAX_TASK_REQUESTS: u64 = 10_000;
pub const MAX_TASK_BYTES_SENT: u64 = 1024 * 1024 * 1024;
pub const MAX_TASK_TOKENS: u64 = 1_000_000_000;
pub const MAX_TASK_COST_MICRO: u64 = 1_000_000_000_000_000;
