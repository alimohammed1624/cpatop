//! Keeper API: client, models and the live event stream.

pub mod client;
pub mod models;
pub mod sse;

pub use client::{ApiError, Client, LoginMode};

/// Session role reported by `/auth/session`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Admin,
    KeyViewer,
}

impl Role {
    pub fn from_session(role: Option<&str>) -> Option<Role> {
        match role {
            Some("admin") => Some(Role::Admin),
            Some("api_key_viewer") => Some(Role::KeyViewer),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::KeyViewer => "api-key",
        }
    }
}
