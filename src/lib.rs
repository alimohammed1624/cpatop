//! cpatop — a btop-style terminal dashboard for CPA Usage Keeper.
//!
//! The binary lives in `main.rs`; everything else is a library so the UI can
//! be snapshot-tested against recorded API fixtures.

pub mod api;
pub mod app;
pub mod auth;
pub mod cli;
pub mod config;
pub mod fmt;
pub mod layout;
pub mod poller;
pub mod range;
pub mod secrets;
pub mod series;
pub mod server;
pub mod session_store;
pub mod theme;
pub mod tui;
pub mod ui;
pub mod update;
