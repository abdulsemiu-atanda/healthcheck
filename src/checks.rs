//! Built-in dependency checks. Each one reports under the key the status page
//! design names for it; a service picks the ones that apply to it.

#[cfg(feature = "database")]
mod database;
mod faktory;
#[cfg(feature = "keystore")]
mod redis;
#[cfg(feature = "keystore")]
mod worker;

#[cfg(feature = "database")]
pub use database::Database;
pub use faktory::Faktory;
#[cfg(feature = "keystore")]
pub use redis::Redis;
#[cfg(feature = "keystore")]
pub use worker::Worker;
