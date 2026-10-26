//! The result file: `ci-bench/1` JSON, one record per (arm, fixture, scenario, repeat).
//!
//! The same format holds every arm of the head-to-head comparison: the competitor runners
//! (written by this driver) and any other system measured on the same fixtures and scenarios
//! (its own tool writes `arm`, `provider` and the measures it has). Consumers must accept
//! records whose optional measures are absent.
//!
//! ```json
//! {"schema": "ci-bench/1", "date": "2026-10-26", "label": "smoke",
//!  "source": {"tool": "ci-bench", "version": "0.1.0", "repo": "HelixDB/ci-bench", "base_commit": "…"},
//!  "runs": [{"arm": "ubuntu-latest", "provider": "github", "arch": "x86_64", "vcpus": 4,
//!            "fixture": "go-small", "scenario": "leaf", "repeat": 1, "outcome": "success",
//!            "url": "https://github.com/HelixDB/ci-bench/actions/runs/1", "head_sha": "…",
//!            "times_s": {"queue": 3.0, "start": 14.0, "verdict": 55.0, "total": 60.0},
//!            "billed_s": 60.0, "usd": 0.006, "usd_per_minute": 0.006, "pricing_as_of": "2026-10-26"}]}
//! ```

use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::timing::{Outcome, Times};

/// The format's name and version.
pub const SCHEMA: &str = "ci-bench/1";

/// Who wrote the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// The tool (`ci-bench`).
    pub tool: String,
    /// Its version.
    pub version: String,
    /// The benchmark repository (`owner/name`).
    pub repo: String,
    /// The commit of its `main` the scenario branches were cut from.
    pub base_commit: String,
}

/// One measured run of one arm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmRun {
    /// The arm: a runner label (`ubuntu-latest`, …) or another system's name.
    pub arm: String,
    /// Who runs it (`github`, `blacksmith`, …).
    pub provider: String,
    /// `x86_64` or `aarch64`.
    pub arch: String,
    /// vCPUs the job had.
    pub vcpus: u32,
    /// The fixture.
    pub fixture: String,
    /// The scenario.
    pub scenario: String,
    /// 1-based repeat of this (arm, fixture, scenario).
    pub repeat: u32,
    /// How it ended.
    pub outcome: Outcome,
    /// Where to look at it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    /// The commit tested.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub head_sha: Option<String>,
    /// The measures.
    pub times_s: Times,
    /// Billed duration after the provider's rounding.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub billed_s: Option<f64>,
    /// List-price cost.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usd: Option<f64>,
    /// The price used.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usd_per_minute: Option<f64>,
    /// The price table used.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pricing_as_of: Option<String>,
    /// Why something is missing or unusual.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
}

/// A result file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// Always [`SCHEMA`].
    pub schema: String,
    /// The day of the first run (GitHub's clock, UTC).
    pub date: String,
    /// What the batch was (`smoke`, `full`, …).
    pub label: String,
    /// Who wrote it.
    pub source: Source,
    /// Every run.
    pub runs: Vec<ArmRun>,
}

/// Why a result file is unusable.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// IO.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// Why.
        source: std::io::Error,
    },
    /// Not JSON of this format.
    #[error("{path}: {source}")]
    Json {
        /// The file.
        path: String,
        /// Why.
        source: serde_json::Error,
    },
    /// Another format.
    #[error("{path}: schema {found:?}, expected {SCHEMA}")]
    Schema {
        /// The file.
        path: String,
        /// What it says.
        found: String,
    },
}

impl Report {
    /// Writes the report as pretty JSON, refusing to overwrite.
    ///
    /// # Errors
    /// [`ReportError::Io`] (an existing file included).
    pub fn write_new(&self, path: &Path) -> Result<(), ReportError> {
        use std::io::Write as _;
        let io = |source| ReportError::Io {
            path: path.display().to_string(),
            source,
        };
        let json = serde_json::to_string_pretty(self).map_err(|source| ReportError::Json {
            path: path.display().to_string(),
            source,
        })? + "\n";
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(io)?;
        file.write_all(json.as_bytes()).map_err(io)
    }

    /// Reads a report.
    ///
    /// # Errors
    /// [`ReportError`].
    pub fn load(path: &Path) -> Result<Self, ReportError> {
        let p = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|source| ReportError::Io {
            path: p.clone(),
            source,
        })?;
        let report: Self = serde_json::from_str(&text).map_err(|source| ReportError::Json {
            path: p.clone(),
            source,
        })?;
        if report.schema != SCHEMA {
            return Err(ReportError::Schema {
                path: p,
                found: report.schema,
            });
        }
        Ok(report)
    }

    /// One line per run: arm, fixture, scenario, outcome, verdict and cost.
    ///
    /// ```
    /// let r = ci_bench::report::tests_sample();
    /// assert!(r.table().contains("ubuntu-latest"));
    /// ```
    #[must_use]
    pub fn table(&self) -> String {
        let secs = |v: Option<f64>| v.map_or_else(|| "-".to_owned(), |s| format!("{s:.0}s"));
        let mut out = String::from(
            "arm                               fixture       scenario   outcome    start  verdict  total  usd\n",
        );
        for r in &self.runs {
            let _ = writeln!(
                out,
                "{:<33} {:<13} {:<10} {:<10} {:>5}  {:>7}  {:>5}  {}",
                r.arm,
                r.fixture,
                r.scenario,
                serde_json::to_value(r.outcome)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                secs(r.times_s.start),
                secs(r.times_s.verdict),
                secs(r.times_s.total),
                r.usd.map_or_else(|| "-".to_owned(), |u| format!("{u:.4}")),
            );
        }
        out
    }
}

/// A small report (used by doc examples and tests).
#[doc(hidden)]
#[must_use]
pub fn tests_sample() -> Report {
    Report {
        schema: SCHEMA.to_owned(),
        date: "2026-10-26".to_owned(),
        label: "smoke".to_owned(),
        source: Source {
            tool: "ci-bench".to_owned(),
            version: "0.1.0".to_owned(),
            repo: "HelixDB/ci-bench".to_owned(),
            base_commit: "0".repeat(40),
        },
        runs: vec![ArmRun {
            arm: "ubuntu-latest".to_owned(),
            provider: "github".to_owned(),
            arch: "x86_64".to_owned(),
            vcpus: 4,
            fixture: "go-small".to_owned(),
            scenario: "leaf".to_owned(),
            repeat: 1,
            outcome: Outcome::Success,
            url: Some("https://github.com/HelixDB/ci-bench/actions/runs/1".to_owned()),
            head_sha: Some("1".repeat(40)),
            times_s: Times {
                queue: Some(3.0),
                start: Some(14.0),
                verdict: Some(55.0),
                total: Some(60.0),
                ..Times::default()
            },
            billed_s: Some(60.0),
            usd: Some(0.006),
            usd_per_minute: Some(0.006),
            pricing_as_of: Some("2026-10-26".to_owned()),
            note: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The golden file other tools parse (`crucible-e2e bench versus import` keeps a copy).
    const GOLDEN: &str = include_str!("../tests/sample-report.json");

    #[test]
    fn the_golden_file_is_the_format() {
        let parsed: Report = serde_json::from_str(GOLDEN).unwrap();
        assert_eq!(parsed, tests_sample());
        assert_eq!(
            serde_json::to_string_pretty(&parsed).unwrap() + "\n",
            GOLDEN
        );
    }

    #[test]
    fn write_load_and_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("r.json");
        tests_sample().write_new(&path).unwrap();
        assert_eq!(Report::load(&path).unwrap(), tests_sample());
        assert!(
            tests_sample().write_new(&path).is_err(),
            "never overwritten"
        );
        std::fs::write(&path, GOLDEN.replace("ci-bench/1", "ci-bench/2")).unwrap();
        assert!(matches!(
            Report::load(&path),
            Err(ReportError::Schema { .. })
        ));
        std::fs::write(&path, "{").unwrap();
        assert!(matches!(Report::load(&path), Err(ReportError::Json { .. })));
        assert!(matches!(
            Report::load(&tmp.path().join("none")),
            Err(ReportError::Io { .. })
        ));
        let table = tests_sample().table();
        let row = table.lines().nth(1).unwrap();
        let cells: Vec<&str> = row.split_whitespace().collect();
        assert_eq!(
            cells,
            [
                "ubuntu-latest",
                "go-small",
                "leaf",
                "success",
                "14s",
                "55s",
                "60s",
                "0.0060"
            ]
        );
    }
}
