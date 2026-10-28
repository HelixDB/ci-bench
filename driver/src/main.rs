//! `ci-bench`: see the library docs and the repository's README.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use ci_bench::catalog::{Catalog, Runner};
use ci_bench::scenario::{ScenarioName, Scenarios};
use ci_bench::{bench, corpus, github, pricing, scenario, workflow};
use clap::Parser;

/// Head-to-head CI benchmark driver.
#[derive(Debug, Parser)]
#[command(name = "ci-bench")]
struct Args {
    /// The repository checkout (holds fixtures.toml).
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Check the catalogue, scenarios, prices and workflows against the checkout.
    Validate,
    /// Compare .github/workflows with their rendering; --write rewrites them.
    Workflows {
        /// Rewrite stale or missing workflows.
        #[arg(long)]
        write: bool,
    },
    /// Apply one scenario to a copy of its fixture (for arms that run elsewhere).
    Apply {
        /// The fixture.
        #[arg(long)]
        fixture: String,
        /// The scenario: a benchmark class, or an adversarial fixture's case.
        #[arg(long)]
        scenario: String,
        /// The copy of fixtures/<fixture> to edit.
        #[arg(long)]
        dir: PathBuf,
    },
    /// Check every adversarial case on local toolchains (cargo-nextest, Go, pnpm, uv): a
    /// breaking case breaks exactly its unit, the others stay green.
    Corpus {
        /// Adversarial fixtures (repeat; default every one).
        #[arg(long = "fixture")]
        fixtures: Vec<String>,
        /// Cases (repeat; default every case of the fixtures).
        #[arg(long = "case")]
        cases: Vec<String>,
        /// Seconds one command may take.
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// Run a batch on GitHub and write its ci-bench/1 result file.
    Run(Box<RunArgs>),
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    /// The benchmark repository.
    #[arg(long, default_value = "HelixDB/ci-bench")]
    repo: String,
    /// Runners (repeat; default every runner).
    #[arg(long = "runner", value_enum)]
    runners: Vec<Runner>,
    /// Fixtures (repeat; default every fixture).
    #[arg(long = "fixture")]
    fixtures: Vec<String>,
    /// Scenarios (repeat; default every scripted scenario).
    #[arg(long = "scenario", value_enum)]
    scenarios: Vec<ScenarioName>,
    /// Run each fixture's baseline on main first (warms the caches branches restore).
    #[arg(long)]
    warm: bool,
    /// Repeats of every cell.
    #[arg(long, default_value_t = 1)]
    repeats: u32,
    /// The batch's label.
    #[arg(long, default_value = "full")]
    label: String,
    /// The result file to write (never overwritten).
    #[arg(long)]
    out: PathBuf,
    /// Seconds a run may stay queued before it is cancelled.
    #[arg(long, default_value_t = bench::Patience::STANDARD.queued.as_secs())]
    queue_timeout: u64,
    /// Seconds a run may take before it is cancelled.
    #[arg(long, default_value_t = bench::Patience::STANDARD.run.as_secs())]
    run_timeout: u64,
    /// Seconds between polls of a run (raise it when several batches share one token's rate
    /// limit).
    #[arg(long, default_value_t = bench::Patience::STANDARD.poll.as_secs())]
    poll: u64,
    /// Print the cells and exit; contacts nothing.
    #[arg(long)]
    dry_run: bool,
}

fn load(root: &std::path::Path) -> anyhow::Result<(Catalog, Scenarios, pricing::Pricing)> {
    let read = |name: &str| {
        std::fs::read_to_string(root.join(name)).with_context(|| format!("reading {name}"))
    };
    let catalog = Catalog::parse(&read("fixtures.toml")?)?;
    let scenarios = Scenarios::parse(&read("scenarios.toml")?, &catalog)?;
    let pricing = pricing::Pricing::parse(&read("pricing.toml")?)?;
    Ok((catalog, scenarios, pricing))
}

fn validate(root: &std::path::Path) -> anyhow::Result<ExitCode> {
    let (catalog, scenarios, _) = load(root)?;
    let mut problems: Vec<String> = catalog
        .check_dir(&root.join("fixtures"))
        .err()
        .unwrap_or_default();
    for f in catalog.fixtures() {
        let dir = root.join("fixtures").join(f.name.as_str());
        for name in ScenarioName::SCRIPTED {
            let edits = scenarios.edits(f.name.as_str(), name).unwrap_or_default();
            if let Err(e) = scenario::plan(edits, |p| scenario::read_in(&dir, p)) {
                problems.push(format!("{}/{name}: {e}", f.name));
            }
        }
    }
    let mut cases = 0;
    for a in catalog.adversarial() {
        let dir = root.join("fixtures").join(a.name.as_str());
        for (name, case) in scenarios.cases(a.name.as_str()) {
            cases += 1;
            let Err(e) = scenario::plan(&case.edits, |p| scenario::read_in(&dir, p)) else {
                continue;
            };
            problems.push(format!("{}/{name}: {e}", a.name));
        }
    }
    let drift = workflow::sync(root, &catalog, false)?;
    problems.extend(
        drift
            .iter()
            .map(|d| format!("workflow {d:?}: run `ci-bench workflows --write`")),
    );
    problems.iter().for_each(|p| eprintln!("ci-bench: {p}"));
    println!(
        "{} fixtures, {} scenarios each; {} adversarial fixtures, {cases} cases; {} problems",
        catalog.fixtures().len(),
        ScenarioName::SCRIPTED.len(),
        catalog.adversarial().len(),
        problems.len()
    );
    Ok(if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn run(root: &std::path::Path, args: RunArgs) -> anyhow::Result<ExitCode> {
    let (catalog, scenarios, pricing) = load(root)?;
    let runners = if args.runners.is_empty() {
        Runner::ALL.to_vec()
    } else {
        args.runners
    };
    let fixtures: Vec<_> = if args.fixtures.is_empty() {
        catalog.fixtures().iter().map(|f| f.name.clone()).collect()
    } else {
        args.fixtures
            .iter()
            .map(|f| {
                anyhow::ensure!(
                    catalog.adversarial_fixture(f).is_none(),
                    "{f} is an adversarial fixture: only arms that select tests run its cases"
                );
                catalog
                    .fixture(f)
                    .map(|f| f.name.clone())
                    .with_context(|| format!("no fixture {f}"))
            })
            .collect::<anyhow::Result<_>>()?
    };
    let names = if args.scenarios.is_empty() {
        ScenarioName::SCRIPTED.to_vec()
    } else {
        args.scenarios
    };
    let warm = fixtures.iter().filter(|_| args.warm).map(|f| bench::Cell {
        fixture: f.clone(),
        scenario: ScenarioName::Baseline,
    });
    let cells: Vec<bench::Cell> = warm
        .chain(fixtures.iter().flat_map(|f| {
            names.iter().map(|s| bench::Cell {
                fixture: f.clone(),
                scenario: *s,
            })
        }))
        .collect();
    let id = uuid::Uuid::now_v7().simple().to_string()[..12].to_owned();
    let plan = bench::Plan {
        repo: args.repo,
        label: args.label,
        runners,
        cells,
        repeats: args.repeats,
        id,
    };
    if args.dry_run {
        println!(
            "dry run: nothing is contacted\nrepo {}  batch {}  repeats {}",
            plan.repo, plan.id, plan.repeats
        );
        for c in &plan.cells {
            let runners: Vec<&str> = plan.runners.iter().map(|r| r.label()).collect();
            println!("{}/{} on {}", c.fixture, c.scenario, runners.join(", "));
        }
        return Ok(ExitCode::SUCCESS);
    }
    anyhow::ensure!(
        !args.out.exists(),
        "{} exists (results are never overwritten)",
        args.out.display()
    );
    let token = github::Https::token_from_gh()
        .await
        .map_err(anyhow::Error::msg)?;
    let client = github::Client::new(
        github::Https::new(token).map_err(anyhow::Error::msg)?,
        github::Limits::STANDARD,
    );
    let patience = bench::Patience {
        queued: std::time::Duration::from_secs(args.queue_timeout),
        run: std::time::Duration::from_secs(args.run_timeout),
        poll: std::time::Duration::from_secs(args.poll.max(1)),
        ..bench::Patience::STANDARD
    };
    let result = bench::Bench {
        client: &client,
        catalog: &catalog,
        scenarios: &scenarios,
        pricing: &pricing,
        plan: &plan,
        patience,
    }
    .run()
    .await;
    result.write_new(&args.out)?;
    print!("{}", result.table());
    eprintln!("ci-bench: wrote {}", args.out.display());
    let clean = result
        .runs
        .iter()
        .all(|r| r.outcome == ci_bench::timing::Outcome::Success);
    Ok(if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn check_corpus(
    root: &std::path::Path,
    fixtures: &[String],
    cases: &[String],
    timeout: std::time::Duration,
) -> anyhow::Result<ExitCode> {
    let (catalog, scenarios, _) = load(root)?;
    let chosen: Vec<_> = if fixtures.is_empty() {
        catalog.adversarial().iter().collect()
    } else {
        fixtures
            .iter()
            .map(|f| {
                catalog
                    .adversarial_fixture(f)
                    .with_context(|| format!("no adversarial fixture {f}"))
            })
            .collect::<anyhow::Result<_>>()?
    };
    let unknown: Vec<&String> = cases
        .iter()
        .filter(|c| {
            !chosen
                .iter()
                .any(|f| scenarios.case(f.name.as_str(), c).is_some())
        })
        .collect();
    anyhow::ensure!(unknown.is_empty(), "no such cases: {unknown:?}");
    let scratch =
        std::env::temp_dir().join(format!("ci-bench-corpus-{}", uuid::Uuid::now_v7().simple()));
    std::fs::create_dir(&scratch).with_context(|| format!("creating {}", scratch.display()))?;
    let mut broken = 0;
    let mut checked = 0;
    for fixture in chosen {
        let name = fixture.name.as_str();
        let picked: Vec<_> = scenarios
            .cases(name)
            .filter(|(c, _)| cases.is_empty() || cases.iter().any(|w| w == c.as_str()))
            .collect();
        let outcomes = corpus::Check {
            fixture,
            source: &root.join("fixtures").join(name),
            scratch: &scratch,
            timeout,
        }
        .run(&picked)
        .await;
        for o in outcomes {
            let at = o
                .case
                .as_ref()
                .map_or_else(|| name.to_owned(), |c| format!("{name}/{c}"));
            checked += usize::from(o.case.is_some());
            match o.result {
                Ok(what) => println!("ok      {at}: {what}"),
                Err(why) => {
                    broken += 1;
                    println!("BROKEN  {at}: {why}");
                }
            }
        }
    }
    std::fs::remove_dir_all(&scratch).with_context(|| format!("removing {}", scratch.display()))?;
    println!("{checked} cases checked, {broken} broken");
    Ok(if broken == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn dispatch(args: Args) -> anyhow::Result<ExitCode> {
    match args.command {
        Command::Validate => validate(&args.root),
        Command::Workflows { write } => {
            let (catalog, ..) = load(&args.root)?;
            let drift = workflow::sync(&args.root, &catalog, write)?;
            drift.iter().for_each(|d| println!("{d:?}"));
            Ok(if drift.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Apply {
            fixture,
            scenario: name,
            dir,
        } => {
            let (_, scenarios, _) = load(&args.root)?;
            let edits = scenarios
                .edits_named(&fixture, &name)
                .with_context(|| format!("no scenario {name} of fixture {fixture}"))?;
            let changes = scenario::apply_dir(&dir, edits)?;
            changes.iter().for_each(|c| println!("{}", c.path()));
            Ok(ExitCode::SUCCESS)
        }
        Command::Corpus {
            fixtures,
            cases,
            timeout,
        } => {
            let timeout = std::time::Duration::from_secs(timeout);
            check_corpus(&args.root, &fixtures, &cases, timeout).await
        }
        Command::Run(a) => run(&args.root, *a).await,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match dispatch(Args::parse()).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ci-bench: {e:#}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    #[test]
    fn cli_is_well_formed() {
        super::Args::command().debug_assert();
    }
}
