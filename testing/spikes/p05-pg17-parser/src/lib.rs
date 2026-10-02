//! PG17 parser admission prototype.
//!
//! `syntax_allow` means the static select gates in this spike passed.
//! It is not permission to execute. OID, ACL, and RLS are not proven here.

mod admit;
mod classify;
mod decision;
mod identity;
mod limits;
mod node_names;
mod parent;
mod policy;
mod procinfo;
mod protocol;
mod sha256;
mod star;
mod supervise;
mod worker;

#[cfg(feature = "test-hooks")]
mod fault;

pub use admit::admit_native;
pub use decision::{accept_worker_decision, Decision};
pub use limits::{PARSE_DEADLINE, PARSER_VERSION_NUM};
pub use parent::admit_in_worker;
pub use parent::exercise_worker;
#[cfg(feature = "test-hooks")]
pub use parent::exercise_untrusted;
pub use policy::Policy;
pub use protocol::{KIND_ADMIT, KIND_CRASH, KIND_FAKE_ALLOW, KIND_GARBAGE, KIND_SPIN};
pub use sha256::sha256_file;
#[cfg(feature = "test-hooks")]
pub use fault::run_fault_worker;
pub use worker::run_worker;
