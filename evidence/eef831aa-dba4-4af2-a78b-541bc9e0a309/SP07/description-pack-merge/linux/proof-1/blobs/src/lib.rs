//! Approved description review to a resealed captured draft and role packs.
//!
//! `apply_description_pack` re-executes `apply_description_review`, then the
//! captured-draft, reviewed-effective, and role-pack compilers. It does not
//! open a database, call a model, grant, authenticate, or publish.

#![forbid(unsafe_code)]

mod apply;
mod canonical;
mod error;

pub use apply::apply_description_pack;
pub use error::{COMMAND_PROTOCOL, PACK_COMPILER, PACK_PROTOCOL, DescriptionPackError};
