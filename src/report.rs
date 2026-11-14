use serde::Serialize;
use std::collections::BTreeMap;

/// Declared best to worst, so the derived ordering makes `max` the worst status.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Status {
  Ok,
  Degraded,
  Failed,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct CheckResult {
  pub status: Status,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub latency_ms: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub heartbeat_age_s: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<&'static str>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Report {
  pub service: String,
  pub version: String,
  pub status: Status,
  pub checked_at: String,
  pub checks: BTreeMap<String, CheckResult>,
}

impl Report {
  pub fn new(service: &str, version: &str, checks: BTreeMap<String, CheckResult>) -> Self {
    let status = checks.values().map(|check| check.status).max().unwrap_or(Status::Ok);

    Self {
      service: service.to_owned(),
      version: version.to_owned(),
      status,
      checked_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
      checks,
    }
  }

  pub fn is_failed(&self) -> bool {
    self.status == Status::Failed
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn result(status: Status) -> CheckResult {
    CheckResult {
      status,
      latency_ms: None,
      heartbeat_age_s: None,
      error: None,
    }
  }

  #[test]
  fn overall_status_is_the_worst_check() {
    let checks = BTreeMap::from([
      ("database".to_owned(), result(Status::Ok)),
      ("redis".to_owned(), result(Status::Degraded)),
      ("faktory".to_owned(), result(Status::Failed)),
    ]);

    let report = Report::new("passwords", "abc123", checks);

    assert_eq!(report.status, Status::Failed);
    assert!(report.is_failed());
  }

  #[test]
  fn a_service_with_no_checks_is_ok() {
    let report = Report::new("metadata", "abc123", BTreeMap::new());

    assert_eq!(report.status, Status::Ok);
  }

  #[test]
  fn serializes_in_the_documented_shape() {
    let checks = BTreeMap::from([
      (
        "database".to_owned(),
        CheckResult {
          latency_ms: Some(3),
          ..result(Status::Ok)
        },
      ),
      (
        "worker".to_owned(),
        CheckResult {
          heartbeat_age_s: Some(17),
          ..result(Status::Ok)
        },
      ),
      (
        "redis".to_owned(),
        CheckResult {
          error: Some("connect"),
          ..result(Status::Failed)
        },
      ),
    ]);

    let body = serde_json::to_value(Report::new("passwords", "abc123", checks)).unwrap();

    assert_eq!(body["status"], "failed");
    assert_eq!(
      body["checks"]["database"],
      serde_json::json!({ "status": "ok", "latency_ms": 3 })
    );
    assert_eq!(
      body["checks"]["worker"],
      serde_json::json!({ "status": "ok", "heartbeat_age_s": 17 })
    );
    assert_eq!(
      body["checks"]["redis"],
      serde_json::json!({ "status": "failed", "error": "connect" })
    );
    assert!(body["checked_at"].as_str().unwrap().ends_with('Z'));
  }
}
