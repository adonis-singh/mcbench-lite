//! The core's methods for the routes of `server/index.js` that have no
//! `mcbench_ui::Command` yet, one file per area (`native/crates/app/ROUTES.md`
//! names each route's method). Each is an `async fn` (or a plain one when
//! nothing waits) on [`crate::core::Core`] returning its typed result or
//! today's refusal with its status.

pub mod archive;
pub mod benches;
pub mod chats;
pub mod files;
pub mod library;
pub mod logins;
pub mod system;
