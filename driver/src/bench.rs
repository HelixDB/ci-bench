//! One benchmark batch: every (fixture, scenario) cell, `repeats` times, on every runner.
//!
//! Per cell and repeat, in order:
//! 1. **commit:** `baseline` runs on `main` (whose runs save the caches every branch restores);
//!    a scripted scenario is committed on `bench/<tag>` from `main`'s head through the Git Data
//!    API ([`crate::actions::commit_branch`]).
//! 2. **dispatch** every runner's workflow on that ref with inputs `fixture`, `scenario`, `tag`
//!    (all at once: the runners are independent); a dispatch GitHub does not answer with the
//!    run's id is found by its tag within [`Patience::find`].
//! 3. **poll** every run each [`Patience::poll`] until completed. A run still queued after
//!    [`Patience::queued`], or unfinished after [`Patience::run`], is cancelled and recorded
//!    with a note (a cancelled run has no verdict).
//! 4. **measure** each run's job with GitHub's timestamps ([`crate::timing::measure`]) and price
//!    it with the table in force on the run's day.
//! 5. **clean up:** the scenario branch is deleted, also after a failure.
//!
//! Failures never abort the batch: a cell that cannot be committed or dispatched records an
//! `error` run per runner with the reason, and the next cell runs.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::json;
use tokio::time::Instant;

use crate::actions;
use crate::catalog::{Arch, Catalog, FixtureName, Runner};
use crate::github::{Api, Client, Method};
use crate::pricing::Pricing;
use crate::report::{ArmRun, Report, SCHEMA, Source};
use crate::scenario::{self, ScenarioName, Scenarios};
use crate::timing::{self, Outcome, Times};

/// One (fixture, scenario) of the batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The fixture.
    pub fixture: FixtureName,
    /// The scenario.
    pub scenario: ScenarioName,
}

/// What to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The benchmark repository (`owner/name`).
    pub repo: String,
    /// The batch's label (`smoke`, `full`).
    pub label: String,
    /// The runners.
    pub runners: Vec<Runner>,
    /// The cells, in run order.
    pub cells: Vec<Cell>,
    /// Repeats of every cell.
    pub repeats: u32,
    /// A unique prefix of this batch's tags and branches (`[a-z0-9-]`).
    pub id: String,
}

/// How long to wait for what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patience {
    /// Between polls.
    pub poll: Duration,
    /// For a dispatched run to appear.
    pub find: Duration,
    /// For a run to leave the queue.
    pub queued: Duration,
    /// For a run to complete.
    pub run: Duration,
}

impl Patience {
    /// Polls every 15 s; 2 minutes to find a run, 15 to start, 70 to finish (the workflows
    /// time out after 60).
    pub const STANDARD: Self = Self {
        poll: Duration::from_secs(15),
        find: Duration::from_mins(2),
        queued: Duration::from_mins(15),
        run: Duration::from_mins(70),
    };
}

/// A batch against GitHub.
pub struct Bench<'a, A> {
    /// The API.
    pub client: &'a Client<A>,
    /// The fixtures.
    pub catalog: &'a Catalog,
    /// The scenarios.
    pub scenarios: &'a Scenarios,
    /// The prices.
    pub pricing: &'a Pricing,
    /// What to run.
    pub plan: &'a Plan,
    /// Waits.
    pub patience: Patience,
}

/// Where a dispatched run is.
enum Follow {
    /// Dispatched; `None` until found by its tag.
    Dispatched(Option<u64>),
    /// Over a patience limit: cancel asked, waiting for it to complete.
    Cancelling(u64, String),
    /// Completed (`note`: why it was cancelled, if it was).
    Done(u64, serde_json::Value, Option<String>),
    /// Given up: no run to measure.
    Failed(String),
}

/// One runner's run being followed.
struct Pending {
    runner: Runner,
    state: Follow,
}

impl<A: Api> Bench<'_, A> {
    /// Runs the batch (see the module docs).
    pub async fn run(&self) -> Report {
        let repo = &self.plan.repo;
        let started = self
            .client
            .call(Method::Get, &format!("/repos/{repo}"), None)
            .await;
        let today = started
            .as_ref()
            .ok()
            .and_then(|r| r.date)
            .map(timing::day_of);
        let base = actions::head(self.client, repo, "main").await;
        let mut runs = Vec::new();
        for cell in &self.plan.cells {
            for repeat in 1..=self.plan.repeats {
                let base = match &base {
                    Ok(sha) => sha.as_str(),
                    Err(e) => {
                        runs.extend(self.failed(cell, repeat, &format!("main's head: {e}")));
                        continue;
                    }
                };
                runs.extend(self.cell(cell, repeat, base, today.as_deref()).await);
            }
        }
        Report {
            schema: SCHEMA.to_owned(),
            date: today.unwrap_or_else(|| "1970-01-01".to_owned()),
            label: self.plan.label.clone(),
            source: Source {
                tool: "ci-bench".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                repo: repo.clone(),
                base_commit: base.unwrap_or_default(),
            },
            runs,
        }
    }

    fn record(&self, cell: &Cell, repeat: u32, runner: Runner) -> ArmRun {
        ArmRun {
            arm: runner.label().to_owned(),
            provider: serde_json::to_value(runner.provider())
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            arch: match runner.arch() {
                Arch::X86_64 => "x86_64",
                Arch::Aarch64 => "aarch64",
            }
            .to_owned(),
            vcpus: self
                .pricing
                .rate("9999-12-31", runner)
                .map_or(0, |r| r.vcpus),
            fixture: cell.fixture.to_string(),
            scenario: cell.scenario.to_string(),
            repeat,
            outcome: Outcome::Error,
            url: None,
            head_sha: None,
            times_s: Times::default(),
            billed_s: None,
            usd: None,
            usd_per_minute: None,
            pricing_as_of: None,
            note: None,
        }
    }

    fn failed(&self, cell: &Cell, repeat: u32, why: &str) -> Vec<ArmRun> {
        self.plan
            .runners
            .iter()
            .map(|r| ArmRun {
                note: Some(why.to_owned()),
                ..self.record(cell, repeat, *r)
            })
            .collect()
    }

    /// The scenario's changes, read from `base` in the repository.
    async fn changes(&self, cell: &Cell, base: &str) -> Result<Vec<scenario::Change>, String> {
        let edits = self
            .scenarios
            .edits(cell.fixture.as_str(), cell.scenario)
            .ok_or_else(|| format!("no scenario {}/{}", cell.fixture, cell.scenario))?;
        let mut files = BTreeMap::new();
        for path in edits.iter().flat_map(scenario::Edit::paths) {
            let full = format!("fixtures/{}/{path}", cell.fixture);
            let bytes = actions::read_file(self.client, &self.plan.repo, base, &full)
                .await
                .map_err(|e| e.to_string())?;
            files.insert(path.as_str().to_owned(), bytes);
        }
        scenario::plan(edits, |p| Ok(files.get(p).cloned().flatten())).map_err(|e| e.to_string())
    }

    async fn cell(&self, cell: &Cell, repeat: u32, base: &str, today: Option<&str>) -> Vec<ArmRun> {
        let repo = &self.plan.repo;
        let tag = format!(
            "{}-{}-{}-{repeat}",
            self.plan.id, cell.fixture, cell.scenario
        );
        let branch = (cell.scenario != ScenarioName::Baseline).then(|| format!("bench/{tag}"));
        if let Some(branch) = &branch {
            let committed = match self.changes(cell, base).await {
                Ok(changes) => actions::commit_branch(
                    self.client,
                    repo,
                    base,
                    &format!("fixtures/{}", cell.fixture),
                    &changes,
                    branch,
                    &format!("ci-bench: {} {} ({tag})", cell.fixture, cell.scenario),
                )
                .await
                .map_err(|e| e.to_string()),
                Err(e) => Err(e),
            };
            if let Err(why) = committed {
                // A half-made branch (the ref call failed late) is removed too.
                let _ = actions::delete_branch(self.client, repo, branch).await;
                return self.failed(cell, repeat, &format!("commit: {why}"));
            }
        }
        let git_ref = branch.as_deref().unwrap_or("main");
        let runs = self.follow(cell, &tag, git_ref).await;
        let mut out = Vec::new();
        for p in runs {
            out.push(self.measure(cell, repeat, p, today).await);
        }
        if let Some(branch) = &branch
            && let Err(e) = actions::delete_branch(self.client, repo, branch).await
        {
            for r in &mut out {
                let note = format!("branch {branch} left behind: {e}");
                r.note = Some(
                    r.note
                        .take()
                        .map_or(note.clone(), |n| format!("{n}; {note}")),
                );
            }
        }
        out
    }

    /// Dispatches every runner and polls until each run is done or given up.
    async fn follow(&self, cell: &Cell, tag: &str, git_ref: &str) -> Vec<Pending> {
        let repo = &self.plan.repo;
        let inputs = json!({"fixture": cell.fixture.as_str(), "scenario": cell.scenario.as_str(), "tag": tag});
        let mut pending = Vec::new();
        for runner in &self.plan.runners {
            let workflow = runner.workflow();
            let state =
                match actions::dispatch(self.client, repo, &workflow, git_ref, &inputs).await {
                    Ok(id) => Follow::Dispatched(id),
                    Err(e) => Follow::Failed(format!("dispatch: {e}")),
                };
            pending.push(Pending {
                runner: *runner,
                state,
            });
        }
        let start = Instant::now();
        // Cancelled runs normally complete within seconds; this bounds the wait if one never does.
        let give_up = self.patience.run + self.patience.find;
        loop {
            for p in &mut pending {
                let elapsed = start.elapsed();
                p.state = match std::mem::replace(&mut p.state, Follow::Failed(String::new())) {
                    Follow::Dispatched(None) => {
                        match actions::find_run(
                            self.client,
                            repo,
                            &p.runner.workflow(),
                            git_ref,
                            tag,
                        )
                        .await
                        {
                            Ok(Some(id)) => Follow::Dispatched(Some(id)),
                            Ok(None) if elapsed > self.patience.find => {
                                Follow::Failed(format!("no run tagged {tag} appeared"))
                            }
                            Ok(None) => Follow::Dispatched(None),
                            Err(e) => Follow::Failed(format!("finding the run: {e}")),
                        }
                    }
                    Follow::Dispatched(Some(id)) => match actions::run(self.client, repo, id).await
                    {
                        Ok(run) if run["status"] == "completed" => Follow::Done(id, run, None),
                        Ok(run) => {
                            let status = run["status"].as_str().unwrap_or("");
                            let waiting =
                                matches!(status, "queued" | "waiting" | "requested" | "pending");
                            let over = if waiting && elapsed > self.patience.queued {
                                Some(format!(
                                    "still {status} after {} s: cancelled",
                                    self.patience.queued.as_secs()
                                ))
                            } else if elapsed > self.patience.run {
                                Some(format!(
                                    "unfinished after {} s: cancelled",
                                    self.patience.run.as_secs()
                                ))
                            } else {
                                None
                            };
                            match over {
                                None => Follow::Dispatched(Some(id)),
                                Some(why) => match actions::cancel(self.client, repo, id).await {
                                    Ok(()) => Follow::Cancelling(id, why),
                                    Err(e) => Follow::Cancelling(
                                        id,
                                        format!("{why} (cancel failed: {e})"),
                                    ),
                                },
                            }
                        }
                        // A failed poll is retried at the next one.
                        Err(_) => Follow::Dispatched(Some(id)),
                    },
                    Follow::Cancelling(id, why) => {
                        match actions::run(self.client, repo, id).await {
                            Ok(run) if run["status"] == "completed" => {
                                Follow::Done(id, run, Some(why))
                            }
                            _ if elapsed > give_up => {
                                Follow::Failed(format!("{why}; it never completed"))
                            }
                            _ => Follow::Cancelling(id, why),
                        }
                    }
                    done @ (Follow::Done(..) | Follow::Failed(_)) => done,
                };
            }
            if pending
                .iter()
                .all(|p| matches!(p.state, Follow::Done(..) | Follow::Failed(_)))
            {
                return pending;
            }
            tokio::time::sleep(self.patience.poll).await;
        }
    }

    async fn measure(&self, cell: &Cell, repeat: u32, p: Pending, today: Option<&str>) -> ArmRun {
        let mut record = self.record(cell, repeat, p.runner);
        let (id, run) = match p.state {
            Follow::Done(id, run, note) => {
                record.note = note;
                (id, run)
            }
            Follow::Failed(why) | Follow::Cancelling(_, why) => {
                record.note = Some(why);
                return record;
            }
            Follow::Dispatched(_) => {
                record.note = Some("never completed".to_owned());
                return record;
            }
        };
        record.url = run["html_url"].as_str().map(str::to_owned);
        record.head_sha = run["head_sha"].as_str().map(str::to_owned);
        let job = match actions::job(self.client, &self.plan.repo, id).await {
            Ok(job) => job,
            Err(e) => {
                record.note = Some(format!("jobs: {e}"));
                return record;
            }
        };
        let m = timing::measure(&run, &job);
        record.outcome = m.outcome;
        record.times_s = m.times;
        let day = m.day.as_deref().or(today).unwrap_or("1970-01-01");
        if let (Some(rate), Some(secs)) = (self.pricing.rate(day, p.runner), m.job_s) {
            let (billed, usd) = rate.bill(secs);
            record.vcpus = rate.vcpus;
            record.billed_s = Some(billed);
            record.usd = Some(usd);
            record.usd_per_minute = Some(rate.usd_per_minute);
            record.pricing_as_of = self.pricing.table(day).map(|t| t.as_of.clone());
        }
        record
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use base64::Engine as _;
    use serde_json::Value;

    use super::*;
    use crate::catalog::FIXTURES_TOML;
    use crate::github::{ApiError, Limits, Response};
    use crate::pricing::PRICING_TOML;
    use crate::scenario::SCENARIOS_TOML;

    /// GitHub as a state machine: run 1 queues once then succeeds, run 2 never leaves the
    /// queue; the second runner's dispatch answers 204 (found by tag).
    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
        polls: RefCell<BTreeMap<u64, u32>>,
        cancelled: RefCell<Vec<u64>>,
    }

    fn sha(c: char) -> String {
        c.to_string().repeat(40)
    }

    impl Api for Fake {
        async fn send(
            &self,
            method: Method,
            path: &str,
            body: Option<&Value>,
        ) -> Result<Response, ApiError> {
            self.calls
                .borrow_mut()
                .push(format!("{} {path}", method.as_str()));
            let ok = |v: Value| Ok(Response::new(200, v));
            let mut r = match (method, path) {
                (Method::Get, "/repos/o/r") => ok(json!({"full_name": "o/r"})),
                (Method::Get, "/repos/o/r/git/ref/heads/main") => {
                    ok(json!({"object": {"sha": sha('m')}}))
                }
                (Method::Get, p)
                    if p.starts_with("/repos/o/r/contents/fixtures/go-small/version7.go") =>
                {
                    ok(
                        json!({"type": "file", "encoding": "base64", "content": base64::engine::general_purpose::STANDARD.encode("package uuid\n")}),
                    )
                }
                (Method::Get, p) if p.starts_with("/repos/o/r/git/commits/") => {
                    ok(json!({"tree": {"sha": sha('t')}}))
                }
                (Method::Post, "/repos/o/r/git/trees") => {
                    let content = body
                        .and_then(|b| b["tree"][0]["content"].as_str())
                        .unwrap_or("");
                    assert!(
                        content.starts_with("package uuid\n\nfunc ciBenchEdit()"),
                        "{content}"
                    );
                    ok(json!({"sha": sha('u')}))
                }
                (Method::Post, "/repos/o/r/git/commits") => ok(json!({"sha": sha('c')})),
                (Method::Post, "/repos/o/r/git/refs") => Ok(Response::new(201, json!({}))),
                (Method::Post, p) if p.ends_with("bench-ubuntu-latest.yml/dispatches") => {
                    ok(json!({"workflow_run_id": 1}))
                }
                (Method::Post, p) if p.ends_with("dispatches") => {
                    Ok(Response::new(204, Value::Null))
                }
                (Method::Get, p) if p.contains("/runs?branch=") => ok(
                    json!({"workflow_runs": [{"id": 2, "display_title": format!("go-small leaf {}", "b1-go-small-leaf-1")}]}),
                ),
                (Method::Get, "/repos/o/r/actions/runs/1") => {
                    let n = *self
                        .polls
                        .borrow_mut()
                        .entry(1)
                        .and_modify(|n| *n += 1)
                        .or_insert(1);
                    if n == 1 {
                        ok(json!({"status": "queued"}))
                    } else {
                        ok(
                            json!({"status": "completed", "conclusion": "success", "created_at": "2026-10-26T12:00:00Z",
                                  "html_url": "https://github.com/o/r/actions/runs/1", "head_sha": sha('c')}),
                        )
                    }
                }
                (Method::Get, "/repos/o/r/actions/runs/2") => {
                    if self.cancelled.borrow().contains(&2) {
                        ok(
                            json!({"status": "completed", "conclusion": "cancelled", "created_at": "2026-10-26T12:00:00Z"}),
                        )
                    } else {
                        ok(json!({"status": "queued"}))
                    }
                }
                (Method::Post, "/repos/o/r/actions/runs/2/cancel") => {
                    self.cancelled.borrow_mut().push(2);
                    Ok(Response::new(202, Value::Null))
                }
                (Method::Get, "/repos/o/r/actions/runs/1/jobs?filter=latest") => {
                    ok(json!({"jobs": [{
                    "started_at": "2026-10-26T12:00:05Z", "completed_at": "2026-10-26T12:01:10Z",
                    "steps": [{"name": "install", "started_at": "2026-10-26T12:00:20Z", "completed_at": "2026-10-26T12:00:50Z"},
                              {"name": "test", "started_at": "2026-10-26T12:00:50Z", "completed_at": "2026-10-26T12:01:05Z"}]}]}))
                }
                (Method::Get, "/repos/o/r/actions/runs/2/jobs?filter=latest") => {
                    ok(json!({"jobs": [{"started_at": null, "completed_at": null, "steps": []}]}))
                }
                (Method::Delete, p) if p.starts_with("/repos/o/r/git/refs/heads/bench/") => {
                    Ok(Response::new(204, Value::Null))
                }
                _ => panic!("unexpected {} {path}", method.as_str()),
            };
            if let Ok(resp) = &mut r {
                resp.date = Some(1_793_016_005);
            }
            r
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_cell_end_to_end() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let scenarios = Scenarios::parse(SCENARIOS_TOML, &catalog).unwrap();
        let pricing = Pricing::parse(PRICING_TOML).unwrap();
        let plan = Plan {
            repo: "o/r".into(),
            label: "smoke".into(),
            runners: vec![Runner::UbuntuLatest, Runner::Blacksmith2404Arm],
            cells: vec![Cell {
                fixture: "go-small".parse().unwrap(),
                scenario: ScenarioName::Leaf,
            }],
            repeats: 1,
            id: "b1".into(),
        };
        let patience = Patience {
            poll: Duration::from_secs(10),
            find: Duration::from_secs(30),
            queued: Duration::from_mins(1),
            run: Duration::from_mins(10),
        };
        let client = Client::new(Fake::default(), Limits::STANDARD);
        let report = Bench {
            client: &client,
            catalog: &catalog,
            scenarios: &scenarios,
            pricing: &pricing,
            plan: &plan,
            patience,
        }
        .run()
        .await;
        assert_eq!(report.date, "2026-10-26");
        assert_eq!(report.source.base_commit, sha('m'));
        let [gha, bs] = report.runs.as_slice() else {
            panic!("{:?}", report.runs)
        };
        assert_eq!(gha.outcome, Outcome::Success);
        assert_eq!(
            (
                gha.times_s.queue,
                gha.times_s.start,
                gha.times_s.verdict,
                gha.times_s.total
            ),
            (Some(5.0), Some(20.0), Some(65.0), Some(70.0))
        );
        assert_eq!(
            (gha.billed_s, gha.usd, gha.vcpus),
            (Some(120.0), Some(0.012), 4)
        );
        assert_eq!(gha.head_sha.as_deref(), Some(sha('c').as_str()));
        assert_eq!(bs.outcome, Outcome::Cancelled);
        assert!(
            bs.note
                .as_deref()
                .unwrap()
                .contains("still queued after 60 s: cancelled")
        );
        assert_eq!(bs.usd, None, "a job that never started costs nothing");
        let calls = client.api_ref().calls.borrow();
        assert!(
            calls
                .iter()
                .any(|c| c == "POST /repos/o/r/actions/runs/2/cancel")
        );
        assert_eq!(
            calls.last().unwrap(),
            "DELETE /repos/o/r/git/refs/heads/bench/b1-go-small-leaf-1"
        );
    }

    /// GitHub failing everything but the repo and main's head: python-small/leaf cannot read
    /// its file, the next cell cannot dispatch.
    struct Broken;
    impl Api for Broken {
        async fn send(
            &self,
            method: Method,
            path: &str,
            _: Option<&Value>,
        ) -> Result<Response, ApiError> {
            Ok(match (method, path) {
                (Method::Get, "/repos/o/r") => Response::new(200, json!({})),
                (Method::Get, "/repos/o/r/git/ref/heads/main") => {
                    Response::new(200, json!({"object": {"sha": "m".repeat(40)}}))
                }
                (Method::Delete, _) => Response::new(422, json!({})),
                _ => Response::new(500, Value::Null),
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn failures_are_recorded_and_the_batch_goes_on() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let scenarios = Scenarios::parse(SCENARIOS_TOML, &catalog).unwrap();
        let pricing = Pricing::parse(PRICING_TOML).unwrap();
        let plan = Plan {
            repo: "o/r".into(),
            label: "x".into(),
            runners: vec![Runner::UbuntuLatest],
            cells: vec![
                Cell {
                    fixture: "python-small".parse().unwrap(),
                    scenario: ScenarioName::Leaf,
                },
                Cell {
                    fixture: "go-small".parse().unwrap(),
                    scenario: ScenarioName::Baseline,
                },
            ],
            repeats: 1,
            id: "b2".into(),
        };
        let client = Client::new(
            Broken,
            Limits {
                attempts: 2,
                ..Limits::STANDARD
            },
        );
        let report = Bench {
            client: &client,
            catalog: &catalog,
            scenarios: &scenarios,
            pricing: &pricing,
            plan: &plan,
            patience: Patience::STANDARD,
        }
        .run()
        .await;
        assert_eq!(report.runs.len(), 2);
        assert!(
            report.runs[0]
                .note
                .as_deref()
                .unwrap()
                .starts_with("commit: ")
        );
        assert!(
            report.runs[1]
                .note
                .as_deref()
                .unwrap()
                .starts_with("dispatch: "),
            "{:?}",
            report.runs[1]
        );
        assert!(report.runs.iter().all(|r| r.outcome == Outcome::Error));
        assert_eq!(report.date, "1970-01-01", "no Date header from the fake");
    }
}
