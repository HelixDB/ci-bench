# ci-bench

A head-to-head CI benchmark on identical commits: the same open-source fixture projects, the
same scripted "agent diffs", and identical workflows that differ only in the runner they ask for.
Timings come from GitHub's own timestamps; costs from each provider's published per-minute list
prices on the day of the run.

## What is in here

| Path | What |
|---|---|
| `fixtures/<name>` | pinned snapshots of permissively licensed open-source projects (no `.git` or `.github`; each keeps its upstream license) |
| `fixtures.toml` | each fixture's upstream, commit, license, toolchain, and its `install` and `test` commands |
| `scenarios.toml` | the scripted diffs per fixture: `leaf`, `mid_graph`, `trigger`, `test_only`, `docs_only`, `add`, `delete`, `rename` (`baseline` is the fixture as vendored; `cold` is `baseline` with every cache step off, input `cache: false`) |
| `pricing.toml` | dated per-minute list prices of every runner, with sources |
| `.github/workflows/bench-<runner>.yml` | one `workflow_dispatch` workflow per runner, rendered from `fixtures.toml` and identical but for `runs-on` |
| `driver/` | the `ci-bench` binary (Rust) |

### Fixtures

| Name | Upstream | Commit | License | Toolchain and caching |
|---|---|---|---|---|
| `rust-small` | [dtolnay/semver](https://github.com/dtolnay/semver) 1.0.28 | `7625c7a` | MIT OR Apache-2.0 | rustup 1.97.1, cargo-nextest 0.9.149 (`cargo nextest run` then `cargo test --doc`), `Swatinem/rust-cache` (saved from `main`) |
| `rust-large` | [BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep) 15.2.0 | `e89fff8` | Unlicense OR MIT | same; `.config/nextest.toml` (ours, not upstream) runs the tests that share a temporary directory one at a time |
| `go-small` | [google/uuid](https://github.com/google/uuid) v1.6.0 | `0f11ee6` | BSD-3-Clause | Go 1.27.3, `actions/setup-go` module and build cache |
| `go-large` | [gin-gonic/gin](https://github.com/gin-gonic/gin) v1.12.0 | `73726dc` | MIT | same |
| `node-small` | [unjs/defu](https://github.com/unjs/defu) v6.1.7 | `80c0146` | MIT | `pnpm/action-setup`, `actions/setup-node` pnpm store cache; Vitest |
| `node-large` | [vueuse/vueuse](https://github.com/vueuse/vueuse) v15.0.0 (pnpm workspace) | `3cfd9f2` | MIT | same |
| `python-small` | [pallets/itsdangerous](https://github.com/pallets/itsdangerous) main | `672971d` | BSD-3-Clause | `astral-sh/setup-uv` cache (uv.lock); pytest |
| `python-large` | [django/django](https://github.com/django/django) 5.2.18 | `3ae618c` | BSD-3-Clause | `actions/setup-python` pip cache; Django's own runner |

### Runners

| Workflow | `runs-on` | vCPU here | List price / min |
|---|---|---|---|
| `bench-ubuntu-latest.yml` | `ubuntu-latest` | 4 (public repository) | $0.006 (2-core, private repositories) |
| `bench-ubuntu-24.04-arm.yml` | `ubuntu-24.04-arm` | 4 (public repository) | $0.005 |
| `bench-blacksmith-2vcpu-ubuntu-2404.yml` | `blacksmith-2vcpu-ubuntu-2404` | 2 | $0.004 |
| `bench-blacksmith-2vcpu-ubuntu-2404-arm.yml` | `blacksmith-2vcpu-ubuntu-2404-arm` | 2 | $0.0025 |

GitHub gives public repositories 4-vCPU standard runners and no 2-vCPU label, so the GitHub
arms have twice the vCPUs of the 2-vCPU Blacksmith and Crucible arms (their wall times favour
GitHub; their costs are priced at the 2-core rate). Blacksmith's caching is its documented
setup: the upstream cache and setup actions, which its runners redirect to its colocated cache.

Prices are list prices for a paying repository, the lowest each label sells at (GitHub-hosted
runs of this public repository are free). Blacksmith's own Go build cache replaces the
`actions/setup-go` cache when it is enabled on the organization; the workflows stay identical.

## Measures

Per run, from the Actions runs and jobs APIs (GitHub's clock only):
`queue` (run created → job started), `setup`, `checkout`, `toolchain` (setup and cache restore),
`install`, `test`, `post` (cache saves), `start` (run created → first project command),
`verdict` (run created → tests finished) and `total`; the job's billed time (rounded up to whole
minutes) and its list-price cost.

## Running it

Needs the [gh](https://cli.github.com) CLI signed in with access to the repository (the driver
uses `gh auth token`, or `GH_TOKEN` when set; no other credentials).

```sh
cargo run --release -p ci-bench -- validate                 # fixtures, scenarios, prices, workflows
cargo run --release -p ci-bench -- workflows --write        # after editing fixtures.toml
cargo run --release -p ci-bench -- run --label smoke \
  --fixture go-small --scenario leaf --out smoke.json      # every runner
cargo run --release -p ci-bench -- run --warm --repeats 3 --out full.json
cargo run --release -p ci-bench -- apply --fixture go-small --scenario leaf --dir /tmp/copy
```

`run` commits each scenario on a throwaway branch `bench/<batch>-<fixture>-<scenario>-<n>`
(through the Git Data API, no clone), dispatches every runner's workflow on it (the dispatch
answer names the run: `return_run_details`; one that does not is found by its tag within 2
minutes), polls until each run completes (a run queued over 15 minutes or unfinished after 70 is
cancelled and recorded as such), measures and prices it, deletes the branch, and writes one
`ci-bench/1` JSON file (`driver/tests/sample-report.json` shows the format). `--warm` first runs
each fixture's `baseline` on `main`, whose runs save the caches the branches restore. Rate
limits are waited out (GitHub's `Date` and reset headers), server errors retried.

Other systems can be measured on the same fixtures and diffs: `apply` edits a copy of a fixture
exactly as the branches are edited, and a result file may hold any arm.

## License

The driver, workflows and data files are MIT (`LICENSE`). Each fixture keeps its upstream
license, in its directory.
