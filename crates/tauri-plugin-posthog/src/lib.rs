//! `tauri-plugin-posthog`: sends anonymous analytics events to `PostHog`
//! from a queue on disk.

pub mod event;
pub mod identity;
pub mod queue;
pub mod send;
