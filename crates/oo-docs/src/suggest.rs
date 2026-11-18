//! L5 Suggest (10 §11.1, §16; board B-S7, B-S8): an Agent that learned something writes a suggestion
//! (`.context/suggest/` or the MCP `suggest` tool); the impact rules decide what is automatic and what a
//! person must see; accepting one changes the ontology and tells every other running Share that used the
//! fact. OO never writes a Drive file: `diff` is a draft for Desktop's revision mode.

pub mod diff;
pub mod digest;
pub mod impact;
pub mod rules;
pub mod schema;
pub mod store;
