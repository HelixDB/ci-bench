//! Timings of one benchmark run from GitHub's own timestamps (the Actions runs and jobs APIs);
//! the driver's clock is never used.
//!
//! | Measure | From → to |
//! |---|---|
//! | `queue` | the run created (the CI request) → the job started on a runner |
//! | `setup` | GitHub's `Set up job` step |
//! | `checkout` | the `checkout` step |
//! | `toolchain` | the `toolchain: …` steps (setup and cache restore) |
//! | `install` / `test` | those steps |
//! | `post` | the `Post …` steps (cache saves) and `Complete job` |
//! | `start` | the run created → the `install` step started: queue, boot, checkout and cache restore before the first project command |
//! | `verdict` | the run created → the `test` step completed: when the result is known |
//! | `total` | the run created → the job completed |
//!
//! Timestamps are RFC 3339 with `Z` or an offset, to the second or finer. A measure whose
//! timestamps are missing (a cancelled job, a skipped step) is `None`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Seconds since the Unix epoch of an RFC 3339 timestamp (`2026-10-26T12:00:05Z`,
/// `…05.123-07:00`).
///
/// ```
/// use ci_bench::timing::unix;
/// assert_eq!(unix("1970-01-01T00:00:00Z"), Some(0.0));
/// assert_eq!(unix("2026-10-26T12:00:05Z"), Some(1_793_016_005.0));
/// assert_eq!(unix("2026-10-26T05:00:05.500-07:00"), Some(1_793_016_005.5));
/// assert_eq!(unix("2026-10-26 12:00:05"), None);
/// assert_eq!(unix("2026-13-01T00:00:00Z"), None);
/// ```
#[must_use]
pub fn unix(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    let digits = |r: std::ops::Range<usize>| -> Option<i64> {
        let s = text.get(r)?;
        s.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| s.parse().ok())
            .flatten()
    };
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &text[19..];
    let mut fraction = 0.0;
    if let Some(after) = rest.strip_prefix('.') {
        let n = after.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return None;
        }
        fraction = format!("0.{}", &after[..n]).parse().ok()?;
        rest = &after[n..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hh = rest.get(1..3)?.parse::<i64>().ok()?;
            let mm = rest.get(4..6)?.parse::<i64>().ok()?;
            if rest.len() != 6 || rest.as_bytes()[3] != b':' || hh > 23 || mm > 59 {
                return None;
            }
            sign * (hh * 3600 + mm * 60)
        }
    };
    // Howard Hinnant's days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second - offset;
    #[allow(clippy::cast_precision_loss)] // seconds since 1970 fit an f64 exactly
    Some(secs as f64 + fraction)
}

/// The `YYYY-MM-DD` of a timestamp as GitHub wrote it (UTC timestamps only, which is what the
/// runs API returns for `created_at`).
///
/// ```
/// assert_eq!(ci_bench::timing::day("2026-10-26T23:59:59Z"), Some("2026-10-26"));
/// assert_eq!(ci_bench::timing::day("2026-10-26T23:59:59-07:00"), None);
/// ```
#[must_use]
pub fn day(text: &str) -> Option<&str> {
    (text.ends_with('Z') && unix(text).is_some()).then(|| &text[..10])
}

/// The UTC day (`YYYY-MM-DD`) of a Unix time.
///
/// ```
/// assert_eq!(ci_bench::timing::day_of(1_793_016_005), "2026-10-26");
/// assert_eq!(ci_bench::timing::day_of(0), "1970-01-01");
/// ```
#[must_use]
pub fn day_of(unix_secs: u64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = i64::try_from(unix_secs / 86_400).unwrap_or(i64::MAX / 2) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The tests passed.
    Success,
    /// A step failed.
    Failure,
    /// Cancelled (by the driver after its queue or run timeout, or by a person).
    Cancelled,
    /// GitHub's job timeout.
    TimedOut,
    /// Anything else (`startup_failure`, `action_required`, …), or no run.
    Error,
}

impl Outcome {
    /// The outcome of an Actions `conclusion`.
    ///
    /// ```
    /// use ci_bench::timing::Outcome;
    /// assert_eq!(Outcome::of("success"), Outcome::Success);
    /// assert_eq!(Outcome::of("timed_out"), Outcome::TimedOut);
    /// assert_eq!(Outcome::of("startup_failure"), Outcome::Error);
    /// ```
    #[must_use]
    pub fn of(conclusion: &str) -> Self {
        match conclusion {
            "success" => Self::Success,
            "failure" => Self::Failure,
            "cancelled" => Self::Cancelled,
            "timed_out" => Self::TimedOut,
            _ => Self::Error,
        }
    }
}

/// The measures of the module docs, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Times {
    /// Run created → job started.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub queue: Option<f64>,
    /// `Set up job`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub setup: Option<f64>,
    /// `checkout`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub checkout: Option<f64>,
    /// The `toolchain: …` steps.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub toolchain: Option<f64>,
    /// `install`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub install: Option<f64>,
    /// `test`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub test: Option<f64>,
    /// `Post …` and `Complete job`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub post: Option<f64>,
    /// Run created → `install` started.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub start: Option<f64>,
    /// Run created → `test` completed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub verdict: Option<f64>,
    /// Run created → job completed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total: Option<f64>,
}

/// What one run's JSON says.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    /// The run's conclusion.
    pub outcome: Outcome,
    /// The measures.
    pub times: Times,
    /// Job started → completed: what GitHub bills (before rounding).
    pub job_s: Option<f64>,
    /// The run's day (UTC, `YYYY-MM-DD`), from its `created_at`.
    pub day: Option<String>,
}

fn span(from: &Value, to: &Value) -> Option<f64> {
    let (a, b) = (unix(from.as_str()?)?, unix(to.as_str()?)?);
    (b >= a).then_some(b - a)
}

/// Measures a run from its `GET /actions/runs/{id}` JSON and its job (one element of `GET
/// /actions/runs/{id}/jobs`).
///
/// ```
/// use serde_json::json;
/// let run = json!({"created_at": "2026-10-26T12:00:00Z", "status": "completed", "conclusion": "success"});
/// let job = json!({"started_at": "2026-10-26T12:00:04Z", "completed_at": "2026-10-26T12:01:00Z", "steps": [
///   {"name": "Set up job", "started_at": "2026-10-26T12:00:04Z", "completed_at": "2026-10-26T12:00:06Z"},
///   {"name": "checkout", "started_at": "2026-10-26T12:00:06Z", "completed_at": "2026-10-26T12:00:08Z"},
///   {"name": "toolchain: go", "started_at": "2026-10-26T12:00:08Z", "completed_at": "2026-10-26T12:00:14Z"},
///   {"name": "install", "started_at": "2026-10-26T12:00:14Z", "completed_at": "2026-10-26T12:00:40Z"},
///   {"name": "test", "started_at": "2026-10-26T12:00:40Z", "completed_at": "2026-10-26T12:00:55Z"},
///   {"name": "Post toolchain: go", "started_at": "2026-10-26T12:00:55Z", "completed_at": "2026-10-26T12:00:59Z"},
///   {"name": "Complete job", "started_at": "2026-10-26T12:00:59Z", "completed_at": "2026-10-26T12:01:00Z"}]});
/// let m = ci_bench::timing::measure(&run, &job);
/// assert_eq!(m.outcome, ci_bench::timing::Outcome::Success);
/// assert_eq!((m.times.queue, m.times.start, m.times.verdict, m.times.total), (Some(4.0), Some(14.0), Some(55.0), Some(60.0)));
/// assert_eq!((m.times.toolchain, m.times.post, m.job_s), (Some(6.0), Some(5.0), Some(56.0)));
/// ```
#[must_use]
pub fn measure(run: &Value, job: &Value) -> Measured {
    let created = &run["created_at"];
    let steps: &[Value] = job["steps"].as_array().map_or(&[], Vec::as_slice);
    let named = |name: &str| steps.iter().find(|s| s["name"] == name);
    let step = |name: &str| named(name).and_then(|s| span(&s["started_at"], &s["completed_at"]));
    let sum = |keep: &dyn Fn(&str) -> bool| {
        let spans: Vec<f64> = steps
            .iter()
            .filter(|s| s["name"].as_str().is_some_and(keep))
            .filter_map(|s| span(&s["started_at"], &s["completed_at"]))
            .collect();
        (!spans.is_empty()).then(|| spans.iter().sum())
    };
    let conclusion = run["conclusion"].as_str().unwrap_or("");
    let outcome = if run["status"] == "completed" {
        Outcome::of(conclusion)
    } else {
        Outcome::Error
    };
    Measured {
        outcome,
        times: Times {
            queue: span(created, &job["started_at"]),
            setup: step("Set up job"),
            checkout: step("checkout"),
            toolchain: sum(&|n| n.starts_with("toolchain: ")),
            install: step("install"),
            test: step("test"),
            post: sum(&|n| n.starts_with("Post ") || n == "Complete job"),
            start: named("install").and_then(|s| span(created, &s["started_at"])),
            verdict: named("test").and_then(|s| span(created, &s["completed_at"])),
            total: span(created, &job["completed_at"]),
        },
        job_s: span(&job["started_at"], &job["completed_at"]),
        day: created.as_str().and_then(day).map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn missing_and_reversed_timestamps_are_none() {
        let run = json!({"created_at": "2026-10-26T12:00:00Z", "status": "completed", "conclusion": "cancelled"});
        let job = json!({"started_at": "2026-10-26T12:00:10Z", "completed_at": null, "steps": [
            {"name": "Set up job", "started_at": "2026-10-26T12:00:10Z", "completed_at": "2026-10-26T12:00:09Z"},
            {"name": "install", "started_at": null, "completed_at": null},
        ]});
        let m = measure(&run, &job);
        assert_eq!(m.outcome, Outcome::Cancelled);
        assert_eq!(m.times.queue, Some(10.0));
        assert_eq!(m.times.setup, None, "reversed");
        assert_eq!(
            (m.times.install, m.times.start, m.times.total, m.job_s),
            (None, None, None, None)
        );
        assert_eq!(m.times.toolchain, None, "no such steps");
        assert_eq!(m.day.as_deref(), Some("2026-10-26"));
        let running = json!({"created_at": "2026-10-26T12:00:00Z", "status": "in_progress", "conclusion": null});
        assert_eq!(measure(&running, &json!({})).outcome, Outcome::Error);
        assert_eq!(measure(&json!({}), &json!({})).day, None);
    }

    #[test]
    fn timestamps() {
        assert_eq!(
            unix("2026-10-26T12:00:05+01:30"),
            Some(1_793_016_005.0 - 5400.0)
        );
        assert_eq!(unix("2024-02-29T00:00:00Z"), Some(1_709_164_800.0));
        for bad in [
            "",
            "2026-10-26T12:00:05",
            "2026-10-26T12:00:05.Z",
            "2026-10-26T12:00:05+0100",
            "2026-10-26T24:00:05Z",
            "2026-10-26T12:60:05Z",
            "2026-1a-26T12:00:05Z",
            "2026-10-26T12:00:05+01:3x",
        ] {
            assert_eq!(unix(bad), None, "{bad}");
        }
        assert_eq!(Outcome::of("failure"), Outcome::Failure);
        assert_eq!(Outcome::of("cancelled"), Outcome::Cancelled);
        assert_eq!(
            serde_json::to_string(&Outcome::TimedOut).unwrap(),
            "\"timed_out\""
        );
        let empty = serde_json::to_string(&Times::default()).unwrap();
        assert_eq!(empty, "{}");
    }
}
