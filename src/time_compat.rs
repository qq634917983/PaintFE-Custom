// `std::time::Instant`/`SystemTime` call into the OS clock, which panics on
// wasm32-unknown-unknown ("time not implemented on this platform" — there's
// no OS clock in the browser sandbox). `web-time` is a drop-in replacement
// with the identical API, backed by `performance.now()`/`Date.now()` on web
// and re-exporting real `std::time` on every other target.
#[cfg(target_arch = "wasm32")]
pub use web_time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
