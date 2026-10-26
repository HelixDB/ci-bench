//! The four benchmark workflows, rendered from the catalogue: identical but for `runs-on`
//! (and the name that says which runner it is).
//!
//! Each is `workflow_dispatch` only, with inputs `fixture` (a choice of the catalogue), `scenario`
//! (the class the branch's commit applies, a label) and `tag` (the driver's id for finding the
//! run). One job: a sparse checkout of the fixture, the toolchain setup and cache steps of its
//! [`Toolchain`] (each step guarded by the fixtures that use it), then `install` and `test`
//! from the catalogue in a bash `case` on `$FIXTURE` (inputs reach scripts through `env`, never
//! through `${{ }}` in the script). Step names are what [`crate::timing`] groups by: `checkout`,
//! `toolchain: …`, `install`, `test`. Third-party actions are pinned to commit SHAs.

use std::fmt::Write as _;
use std::path::Path;

use crate::catalog::{Catalog, Fixture, Runner, Toolchain};

/// The Rust toolchain the Rust fixtures build with.
pub const RUST_TOOLCHAIN: &str = "1.97.1";
/// Go for both Go fixtures (one exact release: `go.mod` need not name one, and runners
/// preinstall different versions or none).
pub const GO_VERSION: &str = "1.27.3";
/// Node.js for the pnpm fixtures.
pub const NODE_VERSION: &str = "24";
/// Python for both Python fixtures.
pub const PYTHON_VERSION: &str = "3.13";
/// A job is cancelled by GitHub after this long.
pub const JOB_TIMEOUT_MINUTES: u32 = 60;

/// An action pinned to a commit (`uses: <repo>@<sha> # <tag>`).
struct Pin {
    repo: &'static str,
    sha: &'static str,
    tag: &'static str,
}

impl Pin {
    fn uses(&self) -> String {
        format!("{}@{} # {}", self.repo, self.sha, self.tag)
    }
}

const CHECKOUT: Pin = Pin {
    repo: "actions/checkout",
    sha: "3d3c42e5aac5ba805825da76410c181273ba90b1",
    tag: "v7.0.1",
};
const RUST_CACHE: Pin = Pin {
    repo: "Swatinem/rust-cache",
    sha: "d1527c8f67ef9af357290475d4af4b0fab8f12e0",
    tag: "v2.9.3",
};
const SETUP_GO: Pin = Pin {
    repo: "actions/setup-go",
    sha: "b7ad1dad31e06c5925ef5d2fc7ad053ef454303e",
    tag: "v7.0.0",
};
const SETUP_PNPM: Pin = Pin {
    repo: "pnpm/action-setup",
    sha: "ea17c68df8912ef543352723c149a84f56e3d413",
    tag: "v6.1.0",
};
const SETUP_NODE: Pin = Pin {
    repo: "actions/setup-node",
    sha: "949feb2413d6458794dcd2491c4babbbce0c15c1",
    tag: "v7.1.0",
};
const SETUP_UV: Pin = Pin {
    repo: "astral-sh/setup-uv",
    sha: "07a047e608db84b3d8c06cd9a7c7dcc9c355c18e",
    tag: "v10.3.2",
};
const SETUP_PYTHON: Pin = Pin {
    repo: "actions/setup-python",
    sha: "6d4fd22b20465ccc0f7354476588e8df7c9ca0e0",
    tag: "v7.1.0",
};

/// `inputs.fixture == 'a' || inputs.fixture == 'b'` for the fixtures of `toolchain`.
fn guard(fixtures: &[&Fixture]) -> String {
    fixtures
        .iter()
        .map(|f| format!("inputs.fixture == '{}'", f.name))
        .collect::<Vec<_>>()
        .join(" || ")
}

/// The setup and cache steps of `toolchain` (`dir` is the fixture directory expression).
fn toolchain_steps(toolchain: Toolchain, if_: &str) -> String {
    let dir = "fixtures/${{ inputs.fixture }}";
    let step = |name: &str, body: &str| {
        format!("      - name: \"toolchain: {name}\"\n        if: {if_}\n{body}")
    };
    match toolchain {
        Toolchain::Rust => [
            step(
                "rust",
                &format!(
                    "        run: rustup toolchain install {RUST_TOOLCHAIN} --profile minimal --no-self-update && rustup default {RUST_TOOLCHAIN}\n"
                ),
            ),
            step(
                "rust cache",
                &format!(
                    "        uses: {}\n        with:\n          workspaces: {dir}\n          shared-key: ${{{{ inputs.fixture }}}}\n          save-if: ${{{{ github.ref == 'refs/heads/main' }}}}\n",
                    RUST_CACHE.uses()
                ),
            ),
        ]
        .concat(),
        Toolchain::Go => step(
            "go",
            &format!(
                "        uses: {}\n        with:\n          go-version: \"{GO_VERSION}\"\n          cache-dependency-path: {dir}/go.*\n",
                SETUP_GO.uses()
            ),
        ),
        Toolchain::NodePnpm => [
            step(
                "pnpm",
                &format!(
                    "        uses: {}\n        with:\n          package_json_file: {dir}/package.json\n",
                    SETUP_PNPM.uses()
                ),
            ),
            step(
                "node",
                &format!(
                    "        uses: {}\n        with:\n          node-version: \"{NODE_VERSION}\"\n          cache: pnpm\n          cache-dependency-path: {dir}/pnpm-lock.yaml\n",
                    SETUP_NODE.uses()
                ),
            ),
        ]
        .concat(),
        Toolchain::PythonUv => step(
            "uv",
            &format!(
                "        uses: {}\n        with:\n          python-version: \"{PYTHON_VERSION}\"\n          enable-cache: true\n          cache-dependency-glob: {dir}/uv.lock\n",
                SETUP_UV.uses()
            ),
        ),
        Toolchain::PythonPip => step(
            "python",
            &format!(
                "        uses: {}\n        with:\n          python-version: \"{PYTHON_VERSION}\"\n          cache: pip\n          cache-dependency-path: {dir}/pyproject.toml\n",
                SETUP_PYTHON.uses()
            ),
        ),
    }
}

/// A step running one catalogue command per fixture.
fn command_step(name: &str, catalog: &Catalog, command: fn(&Fixture) -> &str) -> String {
    let mut out = format!(
        "      - name: {name}\n        env:\n          FIXTURE: ${{{{ inputs.fixture }}}}\n        run: |\n          case \"$FIXTURE\" in\n"
    );
    for f in catalog.fixtures() {
        let _ = writeln!(out, "            {}) {} ;;", f.name, command(f));
    }
    out.push_str(
        "            *) echo \"unknown fixture $FIXTURE\" >&2; exit 2 ;;\n          esac\n",
    );
    out
}

/// The workflow for `runner`.
///
/// ```
/// use ci_bench::catalog::{Catalog, Runner, FIXTURES_TOML};
/// let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
/// let yaml = ci_bench::workflow::render(&catalog, Runner::Blacksmith2404);
/// assert!(yaml.contains("    runs-on: blacksmith-2vcpu-ubuntu-2404\n"));
/// assert!(yaml.contains("          - go-small\n"));
/// ```
#[must_use]
pub fn render(catalog: &Catalog, runner: Runner) -> String {
    let options = catalog.fixtures().iter().fold(String::new(), |mut out, f| {
        let _ = writeln!(out, "          - {}", f.name);
        out
    });
    let first = catalog.fixtures().first().map_or("", |f| f.name.as_str());
    let toolchains: String = Toolchain::ALL
        .into_iter()
        .filter_map(|t| {
            let users: Vec<&Fixture> = catalog
                .fixtures()
                .iter()
                .filter(|f| f.toolchain == t)
                .collect();
            (!users.is_empty()).then(|| toolchain_steps(t, &guard(&users)))
        })
        .collect();
    format!(
        "# Generated by `cargo run -p ci-bench -- workflows --write` from fixtures.toml: do not edit.
# The four bench-*.yml workflows are identical but for the runner (`runs-on` and the name).
name: \"bench: {label}\"
run-name: \"${{{{ inputs.fixture }}}} ${{{{ inputs.scenario }}}} ${{{{ inputs.tag }}}}\"

on:
  workflow_dispatch:
    inputs:
      fixture:
        description: The fixture under fixtures/
        required: true
        type: choice
        default: {first}
        options:
{options}      scenario:
        description: The scenario the branch's commit applies (scenarios.toml), a label
        required: true
        type: string
        default: baseline
      tag:
        description: The driver's id for this run
        required: true
        type: string
        default: manual

permissions:
  contents: read

jobs:
  bench:
    name: bench
    runs-on: {label}
    timeout-minutes: {JOB_TIMEOUT_MINUTES}
    defaults:
      run:
        shell: bash
        working-directory: fixtures/${{{{ inputs.fixture }}}}
    steps:
      - name: checkout
        uses: {checkout}
        with:
          sparse-checkout: fixtures/${{{{ inputs.fixture }}}}
          persist-credentials: false
{toolchains}{install}{test}",
        label = runner.label(),
        checkout = CHECKOUT.uses(),
        install = command_step("install", catalog, |f| &f.install),
        test = command_step("test", catalog, |f| &f.test),
    )
}

/// What `workflows --check` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// The file is missing.
    Missing(String),
    /// The file differs from its rendering.
    Stale(String),
}

/// Compares `.github/workflows` under `root` with the renderings; with `write`, rewrites the
/// stale and missing ones first (so the answer is then empty).
///
/// # Errors
/// IO errors reading or writing.
pub fn sync(root: &Path, catalog: &Catalog, write: bool) -> std::io::Result<Vec<Drift>> {
    let dir = root.join(".github/workflows");
    if write {
        std::fs::create_dir_all(&dir)?;
    }
    let mut drift = Vec::new();
    for runner in Runner::ALL {
        let path = dir.join(runner.workflow());
        let want = render(catalog, runner);
        let have = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        if have.as_deref() == Some(want.as_str()) {
            continue;
        }
        if write {
            std::fs::write(&path, &want)?;
            continue;
        }
        drift.push(if have.is_some() {
            Drift::Stale(runner.workflow())
        } else {
            Drift::Missing(runner.workflow())
        });
    }
    Ok(drift)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::FIXTURES_TOML;

    #[test]
    fn checked_in_workflows_are_current() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        assert_eq!(
            sync(&root, &catalog, false).unwrap(),
            [],
            "run `cargo run -p ci-bench -- workflows --write`"
        );
    }

    #[test]
    fn workflows_differ_only_in_the_runner() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let renders: Vec<String> = Runner::ALL.map(|r| render(&catalog, r)).to_vec();
        let neutral = |r: Runner, text: &str| text.replace(r.label(), "RUNNER");
        let first = neutral(Runner::ALL[0], &renders[0]);
        for (r, text) in Runner::ALL.into_iter().zip(&renders) {
            assert_eq!(neutral(r, text), first, "{r}");
            let differing: Vec<&str> = text
                .lines()
                .zip(renders[0].lines())
                .filter(|(a, b)| a != b)
                .map(|(a, _)| a)
                .collect();
            let allowed = [format!("name: \"bench: {r}\""), format!("    runs-on: {r}")];
            assert!(
                differing.iter().all(|l| allowed.iter().any(|a| a == l)),
                "{r}: {differing:?}"
            );
        }
    }

    #[test]
    fn structure() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let yaml = render(&catalog, Runner::UbuntuLatest);
        // Inputs never reach a script through ${{ }}.
        let in_scripts = yaml.split("run: |").skip(1).any(|script| {
            script
                .split("\n      - ")
                .next()
                .unwrap_or("")
                .contains("${{")
        });
        assert!(!in_scripts);
        for step in ["- name: checkout", "- name: install", "- name: test"] {
            assert_eq!(yaml.matches(step).count(), 1, "{step}");
        }
        assert!(
            yaml.contains("if: inputs.fixture == 'rust-small' || inputs.fixture == 'rust-large'")
        );
        assert!(yaml.contains("            python-large) python -m pip install -e . tblib ;;"));
        assert!(yaml.contains("save-if: ${{ github.ref == 'refs/heads/main' }}"));
        for pin in [
            CHECKOUT,
            RUST_CACHE,
            SETUP_GO,
            SETUP_PNPM,
            SETUP_NODE,
            SETUP_UV,
            SETUP_PYTHON,
        ] {
            assert_eq!(pin.sha.len(), 40, "{} pinned to a commit", pin.repo);
            assert!(yaml.contains(&pin.uses()), "{}", pin.repo);
        }
        assert!(yaml.contains(&format!("go-version: \"{GO_VERSION}\"")));
        assert!(!yaml.contains("go-version-file"), "go.mod need not name a version");
        assert!(yaml.ends_with("esac\n"));
    }

    #[test]
    fn sync_writes_and_detects() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let missing = sync(tmp.path(), &catalog, false).unwrap();
        assert_eq!(missing.len(), 4);
        assert!(matches!(missing[0], Drift::Missing(_)));
        assert_eq!(sync(tmp.path(), &catalog, true).unwrap(), []);
        assert_eq!(sync(tmp.path(), &catalog, false).unwrap(), []);
        let path = tmp.path().join(".github/workflows/bench-ubuntu-latest.yml");
        std::fs::write(&path, "x").unwrap();
        assert_eq!(
            sync(tmp.path(), &catalog, false).unwrap(),
            [Drift::Stale("bench-ubuntu-latest.yml".into())]
        );
    }
}
