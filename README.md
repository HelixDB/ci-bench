# ci-bench

A head-to-head CI benchmark on identical commits: the same open-source fixture projects, the
same scripted "agent diffs", and identical workflows that differ only in the runner they ask for.
Timings come from GitHub's own timestamps; costs from each provider's published per-minute list
prices on the day of the run.

## What is in here

| Path | What |
|---|---|
| `fixtures/<name>` | pinned snapshots of permissively licensed open-source projects (no `.git` or `.github`; each keeps its upstream license) |
| `fixtures/adv-<language>` | the [adversarial corpus](#adversarial-corpus): small synthetic projects of ci-bench's own (MIT) |
| `fixtures.toml` | each fixture's upstream, commit, license, toolchain, and its `install` and `test` commands; each adversarial fixture's commands |
| `scenarios.toml` | the scripted diffs per fixture: `leaf`, `mid_graph`, `trigger`, `test_only`, `docs_only`, `add`, `delete`, `rename` (`baseline` is the fixture as vendored; `cold` is `baseline` with every cache step off, input `cache: false`); the adversarial cases |
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

## Adversarial corpus

Arms that skip tests a change cannot affect (affected-test selection) are fast only if they are
sound: a skipped test that the change breaks is a wrong green. The corpus is a set of agent diffs
built to catch that. Each fixture is a small synthetic project (`fixtures/adv-rust`, `adv-go`,
`adv-node` for Vitest, `adv-jest`, `adv-python` for pytest), and each case
(`class = "adversarial"` in `scenarios.toml`) says what a selecting arm must do with it:

| `expect` | The selecting arm must |
|---|---|
| `fail`, `breaks = "<unit>"` | fail the job, `<unit>` among the failures (the change breaks that unit and no other) |
| `fail` without `breaks` | fail the job (the change breaks the build) |
| `fail`, `limit = true` | nothing: a known limit (a test runs another package's binary, which no static graph sees), reported only |
| `pass` | pass, running fewer units than the suite has (`leaf_pass`, in every fixture: an arm whose selection is always off fails it) |
| `full`, `why = "<why>"` | run every unit, for `<why>`: `global` (a lockfile, a manifest, toolchain or runner configuration, a file the runner's configuration loads); the suite stays green |

A unit is named as the fixture's runner names it: a nextest binary id (`adv-core::greeting`), a
Go import path, a test file's path in the fixture. A fail case's `why` (`unmapped`,
`shadowing`) is the reason a selection is expected to run everything there; it is reported, not
required. Every case that breaks a unit also makes the fixture's harmless leaf edit, so a
selection that loses track of the break still selects something rather than falling back to
everything.

| Fixture | Cases |
|---|---|
| `adv-rust` | an `include_str!` input; another package's test data read at run time; `util.rs` beside `util/mod.rs` (the build breaks); `math.rs` moved to `math/mod.rs` and changed; `Cargo.lock` (full); a build script's input outside every package; `.cargo/config.toml` (full); another package's binary run by a test (limit) |
| `adv-go` | a file added under a `//go:embed` pattern; another package's `testdata` read at run time; `go.mod`'s language version (loop variables); `go run ./cmd/tool` from a test (limit) |
| `adv-node` | a vite alias; a tsconfig path; a workspace package; JSON; a Vue single-file component; a template `import()`; a module only `setupFiles` imports; a fixture file read at run time; `util.ts` beside `util/index.ts`; `money.ts` moved to `money/index.ts` and changed; a `#internal` subpath; a `new Worker(new URL(…))`; a `vi.mock` target; an auto-import plugin added through a file the config imports (full); a `.d.ts` (full); type tests turned on (full); `pnpm-lock.yaml` (full); another package's bin run by a test (limit) |
| `adv-jest` | a `moduleNameMapper` path; a `__mocks__` file added; a module only `setupFilesAfterEnv` requires |
| `adv-python` | a `conftest.py` fixture; a package `__init__.py`'s side effect; an `importlib` import by name; an undotted `pytest_plugins` entry; `tests/helpers.py` imported through the rootdir; a data file; `ext.py` beside the namespace package `ext/`; `python -m adv.cli` from a test; `money.py` moved to `money/__init__.py` and changed; `uv.lock` (full); a pytest-django setting (full) |

Only arms that select tests run the corpus: no workflow includes the adversarial fixtures, and
`run` refuses them. `ci-bench corpus` checks on local toolchains (cargo-nextest, Go, pnpm, uv)
that every case holds what it claims: each fixture installs and passes as vendored; a case with
`breaks` fails `test` and the fixture's `only` command for that unit, and passes its `except`
command (every unit but that one); a build-breaking case fails `install` or `test`; `pass` and
`full` cases pass.

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
cargo run --release -p ci-bench -- apply --fixture adv-go --scenario go_mod --dir /tmp/copy
cargo run --release -p ci-bench -- corpus --fixture adv-go   # the adversarial cases, locally
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

The driver, workflows, data files and adversarial fixtures are MIT (`LICENSE`). Each benchmark
fixture keeps its upstream license, in its directory.
