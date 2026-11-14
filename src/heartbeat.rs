use log::error;
use redis::{Client, Commands};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const INTERVAL: Duration = Duration::from_secs(30);

/// A heartbeat older than this, or none at all, means the worker is down. The key
/// expires at the same age, so a dead worker leaves nothing behind.
pub const MAX_AGE: Duration = Duration::from_secs(120);

pub fn heartbeat_key(service: &str) -> String {
  format!("status:heartbeat:{service}")
}

pub fn unix_now() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|elapsed| elapsed.as_secs())
    .unwrap_or_default()
}

/// Writes the worker's heartbeat every 30 s, forever. Run it beside the job loop
/// so that it stops when the loop does, e.g. in a `tokio::select!` with it; a
/// heartbeat that outlives the jobs it vouches for is worse than none.
pub async fn beat(client: Client, service: &str) {
  let key = heartbeat_key(service);
  let mut ticks = tokio::time::interval(INTERVAL);

  loop {
    ticks.tick().await;

    let client = client.clone();
    let key = key.clone();
    let written = tokio::task::spawn_blocking(move || {
      let mut connection = client.get_connection()?;
      connection.set_ex::<_, _, ()>(&key, unix_now(), MAX_AGE.as_secs())
    })
    .await;

    match written {
      Ok(Ok(())) => {}
      Ok(Err(err)) => error!("Failed to write worker heartbeat: {err}"),
      Err(err) => error!("Worker heartbeat task failed: {err}"),
    }
  }
}
