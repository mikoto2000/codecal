pub mod activity;
pub mod claude;
#[cfg(feature = "desktop")]
mod desktop;
pub mod parser;
pub mod rei;
pub mod store;
#[cfg(feature = "desktop")]
pub use desktop::run;
pub mod storage;
