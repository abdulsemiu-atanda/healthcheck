use diesel::{
  PgConnection, RunQueryDsl,
  r2d2::{ConnectionManager, Pool},
};
use std::time::Duration;

use crate::check::{Check, Failure, Reading};

const TIMEOUT: Duration = Duration::from_millis(500);

pub struct Database {
  pool: Pool<ConnectionManager<PgConnection>>,
}

impl Database {
  /// Takes the service's own pool, so the check sees the same exhaustion the
  /// service's requests would.
  pub fn new(pool: Pool<ConnectionManager<PgConnection>>) -> Self {
    Self { pool }
  }
}

#[async_trait::async_trait]
impl Check for Database {
  fn name(&self) -> &str {
    "database"
  }

  fn timeout(&self) -> Duration {
    TIMEOUT
  }

  /// Diesel is synchronous, so the query runs on the blocking pool. The checkout
  /// carries its own timeout because the route's timeout cannot cancel a blocking
  /// thread, only stop waiting for it.
  async fn probe(&self) -> Result<Reading, Failure> {
    let pool = self.pool.clone();

    tokio::task::spawn_blocking(move || {
      let mut connection = pool.get_timeout(TIMEOUT).map_err(|_| Failure::CONNECT)?;

      diesel::sql_query("SELECT 1")
        .execute(&mut connection)
        .map_err(|_| Failure::QUERY)?;

      Ok(Reading::Responded)
    })
    .await
    .unwrap_or(Err(Failure::INTERNAL))
  }
}
