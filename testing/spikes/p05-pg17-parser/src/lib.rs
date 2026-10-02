//! PG17 parser admission prototype.
//!
//! `syntax_allow` means the static select gates in this spike passed.
//! It is not permission to execute. OID, ACL, and RLS are not proven here.

mod admit;
mod classify;
mod decision;
mod limits;
mod node_names;
mod parent;
mod policy;
mod protocol;
mod star;
mod worker;

pub use admit::admit_native;
pub use decision::Decision;
pub use limits::{PARSE_DEADLINE, PARSER_VERSION_NUM};
pub use parent::{admit_in_worker, exercise_worker};
pub use policy::Policy;
pub use protocol::{KIND_CRASH, KIND_FAKE_ALLOW, KIND_GARBAGE, KIND_SPIN};
pub use worker::run_worker;
