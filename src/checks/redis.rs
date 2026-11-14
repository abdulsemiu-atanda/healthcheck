use redis::Client;
use std::time::Duration;

use crate::check::{Check, Failure, Reading};

const TIMEOUT: Duration = Duration::from_millis(300);

pub struct Redis {
  client: Client,
}

impl Redis {
  pub fn new(client: Client) -> Self {
    Self { client }
  }
}

#[async_trait::async_trait]
impl Check for Redis {
  fn name(&self) -> &str {
    "redis"
  }

  fn timeout(&self) -> Duration {
    TIMEOUT
  }

  async fn probe(&self) -> Result<Reading, Failure> {
    let client = self.client.clone();

    tokio::task::spawn_blocking(move || {
      let mut connection = client
        .get_connection_with_timeout(TIMEOUT)
        .map_err(|_| Failure::CONNECT)?;
      connection
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|_| Failure::CONNECT)?;
      let reply: String = redis::cmd("PING").query(&mut connection).map_err(|_| Failure::QUERY)?;

      if reply == "PONG" {
        Ok(Reading::Responded)
      } else {
        Err(Failure::UNEXPECTED_REPLY)
      }
    })
    .await
    .unwrap_or(Err(Failure::INTERNAL))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn nothing_listening_fails_to_connect() {
    let client = Client::open("redis://127.0.0.1:1").unwrap();

    assert_eq!(Redis::new(client).probe().await, Err(Failure::CONNECT));
  }
}
