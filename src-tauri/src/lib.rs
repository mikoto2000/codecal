pub mod activity;
pub mod claude;
#[cfg(feature = "desktop")]
mod desktop;
pub mod human_bands;
pub mod human_runtime;
pub mod human_summary;
mod migrations;
pub mod parser;
pub mod rei;
pub mod rei_human;
#[cfg(all(feature = "desktop", windows))]
mod startup_window;
pub mod store;
#[cfg(feature = "desktop")]
pub use desktop::run;
pub mod storage;
