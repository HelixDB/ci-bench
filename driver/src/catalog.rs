//! `fixtures.toml` (the fixture catalogue) and the runners the workflows target.
//!
//! A fixture is a vendored snapshot under `fixtures/<name>`; its `install` and `test` commands
//! run with bash in that directory on every runner, after the setup and cache steps its
//! [`Toolchain`] names. [`Runner`] is the closed set of competitor runners: the four workflows
//! are identical but for `runs-on`.
//!
//! An [`Adversarial`] fixture (`fixtures/adv-<language>`) is a small synthetic project of
//! ci-bench's own (MIT) whose cases probe affected-test selection (`scenarios.toml`, class
//! `adversarial`; [`crate::corpus`]). It is no benchmark: no workflow runs it, `run` refuses it,
//! and only arms that select tests run its cases.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The checked-in catalogue.
pub const FIXTURES_TOML: &str = include_str!("../../fixtures.toml");

/// Why the catalogue is unusable.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// Not TOML of this schema.
    #[error("fixtures.toml: {0}")]
    Toml(#[from] toml::de::Error),
    /// A value breaks a rule.
    #[error("fixtures.toml: {fixture}: {why}")]
    Invalid {
        /// The fixture.
        fixture: String,
        /// The rule.
        why: String,
    },
}

/// How a fixture's toolchain is set up and cached (best practice per ecosystem).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Toolchain {
    /// rustup with the pinned toolchain, `Swatinem/rust-cache` (saved from `main` only).
    Rust,
    /// `actions/setup-go` with the pinned Go, caching modules and builds keyed by `go.*`.
    Go,
    /// `pnpm/action-setup` from `packageManager`, `actions/setup-node` caching the pnpm store.
    NodePnpm,
    /// `astral-sh/setup-uv` with its cache keyed by `uv.lock`.
    PythonUv,
    /// `actions/setup-python` caching pip, keyed by `pyproject.toml`.
    PythonPip,
}

impl Toolchain {
    /// Every toolchain, in workflow step order.
    pub const ALL: [Self; 5] = [
        Self::Rust,
        Self::Go,
        Self::NodePnpm,
        Self::PythonUv,
        Self::PythonPip,
    ];

    /// The files the toolchain needs in the fixture root.
    ///
    /// ```
    /// use ci_bench::catalog::Toolchain;
    /// assert_eq!(Toolchain::Go.manifests(), ["go.mod"]);
    /// assert_eq!(Toolchain::PythonUv.manifests(), ["pyproject.toml", "uv.lock"]);
    /// ```
    #[must_use]
    pub const fn manifests(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["Cargo.toml"],
            Self::Go => &["go.mod"],
            Self::NodePnpm => &["package.json", "pnpm-lock.yaml"],
            Self::PythonUv => &["pyproject.toml", "uv.lock"],
            Self::PythonPip => &["pyproject.toml"],
        }
    }
}

/// A fixture name: `[a-z0-9-]`, 1..=40 bytes, starting with a letter.
///
/// ```
/// use ci_bench::catalog::FixtureName;
/// assert_eq!("go-small".parse::<FixtureName>().unwrap().as_str(), "go-small");
/// assert!("Go".parse::<FixtureName>().is_err());
/// assert!("../x".parse::<FixtureName>().is_err());
/// assert!("".parse::<FixtureName>().is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FixtureName(String);

impl FixtureName {
    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for FixtureName {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let valid = (1..=40).contains(&s.len())
            && s.starts_with(|c: char| c.is_ascii_lowercase())
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(format!(
                "{s:?} is not a fixture name ([a-z][a-z0-9-]{{0,39}})"
            ))
        }
    }
}

impl TryFrom<String> for FixtureName {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

impl From<FixtureName> for String {
    fn from(n: FixtureName) -> Self {
        n.0
    }
}

impl fmt::Display for FixtureName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One fixture.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    /// Its directory under `fixtures/`.
    pub name: FixtureName,
    /// Setup and caches.
    pub toolchain: Toolchain,
    /// The upstream repository (`https://github.com/<owner>/<repo>`).
    pub upstream: String,
    /// The upstream tag (or branch) the commit is on.
    pub tag: String,
    /// The pinned upstream commit (40 hex).
    pub commit: String,
    /// The upstream license (SPDX expression).
    pub license: String,
    /// Dependencies and the build.
    pub install: String,
    /// The test command.
    pub test: String,
}

/// One adversarial fixture (module docs): the commands that install it, run its whole suite,
/// and run one unit alone or every unit but one. A unit is named as the runner names it: a
/// nextest binary id (`adv-core::greeting`), a Go import path, a test file's path in the
/// fixture (Vitest, Jest, pytest). `only` and `except` read it from the environment variable
/// `UNIT`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adversarial {
    /// Its directory under `fixtures/` (`adv-…`).
    pub name: FixtureName,
    /// Its toolchain (the manifests it holds).
    pub toolchain: Toolchain,
    /// Dependencies and the build.
    pub install: String,
    /// The whole suite.
    pub test: String,
    /// Unit `$UNIT` alone.
    pub only: String,
    /// Every unit but `$UNIT`.
    pub except: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    fixture: Vec<Fixture>,
    #[serde(default)]
    adversarial: Vec<Adversarial>,
}

/// The prefix every adversarial fixture's name has, and no benchmark fixture's.
pub const ADVERSARIAL_PREFIX: &str = "adv-";

/// Licenses a fixture may have (permissive only).
pub const PERMISSIVE: [&str; 6] = [
    "MIT",
    "Apache-2.0",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "Unlicense",
    "ISC",
];

/// The catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    fixtures: Vec<Fixture>,
    adversarial: Vec<Adversarial>,
}

/// Whether `c` can be pasted into a workflow's or the corpus check's bash: one non-empty line
/// without `${{`.
fn command_ok(c: &str) -> bool {
    !c.trim().is_empty() && !c.contains('\n') && !c.contains("${{")
}

impl Catalog {
    /// Parses and checks the catalogue: unique names, a 40-hex commit, a GitHub upstream, only
    /// permissive licenses, single-line commands without `${{` (they are pasted into the
    /// workflow's bash `case`). Adversarial fixtures are named `adv-…` (and benchmark fixtures
    /// never are), their commands follow the same rule, and `only` and `except` name `$UNIT`.
    ///
    /// ```
    /// let catalog = ci_bench::catalog::Catalog::parse(ci_bench::catalog::FIXTURES_TOML).unwrap();
    /// assert_eq!(catalog.fixtures().len(), 8);
    /// assert!(catalog.fixture("go-small").is_some());
    /// assert_eq!(catalog.adversarial().len(), 5);
    /// assert!(catalog.adversarial_fixture("adv-go").is_some());
    /// assert!(catalog.fixture("adv-go").is_none(), "no benchmark");
    /// let bad = r#"[[fixture]]
    /// name = "x"
    /// toolchain = "go"
    /// upstream = "https://github.com/a/b"
    /// tag = "v1"
    /// commit = "0123"
    /// license = "MIT"
    /// install = "true"
    /// test = "true""#;
    /// assert!(ci_bench::catalog::Catalog::parse(bad).is_err(), "a short commit");
    /// ```
    ///
    /// # Errors
    /// [`CatalogError`] naming the fixture and the rule.
    pub fn parse(text: &str) -> Result<Self, CatalogError> {
        let raw: Raw = toml::from_str(text)?;
        let invalid = |f: &Fixture, why: &str| CatalogError::Invalid {
            fixture: f.name.to_string(),
            why: why.to_owned(),
        };
        for (i, f) in raw.fixture.iter().enumerate() {
            if raw.fixture[..i].iter().any(|g| g.name == f.name) {
                return Err(invalid(f, "duplicate name"));
            }
            if f.name.as_str().starts_with(ADVERSARIAL_PREFIX) {
                return Err(invalid(f, "names starting adv- are adversarial fixtures'"));
            }
            if f.commit.len() != 40 || !f.commit.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(invalid(f, "commit must be 40 hex digits"));
            }
            let repo = f.upstream.strip_prefix("https://github.com/").unwrap_or("");
            if repo.split('/').filter(|s| !s.is_empty()).count() != 2 {
                return Err(invalid(
                    f,
                    "upstream must be https://github.com/<owner>/<repo>",
                ));
            }
            let permissive = f
                .license
                .split(" OR ")
                .all(|l| PERMISSIVE.contains(&l.trim()));
            if !permissive {
                return Err(invalid(f, "the license must be permissive"));
            }
            if !command_ok(&f.install) || !command_ok(&f.test) {
                return Err(invalid(
                    f,
                    "install and test must be single, non-empty lines without ${{",
                ));
            }
        }
        for (i, a) in raw.adversarial.iter().enumerate() {
            let invalid = |why: &str| CatalogError::Invalid {
                fixture: a.name.to_string(),
                why: why.to_owned(),
            };
            if raw.adversarial[..i].iter().any(|b| b.name == a.name) {
                return Err(invalid("duplicate name"));
            }
            if !a.name.as_str().starts_with(ADVERSARIAL_PREFIX) {
                return Err(invalid("an adversarial fixture is named adv-…"));
            }
            let commands = [&a.install, &a.test, &a.only, &a.except].map(String::as_str);
            if !commands.into_iter().all(command_ok) {
                return Err(invalid(
                    "install, test, only and except must be single, non-empty lines without ${{",
                ));
            }
            if ![&a.only, &a.except]
                .into_iter()
                .all(|c| c.contains("$UNIT"))
            {
                return Err(invalid("only and except run the unit named by $UNIT"));
            }
        }
        if raw.fixture.is_empty() {
            return Err(CatalogError::Invalid {
                fixture: String::new(),
                why: "no fixtures".to_owned(),
            });
        }
        Ok(Self {
            fixtures: raw.fixture,
            adversarial: raw.adversarial,
        })
    }

    /// Every fixture, in file order.
    #[must_use]
    pub fn fixtures(&self) -> &[Fixture] {
        &self.fixtures
    }

    /// The fixture named `name`.
    #[must_use]
    pub fn fixture(&self, name: &str) -> Option<&Fixture> {
        self.fixtures.iter().find(|f| f.name.as_str() == name)
    }

    /// Every adversarial fixture, in file order.
    #[must_use]
    pub fn adversarial(&self) -> &[Adversarial] {
        &self.adversarial
    }

    /// The adversarial fixture named `name`.
    #[must_use]
    pub fn adversarial_fixture(&self, name: &str) -> Option<&Adversarial> {
        self.adversarial.iter().find(|a| a.name.as_str() == name)
    }

    /// Checks every fixture, adversarial ones included, against `dir` (the repository's
    /// `fixtures/`): its directory exists, holds its toolchain's manifests, and has no `.git` or
    /// `.github`.
    ///
    /// # Errors
    /// Every problem found, one line each.
    pub fn check_dir(&self, dir: &Path) -> Result<(), Vec<String>> {
        let all = self
            .fixtures
            .iter()
            .map(|f| (&f.name, f.toolchain))
            .chain(self.adversarial.iter().map(|a| (&a.name, a.toolchain)));
        let problems: Vec<String> = all
            .flat_map(|(name, toolchain)| {
                let root = dir.join(name.as_str());
                let missing = toolchain
                    .manifests()
                    .iter()
                    .filter(|m| !root.join(m).is_file())
                    .map(|m| format!("{name}: no {m}"));
                let stray = [".git", ".github"]
                    .into_iter()
                    .filter(|d| root.join(d).exists())
                    .map(|d| format!("{name}: has {d} (vendored snapshots carry neither)"));
                let absent = (!root.is_dir()).then(|| format!("{name}: no directory"));
                absent
                    .into_iter()
                    .chain(missing)
                    .chain(stray)
                    .collect::<Vec<_>>()
            })
            .collect();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}

/// Who runs a runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// GitHub-hosted runners.
    Github,
    /// Blacksmith (installed on the `HelixDB` organization).
    Blacksmith,
}

/// A CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    /// x86-64.
    X86_64,
    /// 64-bit ARM.
    Aarch64,
}

/// A competitor runner: one workflow each, identical but for `runs-on`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
pub enum Runner {
    /// GitHub-hosted `ubuntu-latest` (x64).
    #[serde(rename = "ubuntu-latest")]
    #[value(name = "ubuntu-latest")]
    UbuntuLatest,
    /// GitHub-hosted `ubuntu-24.04-arm`.
    #[serde(rename = "ubuntu-24.04-arm")]
    #[value(name = "ubuntu-24.04-arm")]
    Ubuntu2404Arm,
    /// Blacksmith `blacksmith-2vcpu-ubuntu-2404` (x64).
    #[serde(rename = "blacksmith-2vcpu-ubuntu-2404")]
    #[value(name = "blacksmith-2vcpu-ubuntu-2404")]
    Blacksmith2404,
    /// Blacksmith `blacksmith-2vcpu-ubuntu-2404-arm`.
    #[serde(rename = "blacksmith-2vcpu-ubuntu-2404-arm")]
    #[value(name = "blacksmith-2vcpu-ubuntu-2404-arm")]
    Blacksmith2404Arm,
}

impl Runner {
    /// Every runner, in report order.
    pub const ALL: [Self; 4] = [
        Self::UbuntuLatest,
        Self::Ubuntu2404Arm,
        Self::Blacksmith2404,
        Self::Blacksmith2404Arm,
    ];

    /// The `runs-on` label (Blacksmith's per docs.blacksmith.sh, "Runner Tags").
    ///
    /// ```
    /// use ci_bench::catalog::Runner;
    /// assert_eq!(Runner::Blacksmith2404Arm.label(), "blacksmith-2vcpu-ubuntu-2404-arm");
    /// ```
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UbuntuLatest => "ubuntu-latest",
            Self::Ubuntu2404Arm => "ubuntu-24.04-arm",
            Self::Blacksmith2404 => "blacksmith-2vcpu-ubuntu-2404",
            Self::Blacksmith2404Arm => "blacksmith-2vcpu-ubuntu-2404-arm",
        }
    }

    /// Its workflow file under `.github/workflows/`.
    ///
    /// ```
    /// use ci_bench::catalog::Runner;
    /// assert_eq!(Runner::Ubuntu2404Arm.workflow(), "bench-ubuntu-24.04-arm.yml");
    /// ```
    #[must_use]
    pub fn workflow(self) -> String {
        format!("bench-{}.yml", self.label())
    }

    /// The provider.
    #[must_use]
    pub const fn provider(self) -> Provider {
        match self {
            Self::UbuntuLatest | Self::Ubuntu2404Arm => Provider::Github,
            Self::Blacksmith2404 | Self::Blacksmith2404Arm => Provider::Blacksmith,
        }
    }

    /// The architecture.
    #[must_use]
    pub const fn arch(self) -> Arch {
        match self {
            Self::UbuntuLatest | Self::Blacksmith2404 => Arch::X86_64,
            Self::Ubuntu2404Arm | Self::Blacksmith2404Arm => Arch::Aarch64,
        }
    }

    /// The runner of `label`.
    ///
    /// ```
    /// use ci_bench::catalog::Runner;
    /// assert_eq!(Runner::of_label("ubuntu-latest"), Some(Runner::UbuntuLatest));
    /// assert_eq!(Runner::of_label("macos-latest"), None);
    /// ```
    #[must_use]
    pub fn of_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.label() == label)
    }
}

impl fmt::Display for Runner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    #[test]
    fn the_checked_in_catalog_matches_the_vendored_fixtures() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        catalog.check_dir(&repo_root().join("fixtures")).unwrap();
        let toolchains: std::collections::BTreeSet<Toolchain> =
            catalog.fixtures().iter().map(|f| f.toolchain).collect();
        assert_eq!(
            toolchains.len(),
            Toolchain::ALL.len(),
            "every toolchain is used"
        );
        for prefix in ["rust-", "go-", "node-", "python-"] {
            for size in ["small", "large"] {
                assert!(
                    catalog.fixture(&format!("{prefix}{size}")).is_some(),
                    "one small and one large per language"
                );
            }
        }
    }

    #[test]
    fn rules() {
        let one = |name: &str, commit: &str, license: &str, upstream: &str, test: &str| {
            format!(
                "[[fixture]]\nname = \"{name}\"\ntoolchain = \"go\"\nupstream = \"{upstream}\"\ntag = \"v1\"\ncommit = \"{commit}\"\nlicense = \"{license}\"\ninstall = \"true\"\ntest = \"{test}\"\n"
            )
        };
        let sha = "0".repeat(40);
        let good = one("a", &sha, "MIT", "https://github.com/o/r", "go test");
        assert!(Catalog::parse(&good).is_ok());
        assert!(
            Catalog::parse(&format!("{good}{good}")).is_err(),
            "duplicate"
        );
        assert!(Catalog::parse(&one("a", &sha, "GPL-3.0", "https://github.com/o/r", "x")).is_err());
        assert!(
            Catalog::parse(&one(
                "a",
                &sha,
                "MIT OR GPL-3.0",
                "https://github.com/o/r",
                "x"
            ))
            .is_err()
        );
        assert!(Catalog::parse(&one("a", &sha, "MIT", "https://gitlab.com/o/r", "x")).is_err());
        assert!(Catalog::parse(&one("a", &sha, "MIT", "https://github.com/o", "x")).is_err());
        assert!(
            Catalog::parse(&one(
                "a",
                &sha,
                "MIT",
                "https://github.com/o/r",
                "${{ github.token }}"
            ))
            .is_err()
        );
        assert!(
            Catalog::parse(&one(
                "a",
                &"g".repeat(40),
                "MIT",
                "https://github.com/o/r",
                "x"
            ))
            .is_err()
        );
        assert!(Catalog::parse(&one("A", &sha, "MIT", "https://github.com/o/r", "x")).is_err());
        assert!(Catalog::parse("fixture = []").is_err(), "no fixtures");
        assert!(
            Catalog::parse(&format!("{good}extra = 1\n")).is_err(),
            "unknown keys"
        );
    }

    #[test]
    fn adversarial_rules() {
        let sha = "0".repeat(40);
        let bench = |name: &str| {
            format!(
                "[[fixture]]\nname = \"{name}\"\ntoolchain = \"go\"\nupstream = \"https://github.com/o/r\"\ntag = \"v1\"\ncommit = \"{sha}\"\nlicense = \"MIT\"\ninstall = \"true\"\ntest = \"go test\"\n"
            )
        };
        let adv = |name: &str, only: &str, except: &str| {
            format!(
                "[[adversarial]]\nname = \"{name}\"\ntoolchain = \"go\"\ninstall = \"true\"\ntest = \"go test ./...\"\nonly = '{only}'\nexcept = '{except}'\n"
            )
        };
        let only = r#"go test "$UNIT""#;
        let except = r#"go test $(go list ./... | grep -vxF "$UNIT")"#;
        let good = format!("{}{}", bench("a"), adv("adv-a", only, except));
        let catalog = Catalog::parse(&good).unwrap();
        assert_eq!(catalog.fixtures().len(), 1);
        assert_eq!(catalog.adversarial()[0].only, only);
        assert!(catalog.fixture("adv-a").is_none());
        assert!(catalog.adversarial_fixture("a").is_none());
        let why = |text: &str| match Catalog::parse(text) {
            Err(CatalogError::Invalid { why, .. }) => why,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            why(&format!("{good}{}", adv("adv-a", only, except))),
            "duplicate name"
        );
        assert!(
            why(&format!("{}{}", bench("adv-b"), adv("adv-a", only, except)))
                .contains("adversarial")
        );
        assert!(why(&format!("{}{}", bench("a"), adv("b", only, except))).contains("named adv-"));
        assert!(
            why(&format!(
                "{}{}",
                bench("a"),
                adv("adv-a", "go test ./...", except)
            ))
            .contains("$UNIT")
        );
        assert!(why(&format!("{}{}", bench("a"), adv("adv-a", only, "go test"))).contains("$UNIT"));
        assert!(
            why(&format!("{}{}", bench("a"), adv("adv-a", only, "  ")))
                .contains("single, non-empty")
        );
        assert!(
            why(&format!(
                "{}{}",
                bench("a"),
                adv("adv-a", "${{ x }} $UNIT", except)
            ))
            .contains("single, non-empty")
        );
        assert!(
            Catalog::parse(&format!("{good}extra = 1\n")).is_err(),
            "unknown keys"
        );
    }

    #[test]
    fn check_dir_reports_every_problem() {
        let tmp = tempfile::tempdir().unwrap();
        let sha = "0".repeat(40);
        let text = format!(
            "[[fixture]]\nname = \"a\"\ntoolchain = \"python_uv\"\nupstream = \"https://github.com/o/r\"\ntag = \"v1\"\ncommit = \"{sha}\"\nlicense = \"MIT\"\ninstall = \"x\"\ntest = \"y\"\n[[fixture]]\nname = \"b\"\ntoolchain = \"go\"\nupstream = \"https://github.com/o/r\"\ntag = \"v1\"\ncommit = \"{sha}\"\nlicense = \"MIT\"\ninstall = \"x\"\ntest = \"y\"\n"
        );
        let catalog = Catalog::parse(&text).unwrap();
        std::fs::create_dir_all(tmp.path().join("a/.github")).unwrap();
        std::fs::write(tmp.path().join("a/pyproject.toml"), "").unwrap();
        let problems = catalog.check_dir(tmp.path()).unwrap_err();
        assert_eq!(
            problems,
            [
                "a: no uv.lock",
                "a: has .github (vendored snapshots carry neither)",
                "b: no directory",
                "b: no go.mod",
            ]
        );
    }

    #[test]
    fn runners() {
        for r in Runner::ALL {
            assert_eq!(Runner::of_label(r.label()), Some(r));
            let json = serde_json::to_string(&r).unwrap();
            assert_eq!(json, format!("\"{}\"", r.label()));
            assert_eq!(serde_json::from_str::<Runner>(&json).unwrap(), r);
            assert_eq!(r.to_string(), r.label());
        }
        assert_eq!(Runner::UbuntuLatest.provider(), Provider::Github);
        assert_eq!(Runner::Blacksmith2404.arch(), Arch::X86_64);
        assert_eq!(Runner::Blacksmith2404Arm.arch(), Arch::Aarch64);
        assert_eq!(Runner::UbuntuLatest.workflow(), "bench-ubuntu-latest.yml");
    }
}
