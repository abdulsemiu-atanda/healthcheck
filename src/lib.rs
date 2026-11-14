//! Health reporting shared by every service the status page watches.
//!
//! Each service keeps its own bare `GET /healthcheck` (a 200 with no body and no
//! I/O, safe for any caller at any rate). This crate supplies the second route,
//! `GET /healthcheck/details`, which is the only place a service touches its
//! dependencies on behalf of a monitor. The contract it implements comes from the
//! status page design:
//!
//! - The route answers only to `Authorization: Bearer <PROBE_TOKEN>`. Any other
//!   request gets a 404, indistinguishable from a route that does not exist, and
//!   an unset token disables the route entirely.
//! - Each token holder is limited to 10 requests a minute per client IP.
//! - Checks run concurrently and the whole round is cut off after 2 s. The result
//!   is cached for 5 s, so the route costs one round of checks per 5 s however
//!   often it is called.
//! - The body is a [`Report`]: the worst check decides the overall status, and a
//!   `failed` overall status answers 503, anything else 200.
//!
//! How the pieces fit:
//!
//! - [`Check`] is one dependency probe. The built-in checks live in [`checks`]:
//!   Faktory always, Postgres behind the `database` feature, Redis and worker
//!   heartbeats behind `keystore`, and a plain HTTP probe behind `http`, with
//!   which a public service reports on private ones it can reach.
//! - [`Health`] holds a service's checks, cache and rate limiter. A service builds
//!   one at startup, registers it as `web::Data<Health>`, and mounts [`details`]
//!   at `healthcheck/details` in its own routes.
//! - [`heartbeat::beat`] (feature `keystore`) is the writer side of the worker
//!   check: a worker process runs it alongside its job loop so the API can tell
//!   whether the worker is alive.

mod check;
pub mod checks;
#[cfg(feature = "keystore")]
pub mod heartbeat;
mod limiter;
mod report;
mod route;

pub use check::{Check, Failure, Reading};
pub use report::{CheckResult, Report, Status};
pub use route::{Health, details};
