use futures_util::future::join_all;
use std::{
  collections::BTreeMap,
  time::{Duration, Instant},
};

use super::report::{CheckResult, Status};

/// No round of checks may hold the route longer than this, whatever the
/// individual timeouts say.
pub const ROUND_DEADLINE: Duration = Duration::from_secs(2);

/// What a dependency answered when it answered as expected.
#[derive(Debug, PartialEq, Eq)]
pub enum Reading {
  Responded,
  Heartbeat { age_s: u64 },
}

/// A short error class, never a message: the details body is private, but it
/// still should not carry hostnames or driver output.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Failure(pub &'static str);

impl Failure {
  pub const CONNECT: Self = Self("connect");
  pub const QUERY: Self = Self("query");
  pub const UNEXPECTED_REPLY: Self = Self("unexpected_reply");
  pub const MISSING: Self = Self("missing");
  pub const STALE: Self = Self("stale");
  pub const TIMEOUT: Self = Self("timeout");
  pub const INTERNAL: Self = Self("internal");
}

#[async_trait::async_trait]
pub trait Check: Send + Sync {
  /// The key this check reports under, e.g. `database` or `upstream.auth`.
  fn name(&self) -> &str;

  fn timeout(&self) -> Duration;

  async fn probe(&self) -> Result<Reading, Failure>;
}

pub async fn run_all(checks: &[Box<dyn Check>]) -> BTreeMap<String, CheckResult> {
  let results = join_all(checks.iter().map(|check| run_one(check.as_ref()))).await;

  checks
    .iter()
    .map(|check| check.name().to_owned())
    .zip(results)
    .collect()
}

/// A dependency that answers in more than half its timeout is working but slow.
async fn run_one(check: &dyn Check) -> CheckResult {
  let timeout = check.timeout().min(ROUND_DEADLINE);
  let started = Instant::now();
  let outcome = tokio::time::timeout(timeout, check.probe())
    .await
    .unwrap_or(Err(Failure::TIMEOUT));
  let elapsed = started.elapsed();

  match outcome {
    Ok(Reading::Responded) => CheckResult {
      status: if elapsed > timeout / 2 {
        Status::Degraded
      } else {
        Status::Ok
      },
      latency_ms: Some(elapsed.as_millis() as u64),
      heartbeat_age_s: None,
      error: None,
    },
    Ok(Reading::Heartbeat { age_s }) => CheckResult {
      status: Status::Ok,
      latency_ms: None,
      heartbeat_age_s: Some(age_s),
      error: None,
    },
    Err(Failure(error)) => CheckResult {
      status: Status::Failed,
      latency_ms: None,
      heartbeat_age_s: None,
      error: Some(error),
    },
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use super::*;
  use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  };

  pub struct Stub {
    pub name: &'static str,
    pub delay: Duration,
    pub outcome: Result<Reading, Failure>,
    pub calls: Arc<AtomicUsize>,
  }

  impl Stub {
    pub fn answering(name: &'static str, outcome: Result<Reading, Failure>) -> Self {
      Self {
        name,
        delay: Duration::ZERO,
        outcome,
        calls: Arc::default(),
      }
    }
  }

  #[async_trait::async_trait]
  impl Check for Stub {
    fn name(&self) -> &str {
      self.name
    }

    fn timeout(&self) -> Duration {
      Duration::from_millis(200)
    }

    async fn probe(&self) -> Result<Reading, Failure> {
      self.calls.fetch_add(1, Ordering::SeqCst);
      tokio::time::sleep(self.delay).await;

      match &self.outcome {
        Ok(Reading::Responded) => Ok(Reading::Responded),
        Ok(Reading::Heartbeat { age_s }) => Ok(Reading::Heartbeat { age_s: *age_s }),
        Err(failure) => Err(*failure),
      }
    }
  }

  #[tokio::test]
  async fn reports_every_check_under_its_name() {
    let checks: Vec<Box<dyn Check>> = vec![
      Box::new(Stub::answering("database", Ok(Reading::Responded))),
      Box::new(Stub::answering("worker", Ok(Reading::Heartbeat { age_s: 12 }))),
      Box::new(Stub::answering("redis", Err(Failure::CONNECT))),
    ];

    let results = run_all(&checks).await;

    assert_eq!(results["database"].status, Status::Ok);
    assert_eq!(results["worker"].heartbeat_age_s, Some(12));
    assert_eq!(results["redis"].status, Status::Failed);
    assert_eq!(results["redis"].error, Some("connect"));
  }

  #[tokio::test]
  async fn a_check_past_its_timeout_fails_as_timeout() {
    let checks: Vec<Box<dyn Check>> = vec![Box::new(Stub {
      delay: Duration::from_secs(5),
      ..Stub::answering("database", Ok(Reading::Responded))
    })];

    let results = run_all(&checks).await;

    assert_eq!(results["database"].status, Status::Failed);
    assert_eq!(results["database"].error, Some("timeout"));
  }

  #[tokio::test]
  async fn a_slow_answer_is_degraded() {
    let checks: Vec<Box<dyn Check>> = vec![Box::new(Stub {
      delay: Duration::from_millis(150),
      ..Stub::answering("database", Ok(Reading::Responded))
    })];

    let results = run_all(&checks).await;

    assert_eq!(results["database"].status, Status::Degraded);
  }

  #[tokio::test]
  async fn checks_run_concurrently() {
    let slow = || {
      Box::new(Stub {
        delay: Duration::from_millis(80),
        ..Stub::answering("database", Ok(Reading::Responded))
      }) as Box<dyn Check>
    };
    let checks = vec![slow(), slow(), slow()];
    let started = Instant::now();

    run_all(&checks).await;

    assert!(started.elapsed() < Duration::from_millis(200));
  }
}
