pub mod activity;
pub mod claude;
pub mod human_runtime;
#[cfg(feature = "desktop")]
mod desktop;
mod migrations;
pub mod parser;
pub mod rei;
pub mod rei_human;
pub mod store;
#[cfg(feature = "desktop")]
pub use desktop::run;
pub mod storage;
