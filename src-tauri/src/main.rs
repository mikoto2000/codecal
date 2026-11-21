#[cfg(feature = "desktop")]
fn main() { codecal_lib::run(); }
#[cfg(not(feature = "desktop"))]
fn main() {}
