use redis::{Client, Commands};
use std::time::Duration;

use crate::{
  check::{Check, Failure, Reading},
  heartbeat::{MAX_AGE, heartbeat_key, unix_now},
};

const TIMEOUT: Duration = Duration::from_millis(300);

/// Reads the heartbeat a worker process writes with [`crate::heartbeat::beat`].
pub struct Worker {
  client: Client,
  key: String,
}

impl Worker {
  /// `service` must match the name the worker process beats under.
  pub fn new(client: Client, service: &str) -> Self {
    Self {
      client,
      key: heartbeat_key(service),
    }
  }
}

#[async_trait::async_trait]
impl Check for Worker {
  fn name(&self) -> &str {
    "worker"
  }

  fn timeout(&self) -> Duration {
    TIMEOUT
  }

  async fn probe(&self) -> Result<Reading, Failure> {
    let client = self.client.clone();
    let key = self.key.clone();

    let beat: Option<u64> = tokio::task::spawn_blocking(move || {
      let mut connection = client
        .get_connection_with_timeout(TIMEOUT)
        .map_err(|_| Failure::CONNECT)?;
      connection
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|_| Failure::CONNECT)?;

      connection.get(&key).map_err(|_| Failure::QUERY)
    })
    .await
    .unwrap_or(Err(Failure::INTERNAL))?;

    age_of(beat, unix_now())
  }
}

fn age_of(beat: Option<u64>, now: u64) -> Result<Reading, Failure> {
  let age_s = now.saturating_sub(beat.ok_or(Failure::MISSING)?);

  if age_s > MAX_AGE.as_secs() {
    Err(Failure::STALE)
  } else {
    Ok(Reading::Heartbeat { age_s })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_recent_beat_reports_its_age() {
    assert_eq!(age_of(Some(1_000), 1_017), Ok(Reading::Heartbeat { age_s: 17 }));
  }

  #[test]
  fn no_beat_means_the_worker_is_missing() {
    assert_eq!(age_of(None, 1_000), Err(Failure::MISSING));
  }

  #[test]
  fn a_beat_older_than_the_max_age_is_stale() {
    assert_eq!(age_of(Some(1_000), 1_121), Err(Failure::STALE));
  }
}
