# healthcheck

The `GET /healthcheck/details` route every service exposes to the status page, and the dependency checks behind it.

## Why this crate exists

The status page probes public URLs only. Private services, workers and infrastructure are observed through the details
route of the public API in the same project. Keeping the route, its access rules and the checks in one crate means every
service answers the same way and the status page can parse all of them with one evaluator.

## The contract

- `Authorization: Bearer <PROBE_TOKEN>` is required. Anything else gets a 404, and an empty token disables the route.
- Token holders are limited to 10 requests per minute per client IP (429 past that).
- Checks run concurrently, are cut off after 2 s in total, and the result is cached for 5 s.
- 200 when the worst check is `ok` or `degraded`, 503 when any check is `failed`, always with the body below.

```json
{
  "service": "passwords",
  "version": "3f9c1a2",
  "status": "ok",
  "checked_at": "2026-11-13T14:02:11Z",
  "checks": {
    "database": { "status": "ok", "latency_ms": 3 },
    "faktory":  { "status": "ok", "latency_ms": 2 },
    "redis":    { "status": "ok", "latency_ms": 1 },
    "worker":   { "status": "ok", "heartbeat_age_s": 17 }
  }
}
```

A check that answers in more than half its timeout is `degraded`. A failed check carries a short `error` class
(`connect`, `query`, `timeout`, `unexpected_reply`, `missing`, `stale`), never a message.

## Installation

```toml
[dependencies]
healthcheck = { git = "https://github.com/abdulsemiu-atanda/healthcheck.git", rev = "<rev>", features = ["database", "keystore"] }
```

Features:

- `database`: the Postgres check (`checks::Database`), built on a diesel `r2d2` pool.
- `http`: a `GET` probe of another service (`checks::Http`), passing on a 2xx. It lets a public service report on
  private ones only it can reach. The client has no TLS, so it is for plain http hosts on the private network.
- `keystore`: the Redis check (`checks::Redis`), the worker heartbeat check (`checks::Worker`) and the heartbeat writer
  (`heartbeat::beat`).

The Faktory check (`checks::Faktory`) needs no feature.

## Usage

In the web server, build one `Health` and share it across actix workers:

```rust
let health = web::Data::new(
  Health::new("passwords", &version, &dotenvy::var("PROBE_TOKEN").unwrap_or_default())
    .check(checks::Database::new(pool.pool.clone()))
    .check(checks::Redis::new(redis.clone()))
    .check(checks::Faktory::new(&settings.faktory.url()))
    .check(checks::Worker::new(redis.clone(), "passwords")),
);

App::new().app_data(health.clone())
```

Mount the route next to the existing liveness route:

```rust
.service(web::resource("healthcheck/details").guard(guard::Get()).to(healthcheck::details))
```

In the worker process, beat alongside the job loop so the heartbeat stops when the jobs do:

```rust
tokio::select! {
  _ = workers.join_all() => {}
  _ = healthcheck::heartbeat::beat(redis, "passwords") => {}
}
```

A custom check implements `healthcheck::Check`.
