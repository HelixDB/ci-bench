//! ci-bench: a head-to-head CI benchmark on identical commits.
//!
//! The repository holds pinned open-source fixtures (`fixtures.toml`), scripted agent-diff
//! scenarios (`scenarios.toml`), dated runner prices (`pricing.toml`) and four workflows that
//! differ only in `runs-on`. This crate renders the workflows, commits each scenario on a
//! throwaway branch, dispatches every runner, and turns GitHub's own timestamps into a
//! `ci-bench/1` result file.
//!
//! | Module | Contract |
//! |---|---|
//! | [`catalog`] | the fixtures and the runners |
//! | [`scenario`] | the scenarios, their edits and how they apply (locally or through the API) |
//! | [`workflow`] | the workflows, rendered from the catalogue and checked for drift |
//! | [`pricing`] | dated per-minute list prices and billing rounding |
//! | [`github`] | REST calls with rate-limit waits and retries |
//! | [`actions`] | branches, dispatches, runs, jobs, cancellation |
//! | [`timing`] | queue, start, step, verdict and total times from GitHub's timestamps |
//! | [`bench`] | one batch: commit, dispatch, poll, measure, price, clean up |
//! | [`report`] | the `ci-bench/1` result file |

pub mod actions;
pub mod bench;
pub mod catalog;
pub mod github;
pub mod pricing;
pub mod report;
pub mod scenario;
pub mod timing;
pub mod workflow;
