//! Terminal UI for `oo init`: five screens (connect, tables, build, confirm,
//! deliver) in one visual language. Every screen is a pure render of its
//! state into a ratatui buffer, so all of them are snapshot-tested; the
//! interactive loop lives in `app` and is the only code that touches the TTY.
pub mod adjudication;
pub mod app;
pub mod backend;
pub mod fx;
pub mod graph;
pub mod scorer;
pub mod screens;
pub mod theme;
