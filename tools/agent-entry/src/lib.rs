//! Localprocess agent entry. Management identity is not a data grant.
//! `parent_acceptance` stays false.

#![forbid(unsafe_code)]

mod config;
mod error;
mod forward;
mod gateway;
mod gold;
mod httputil;
mod identity;
mod mapping;
mod pack;
mod release;
mod server;
mod sessionapi;
mod sha;
mod sqlgate;
mod tlsutil;
mod turn;

pub use config::{load_config, startup_gate, Config, CONTRACT, DEPLOYMENT};
pub use error::Fault;
pub use gold::{
    account_party, account_rows, active_parties, amount_text, entry_rows, memo, party, party_rows, qty, PartyRow,
};
pub use httputil::{cookie_pair, header, status_line, ParsedHttp};
pub use mapping::{self_asserted, SELF_ASSERTED};
pub use pack::pack_digest;
pub use release::{fetch_active_pack, parse_active_pack, serve_private_release, ActivePack};
pub use server::serve;
pub use sessionapi::receipt_closed;
pub use sha::{hex_encode, sha256};
pub use gateway::{parse_query_pg_stdout, query_id_from_hash, QueryPgView};
pub use sqlgate::sql_allowed;
pub use tlsutil::https_exchange;

pub use turn::{
    citations, complete_decision, gold_for, prompt_for, CompleteInput, GOLD_ENTRY, QUESTION_CLARIFY, QUESTION_COUNT,
    QUESTION_PHONE, RELEASE_FIXTURE,
};

pub const PARENT_ACCEPTANCE: bool = false;
pub const PRODUCTION_SIGNED: bool = false;
