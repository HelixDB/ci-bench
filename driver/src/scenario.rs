//! `scenarios.toml`: scripted agent diffs and how they apply.
//!
//! A scenario is a list of [`Edit`]s on a pristine fixture: a benchmark class
//! ([`ScenarioName`]) of every benchmark fixture, or an adversarial [`Case`] of an adversarial
//! fixture (`class = "adversarial"`), which says what an arm that selects tests must do with it
//! ([`Expect`]). [`plan`] turns edits into
//! [`Change`]s (a path's new content, or its removal) from a reader of the current files, so
//! the same function serves a local checkout ([`apply_dir`]) and the GitHub Git Data API (the
//! driver commits the changes on a branch without a clone). Every precondition is checked
//! before anything changes: an `append`, `delete`, `rename` or `replace` target must exist, a
//! `create` or rename target must not, a `replace` must find its text exactly once, and paths
//! are relative, `/`-separated and never leave the fixture.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, FixtureName};

/// The checked-in scenarios.
pub const SCENARIOS_TOML: &str = include_str!("../../scenarios.toml");

/// A scenario class.
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
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum ScenarioName {
    /// The fixture as vendored (no edits; what warms caches on `main`).
    Baseline,
    /// The fixture as vendored on `main` with every cache step off (`cache: false`): a cold
    /// first run.
    Cold,
    /// Code few modules depend on.
    Leaf,
    /// Code many modules depend on.
    MidGraph,
    /// A manifest or tool config file.
    Trigger,
    /// An existing test file.
    TestOnly,
    /// Documentation only.
    DocsOnly,
    /// A new test file.
    Add,
    /// A test or example file removed.
    Delete,
    /// A test file renamed.
    Rename,
}

impl ScenarioName {
    /// Every scripted class (not `baseline`), in report order.
    pub const SCRIPTED: [Self; 8] = [
        Self::Leaf,
        Self::MidGraph,
        Self::Trigger,
        Self::TestOnly,
        Self::DocsOnly,
        Self::Add,
        Self::Delete,
        Self::Rename,
    ];

    /// Whether the scenario runs the fixture as vendored, on `main` (`baseline`, `cold`).
    ///
    /// ```
    /// use ci_bench::scenario::ScenarioName;
    /// assert!(ScenarioName::Cold.unedited() && ScenarioName::Baseline.unedited());
    /// assert!(!ScenarioName::Leaf.unedited());
    /// ```
    #[must_use]
    pub const fn unedited(self) -> bool {
        matches!(self, Self::Baseline | Self::Cold)
    }

    /// The name as written in files and workflow inputs.
    ///
    /// ```
    /// use ci_bench::scenario::ScenarioName;
    /// assert_eq!(ScenarioName::MidGraph.as_str(), "mid_graph");
    /// assert_eq!("docs_only".parse::<ScenarioName>(), Ok(ScenarioName::DocsOnly));
    /// assert!("docs-only".parse::<ScenarioName>().is_err());
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Cold => "cold",
            Self::Leaf => "leaf",
            Self::MidGraph => "mid_graph",
            Self::Trigger => "trigger",
            Self::TestOnly => "test_only",
            Self::DocsOnly => "docs_only",
            Self::Add => "add",
            Self::Delete => "delete",
            Self::Rename => "rename",
        }
    }
}

impl FromStr for ScenarioName {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        [Self::Baseline, Self::Cold]
            .into_iter()
            .chain(Self::SCRIPTED)
            .find(|n| n.as_str() == s)
            .ok_or_else(|| format!("{s:?} is not a scenario"))
    }
}

impl fmt::Display for ScenarioName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A path inside a fixture: relative, `/`-separated, no empty, `.` or `..` component.
///
/// ```
/// use ci_bench::scenario::RelPath;
/// assert!("src/lib.rs".parse::<RelPath>().is_ok());
/// for bad in ["", "/etc/passwd", "a/../b", "./a", "a//b", "a\\b", "a/"] {
///     assert!(bad.parse::<RelPath>().is_err(), "{bad}");
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

impl RelPath {
    /// The path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for RelPath {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let valid = !s.is_empty()
            && !s.contains('\\')
            && s.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(format!("{s:?} is not a relative path inside the fixture"))
        }
    }
}

impl TryFrom<String> for RelPath {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

impl From<RelPath> for String {
    fn from(p: RelPath) -> Self {
        p.0
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An adversarial case's name: `[a-z][a-z0-9_]`, 1..=40 bytes, and never a benchmark class's
/// name (so `apply --scenario` names either unambiguously).
///
/// ```
/// use ci_bench::scenario::CaseName;
/// assert_eq!("include_str".parse::<CaseName>().unwrap().as_str(), "include_str");
/// for bad in ["", "Leaf", "leaf", "a-b", "_a", "1a", &"a".repeat(41)] {
///     assert!(bad.parse::<CaseName>().is_err(), "{bad}");
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaseName(String);

impl CaseName {
    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CaseName {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let valid = (1..=40).contains(&s.len())
            && s.starts_with(|c: char| c.is_ascii_lowercase())
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            && s.parse::<ScenarioName>().is_err();
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(format!(
                "{s:?} is not a case name ([a-z][a-z0-9_]{{0,39}}, not a benchmark class)"
            ))
        }
    }
}

impl fmt::Display for CaseName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A test unit as the adversarial fixture's runner names it
/// ([`crate::catalog::Adversarial`]): 1..=200 bytes, none of them whitespace, a control
/// character, a quote, `$` or a backtick.
///
/// ```
/// use ci_bench::scenario::Unit;
/// for good in ["adv-core::greeting", "example.com/adv/render", "test/alias.test.ts"] {
///     assert!(good.parse::<Unit>().is_ok(), "{good}");
/// }
/// for bad in ["", "a b", "a\tb", "$(x)", "a'b", "a`b"] {
///     assert!(bad.parse::<Unit>().is_err(), "{bad}");
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct Unit(String);

impl Unit {
    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Unit {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let valid = (1..=200).contains(&s.len())
            && !s.chars().any(|c| {
                c.is_whitespace() || c.is_control() || matches!(c, '\'' | '"' | '$' | '`')
            });
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(format!("{s:?} is not a unit name"))
        }
    }
}

impl TryFrom<String> for Unit {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a selection runs every unit for a change. The names are those of Crucible's
/// `proto::skip::Why` (snake case), the selecting arm the corpus was written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// A changed path is an input of every unit: a lockfile, a manifest, toolchain or runner
    /// configuration, a file the runner's configuration loads.
    Global,
    /// A changed path is in no unit's graph: test data, a build script's input.
    Unmapped,
    /// An added path may shadow a module (`a.rs` beside `a/mod.rs`, `a.ts` beside
    /// `a/index.ts`, `a.py` beside the package `a/`).
    Shadowing,
}

impl Why {
    /// The name as written in `scenarios.toml`.
    ///
    /// ```
    /// assert_eq!(ci_bench::scenario::Why::Unmapped.as_str(), "unmapped");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Unmapped => "unmapped",
            Self::Shadowing => "shadowing",
        }
    }
}

/// What an arm that selects tests must do with an adversarial case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// The job fails (`expect = "fail"`): a selection that skips `breaks` is a wrong green.
    Fail {
        /// The one unit the change breaks; `None` when it breaks the build instead.
        breaks: Option<Unit>,
        /// Why a selection is expected to run everything here: reported, not required (a
        /// selection precise enough to run `breaks` is sound too).
        why: Option<Why>,
    },
    /// A known limit (`expect = "fail"`, `limit = true`): the change breaks `breaks` through a
    /// dependency no static graph sees (one package running another's binary), so a selecting
    /// arm may pass it. Reported, never counted as a wrong green.
    Limit {
        /// The unit the change breaks.
        breaks: Unit,
    },
    /// The job passes having run fewer units than the suite has (`expect = "pass"`), so an arm
    /// whose selection is always off cannot pass it.
    Pass,
    /// The selection runs every unit for `why` (`expect = "full"`); the suite stays green.
    Full {
        /// The reason the arm must give.
        why: Why,
    },
}

/// One adversarial case of a fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// What the agent changes (a case that breaks a unit also makes a harmless edit to the
    /// fixture's leaf, so a selection that loses track of the break still selects something
    /// and cannot fall back to running everything).
    pub edits: Vec<Edit>,
    /// What a selecting arm must do with it.
    pub expect: Expect,
}

/// The case every adversarial fixture has: a harmless edit of its leaf, expecting `pass`.
pub const LEAF_PASS: &str = "leaf_pass";

/// One edit, its snippets resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// `text` added at the end of an existing file.
    Append {
        /// The file.
        path: RelPath,
        /// What is added.
        text: String,
    },
    /// A new file.
    Create {
        /// The file.
        path: RelPath,
        /// Its content.
        text: String,
    },
    /// A file removed.
    Delete {
        /// The file.
        path: RelPath,
    },
    /// A file moved, content unchanged.
    Rename {
        /// From.
        from: RelPath,
        /// To.
        to: RelPath,
    },
    /// The one occurrence of `find` replaced by `text`.
    Replace {
        /// The file.
        path: RelPath,
        /// The text that occurs exactly once.
        find: String,
        /// Its replacement.
        text: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEdit {
    append: Option<RelPath>,
    create: Option<RelPath>,
    delete: Option<RelPath>,
    rename: Option<RelPath>,
    replace: Option<RelPath>,
    to: Option<RelPath>,
    find: Option<String>,
    snippet: Option<String>,
}

/// Which arms run a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawClass {
    /// A benchmark class of a benchmark fixture: every arm runs it.
    #[default]
    Benchmark,
    /// A case of an adversarial fixture: arms that select tests run it.
    Adversarial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawExpect {
    Fail,
    Pass,
    Full,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScenario {
    fixture: FixtureName,
    name: String,
    #[serde(default)]
    class: RawClass,
    expect: Option<RawExpect>,
    breaks: Option<Unit>,
    why: Option<Why>,
    limit: Option<bool>,
    edits: Vec<RawEdit>,
}

impl RawScenario {
    /// Whether it sets a field only adversarial cases have.
    const fn has_case_fields(&self) -> bool {
        self.expect.is_some() || self.breaks.is_some() || self.why.is_some() || self.limit.is_some()
    }

    /// The case's expectation from `expect`, `breaks`, `why` and `limit`.
    fn expect(&self) -> Result<Expect, &'static str> {
        match (
            self.expect,
            &self.breaks,
            self.why,
            self.limit.unwrap_or(false),
        ) {
            (Some(RawExpect::Fail), breaks, why, false) => Ok(Expect::Fail {
                breaks: breaks.clone(),
                why,
            }),
            (Some(RawExpect::Fail), Some(breaks), None, true) => Ok(Expect::Limit {
                breaks: breaks.clone(),
            }),
            (Some(RawExpect::Pass), None, None, false) => Ok(Expect::Pass),
            (Some(RawExpect::Full), None, Some(why), false) => Ok(Expect::Full { why }),
            _ => Err(
                "a case expects fail (breaks and why optional; with limit = true, breaks and nothing else), pass (nothing else) or full (with why, no breaks)",
            ),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    snippets: BTreeMap<String, String>,
    scenario: Vec<RawScenario>,
}

/// Why `scenarios.toml` is unusable.
#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    /// Not TOML of this schema.
    #[error("scenarios.toml: {0}")]
    Toml(#[from] toml::de::Error),
    /// A rule is broken.
    #[error("scenarios.toml: {0}")]
    Invalid(String),
}

/// Every fixture's scenarios.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenarios {
    edits: BTreeMap<(FixtureName, ScenarioName), Vec<Edit>>,
    /// In file order.
    cases: Vec<(FixtureName, CaseName, Case)>,
}

fn edit(raw: RawEdit, snippets: &BTreeMap<String, String>) -> Result<Edit, String> {
    let text = |name: Option<String>| {
        let name = name.ok_or("needs a snippet")?;
        snippets
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("no snippet {name:?}"))
    };
    match raw {
        RawEdit {
            append: Some(path),
            create: None,
            delete: None,
            rename: None,
            replace: None,
            to: None,
            find: None,
            snippet,
        } => Ok(Edit::Append {
            path,
            text: text(snippet)?,
        }),
        RawEdit {
            append: None,
            create: Some(path),
            delete: None,
            rename: None,
            replace: None,
            to: None,
            find: None,
            snippet,
        } => Ok(Edit::Create {
            path,
            text: text(snippet)?,
        }),
        RawEdit {
            append: None,
            create: None,
            delete: Some(path),
            rename: None,
            replace: None,
            to: None,
            find: None,
            snippet: None,
        } => Ok(Edit::Delete { path }),
        RawEdit {
            append: None,
            create: None,
            delete: None,
            rename: Some(from),
            replace: None,
            to: Some(to),
            find: None,
            snippet: None,
        } => Ok(Edit::Rename { from, to }),
        RawEdit {
            append: None,
            create: None,
            delete: None,
            rename: None,
            replace: Some(path),
            to: None,
            find: Some(find),
            snippet,
        } if !find.is_empty() => Ok(Edit::Replace {
            path,
            find,
            text: text(snippet)?,
        }),
        _ => Err("an edit is exactly one of append, create, delete, rename (with to) or replace (with find), and only append, create and replace take a snippet".to_owned()),
    }
}

impl Scenarios {
    /// Parses and checks the file against `catalog`: every scenario names a fixture of the
    /// catalogue and is unique, every benchmark fixture has every scripted class, `baseline` is
    /// never scripted, every edit is well formed and its snippet exists. A case
    /// (`class = "adversarial"`) names an adversarial fixture and a [`CaseName`], has edits and
    /// one [`Expect`]; every adversarial fixture has the case [`LEAF_PASS`], expecting `pass`.
    /// Only cases have `expect`, `breaks`, `why` and `limit`.
    ///
    /// ```
    /// use ci_bench::{catalog, scenario};
    /// let catalog = catalog::Catalog::parse(catalog::FIXTURES_TOML).unwrap();
    /// let all = scenario::Scenarios::parse(scenario::SCENARIOS_TOML, &catalog).unwrap();
    /// assert_eq!(all.edits("go-small", scenario::ScenarioName::Baseline), Some(&[][..]));
    /// assert_eq!(all.edits("go-small", scenario::ScenarioName::Leaf).unwrap().len(), 1);
    /// assert_eq!(all.edits("nope", scenario::ScenarioName::Leaf), None);
    /// assert_eq!(all.case("adv-go", "leaf_pass").unwrap().expect, scenario::Expect::Pass);
    /// assert_eq!(all.edits("adv-go", scenario::ScenarioName::Leaf), None, "no benchmark classes");
    /// assert!(all.cases("go-small").next().is_none(), "no cases");
    /// ```
    ///
    /// # Errors
    /// [`ScenarioError`] naming the rule.
    pub fn parse(text: &str, catalog: &Catalog) -> Result<Self, ScenarioError> {
        let raw: RawFile = toml::from_str(text)?;
        let mut edits = BTreeMap::new();
        let mut cases: Vec<(FixtureName, CaseName, Case)> = Vec::new();
        for s in raw.scenario {
            let at = format!("{}/{}", s.fixture, s.name);
            let invalid = |why: &str| ScenarioError::Invalid(format!("{at}: {why}"));
            let resolve = |list: Vec<RawEdit>| {
                list.into_iter()
                    .map(|e| edit(e, &raw.snippets))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|why| invalid(&why))
            };
            match s.class {
                RawClass::Benchmark => {
                    if catalog.fixture(s.fixture.as_str()).is_none() {
                        return Err(invalid(
                            if catalog.adversarial_fixture(s.fixture.as_str()).is_some() {
                                "an adversarial fixture's scenarios are class = \"adversarial\""
                            } else {
                                "no such fixture"
                            },
                        ));
                    }
                    if s.has_case_fields() {
                        return Err(invalid(
                            "expect, breaks, why and limit are adversarial cases' fields",
                        ));
                    }
                    let name: ScenarioName = s.name.parse().map_err(|why: String| invalid(&why))?;
                    if name.unedited() || s.edits.is_empty() {
                        return Err(invalid(
                            "baseline and cold are implicit and a scripted scenario has edits",
                        ));
                    }
                    let list = resolve(s.edits)?;
                    if edits.insert((s.fixture, name), list).is_some() {
                        return Err(invalid("defined twice"));
                    }
                }
                RawClass::Adversarial => {
                    if catalog.adversarial_fixture(s.fixture.as_str()).is_none() {
                        return Err(invalid("no such adversarial fixture"));
                    }
                    let name: CaseName = s.name.parse().map_err(|why: String| invalid(&why))?;
                    if s.edits.is_empty() {
                        return Err(invalid("a case has edits"));
                    }
                    let expect = s.expect().map_err(invalid)?;
                    if cases.iter().any(|(f, n, _)| *f == s.fixture && *n == name) {
                        return Err(invalid("defined twice"));
                    }
                    let list = resolve(s.edits)?;
                    cases.push((
                        s.fixture,
                        name,
                        Case {
                            edits: list,
                            expect,
                        },
                    ));
                }
            }
        }
        let missing: Vec<String> = catalog
            .fixtures()
            .iter()
            .flat_map(|f| ScenarioName::SCRIPTED.map(|n| (f.name.clone(), n)))
            .filter(|key| !edits.contains_key(key))
            .map(|(f, n)| format!("{f}/{n}"))
            .chain(
                catalog
                    .adversarial()
                    .iter()
                    .filter(|a| {
                        !cases.iter().any(|(f, n, c)| {
                            *f == a.name && n.as_str() == LEAF_PASS && c.expect == Expect::Pass
                        })
                    })
                    .map(|a| format!("{}/{LEAF_PASS} (expecting pass)", a.name)),
            )
            .collect();
        if !missing.is_empty() {
            return Err(ScenarioError::Invalid(format!(
                "missing scenarios: {}",
                missing.join(", ")
            )));
        }
        Ok(Self { edits, cases })
    }

    /// The edits of `scenario` on `fixture` (empty for `baseline` and `cold`), or `None` for an
    /// unknown fixture (adversarial fixtures have no benchmark classes).
    #[must_use]
    pub fn edits(&self, fixture: &str, scenario: ScenarioName) -> Option<&[Edit]> {
        if scenario.unedited() {
            return self
                .edits
                .keys()
                .any(|(f, _)| f.as_str() == fixture)
                .then_some(&[][..]);
        }
        self.edits
            .iter()
            .find(|((f, n), _)| f.as_str() == fixture && *n == scenario)
            .map(|(_, e)| e.as_slice())
    }

    /// The cases of `fixture`, in file order (none for a benchmark fixture).
    pub fn cases(&self, fixture: &str) -> impl Iterator<Item = (&CaseName, &Case)> {
        self.cases
            .iter()
            .filter(move |(f, ..)| f.as_str() == fixture)
            .map(|(_, n, c)| (n, c))
    }

    /// The case `name` of `fixture`.
    #[must_use]
    pub fn case(&self, fixture: &str, name: &str) -> Option<&Case> {
        self.cases(fixture)
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, c)| c)
    }

    /// The edits of the scenario `name` of `fixture`, a benchmark class's name or a case's
    /// (what `apply --scenario` takes).
    ///
    /// ```
    /// use ci_bench::{catalog, scenario};
    /// let catalog = catalog::Catalog::parse(catalog::FIXTURES_TOML).unwrap();
    /// let all = scenario::Scenarios::parse(scenario::SCENARIOS_TOML, &catalog).unwrap();
    /// assert!(all.edits_named("go-small", "leaf").is_some());
    /// assert!(all.edits_named("adv-go", "leaf_pass").is_some());
    /// assert!(all.edits_named("adv-go", "leaf").is_none());
    /// assert!(all.edits_named("go-small", "leaf_pass").is_none());
    /// ```
    #[must_use]
    pub fn edits_named(&self, fixture: &str, name: &str) -> Option<&[Edit]> {
        match name.parse::<ScenarioName>() {
            Ok(class) => self.edits(fixture, class),
            Err(_) => self.case(fixture, name).map(|c| c.edits.as_slice()),
        }
    }
}

impl Edit {
    /// The paths the edit reads or writes.
    ///
    /// ```
    /// use ci_bench::scenario::Edit;
    /// let e = Edit::Rename { from: "a".parse().unwrap(), to: "b".parse().unwrap() };
    /// assert_eq!(e.paths().map(|p| p.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    /// ```
    pub fn paths(&self) -> impl Iterator<Item = &RelPath> {
        let (first, second) = match self {
            Self::Append { path, .. }
            | Self::Create { path, .. }
            | Self::Delete { path }
            | Self::Replace { path, .. } => (path, None),
            Self::Rename { from, to } => (from, Some(to)),
        };
        std::iter::once(first).chain(second)
    }
}

/// One path's new state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The file now holds these bytes.
    Write {
        /// The file.
        path: RelPath,
        /// Its content.
        content: Vec<u8>,
    },
    /// The file is gone.
    Remove {
        /// The file.
        path: RelPath,
    },
}

impl Change {
    /// The path changed.
    #[must_use]
    pub const fn path(&self) -> &RelPath {
        match self {
            Self::Write { path, .. } | Self::Remove { path } => path,
        }
    }
}

/// Why edits do not apply.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    /// A file that must exist does not.
    #[error("{0}: no such file")]
    Missing(String),
    /// A file that must not exist does.
    #[error("{0}: already exists")]
    Exists(String),
    /// `find` does not occur exactly once.
    #[error("{path}: the text to replace occurs {count} times, not once")]
    Ambiguous {
        /// The file.
        path: String,
        /// How often it occurs.
        count: usize,
    },
    /// Reading failed.
    #[error("{path}: {why}")]
    Read {
        /// The file.
        path: String,
        /// Why.
        why: String,
    },
}

/// The changes `edits` make, given `read` (a path's current bytes, `None` when absent). Later
/// edits see earlier ones; a path changed twice gets one change with its final state.
///
/// ```
/// use ci_bench::scenario::{plan, Change, Edit};
/// let edits = [Edit::Append { path: "a.txt".parse().unwrap(), text: "!".into() },
///              Edit::Rename { from: "a.txt".parse().unwrap(), to: "b.txt".parse().unwrap() }];
/// let changes = plan(&edits, |p| Ok((p == "a.txt").then(|| b"hi".to_vec()))).unwrap();
/// assert_eq!(changes, [
///     Change::Remove { path: "a.txt".parse().unwrap() },
///     Change::Write { path: "b.txt".parse().unwrap(), content: b"hi!".to_vec() },
/// ]);
/// ```
///
/// # Errors
/// [`ApplyError`] for the first edit that does not apply (nothing is written by `plan`).
pub fn plan(
    edits: &[Edit],
    mut read: impl FnMut(&str) -> Result<Option<Vec<u8>>, String>,
) -> Result<Vec<Change>, ApplyError> {
    let mut state: BTreeMap<RelPath, Option<Vec<u8>>> = BTreeMap::new();
    let mut current = |path: &RelPath,
                       state: &BTreeMap<RelPath, Option<Vec<u8>>>|
     -> Result<Option<Vec<u8>>, ApplyError> {
        match state.get(path) {
            Some(known) => Ok(known.clone()),
            None => read(path.as_str()).map_err(|why| ApplyError::Read {
                path: path.to_string(),
                why,
            }),
        }
    };
    for edit in edits {
        match edit {
            Edit::Append { path, text } => {
                let mut bytes =
                    current(path, &state)?.ok_or_else(|| ApplyError::Missing(path.to_string()))?;
                bytes.extend_from_slice(text.as_bytes());
                state.insert(path.clone(), Some(bytes));
            }
            Edit::Create { path, text } => {
                if current(path, &state)?.is_some() {
                    return Err(ApplyError::Exists(path.to_string()));
                }
                state.insert(path.clone(), Some(text.as_bytes().to_vec()));
            }
            Edit::Delete { path } => {
                current(path, &state)?.ok_or_else(|| ApplyError::Missing(path.to_string()))?;
                state.insert(path.clone(), None);
            }
            Edit::Rename { from, to } => {
                let bytes =
                    current(from, &state)?.ok_or_else(|| ApplyError::Missing(from.to_string()))?;
                if current(to, &state)?.is_some() {
                    return Err(ApplyError::Exists(to.to_string()));
                }
                state.insert(from.clone(), None);
                state.insert(to.clone(), Some(bytes));
            }
            Edit::Replace { path, find, text } => {
                let bytes =
                    current(path, &state)?.ok_or_else(|| ApplyError::Missing(path.to_string()))?;
                let content = String::from_utf8(bytes).map_err(|_| ApplyError::Read {
                    path: path.to_string(),
                    why: "not UTF-8".to_owned(),
                })?;
                let count = content.matches(find.as_str()).count();
                if count != 1 {
                    return Err(ApplyError::Ambiguous {
                        path: path.to_string(),
                        count,
                    });
                }
                state.insert(
                    path.clone(),
                    Some(content.replacen(find.as_str(), text, 1).into_bytes()),
                );
            }
        }
    }
    Ok(state
        .into_iter()
        .map(|(path, content)| match content {
            Some(content) => Change::Write { path, content },
            None => Change::Remove { path },
        })
        .collect())
}

/// Reads `dir/path` for [`plan`]: its bytes, `None` if it does not exist.
///
/// # Errors
/// Any other IO error, as text.
pub fn read_in(dir: &Path, path: &str) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(dir.join(path)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Applies `edits` to the fixture checkout `dir`: all preconditions first, then the writes
/// (parent directories created, emptied ones left in place).
///
/// # Errors
/// [`ApplyError`]; nothing is written when a precondition fails.
pub fn apply_dir(dir: &Path, edits: &[Edit]) -> Result<Vec<Change>, ApplyError> {
    let changes = plan(edits, |p| read_in(dir, p))?;
    for change in &changes {
        let target = dir.join(change.path().as_str());
        let io = |e: std::io::Error| ApplyError::Read {
            path: change.path().to_string(),
            why: e.to_string(),
        };
        match change {
            Change::Write { content, .. } => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                std::fs::write(&target, content).map_err(io)?;
            }
            Change::Remove { .. } => std::fs::remove_file(&target).map_err(io)?,
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::FIXTURES_TOML;

    fn p(s: &str) -> RelPath {
        s.parse().unwrap()
    }

    fn fixtures_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
    }

    /// Edited files stay regular, non-executable files (the API path writes 100644).
    fn assert_regular(dir: &Path, at: &str, changes: &[Change]) {
        for c in changes {
            let Ok(meta) = std::fs::symlink_metadata(dir.join(c.path().as_str())) else {
                continue;
            };
            assert!(meta.is_file(), "{at}: {} is a file", c.path());
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(meta.permissions().mode() & 0o111, 0, "{at}: {}", c.path());
        }
    }

    #[test]
    fn every_checked_in_scenario_applies_to_its_vendored_fixture() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let all = Scenarios::parse(SCENARIOS_TOML, &catalog).unwrap();
        for f in catalog.fixtures() {
            let dir = fixtures_dir().join(f.name.as_str());
            for name in ScenarioName::SCRIPTED {
                let edits = all.edits(f.name.as_str(), name).unwrap();
                let changes = plan(edits, |path| read_in(&dir, path))
                    .unwrap_or_else(|e| panic!("{}/{name}: {e}", f.name));
                assert!(!changes.is_empty());
                assert_regular(&dir, &format!("{}/{name}", f.name), &changes);
            }
        }
        for a in catalog.adversarial() {
            let dir = fixtures_dir().join(a.name.as_str());
            let cases: Vec<_> = all.cases(a.name.as_str()).collect();
            assert!(
                cases.len() > 1,
                "{}: leaf_pass and at least one probe",
                a.name
            );
            for (name, case) in cases {
                let at = format!("{}/{name}", a.name);
                let changes = plan(&case.edits, |path| read_in(&dir, path))
                    .unwrap_or_else(|e| panic!("{at}: {e}"));
                assert!(!changes.is_empty(), "{at}");
                assert_regular(&dir, &at, &changes);
            }
        }
    }

    #[test]
    fn every_breaking_case_also_edits_its_leaf() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let all = Scenarios::parse(SCENARIOS_TOML, &catalog).unwrap();
        for a in catalog.adversarial() {
            let fixture = a.name.as_str();
            let leaf = &all.case(fixture, LEAF_PASS).unwrap().edits;
            for (name, case) in all.cases(fixture) {
                if matches!(case.expect, Expect::Fail { .. } | Expect::Limit { .. }) {
                    assert!(
                        leaf.iter().all(|e| case.edits.contains(e)),
                        "{fixture}/{name} makes the leaf edit of {LEAF_PASS}"
                    );
                }
            }
        }
    }

    #[test]
    fn apply_dir_writes_only_after_every_check() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
        let bad = [
            Edit::Append {
                path: p("a.txt"),
                text: "1".into(),
            },
            Edit::Delete {
                path: p("gone.txt"),
            },
        ];
        assert_eq!(
            apply_dir(tmp.path(), &bad),
            Err(ApplyError::Missing("gone.txt".into()))
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "a"
        );
        let good = [
            Edit::Create {
                path: p("d/new.txt"),
                text: "n".into(),
            },
            Edit::Replace {
                path: p("a.txt"),
                find: "a".into(),
                text: "b".into(),
            },
        ];
        apply_dir(tmp.path(), &good).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("d/new.txt")).unwrap(),
            "n"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "b"
        );
        apply_dir(
            tmp.path(),
            &[Edit::Delete {
                path: p("d/new.txt"),
            }],
        )
        .unwrap();
        assert!(!tmp.path().join("d/new.txt").exists());
    }

    #[test]
    fn preconditions() {
        let files = |path: &str| Ok(["x", "y"].contains(&path).then(|| b"aXa".to_vec()));
        let check = |e: Edit| plan(&[e], files);
        assert_eq!(
            check(Edit::Create {
                path: p("x"),
                text: String::new()
            }),
            Err(ApplyError::Exists("x".into()))
        );
        assert_eq!(
            check(Edit::Rename {
                from: p("x"),
                to: p("y")
            }),
            Err(ApplyError::Exists("y".into()))
        );
        assert_eq!(
            check(Edit::Rename {
                from: p("z"),
                to: p("w")
            }),
            Err(ApplyError::Missing("z".into()))
        );
        assert_eq!(
            check(Edit::Replace {
                path: p("x"),
                find: "a".into(),
                text: "b".into()
            }),
            Err(ApplyError::Ambiguous {
                path: "x".into(),
                count: 2
            })
        );
        assert_eq!(
            check(Edit::Replace {
                path: p("x"),
                find: "Q".into(),
                text: "b".into()
            }),
            Err(ApplyError::Ambiguous {
                path: "x".into(),
                count: 0
            })
        );
        assert_eq!(
            check(Edit::Append {
                path: p("z"),
                text: String::new()
            }),
            Err(ApplyError::Missing("z".into()))
        );
        let read_error = plan(&[Edit::Delete { path: p("x") }], |_| {
            Err("denied".to_owned())
        });
        assert_eq!(
            read_error,
            Err(ApplyError::Read {
                path: "x".into(),
                why: "denied".into()
            })
        );
        // A deleted file can be created again in the same scenario; the final state wins.
        let again = plan(
            &[
                Edit::Delete { path: p("x") },
                Edit::Create {
                    path: p("x"),
                    text: "new".into(),
                },
            ],
            files,
        )
        .unwrap();
        assert_eq!(
            again,
            [Change::Write {
                path: p("x"),
                content: b"new".to_vec()
            }]
        );
        let binary = plan(
            &[Edit::Replace {
                path: p("b"),
                find: "a".into(),
                text: "b".into(),
            }],
            |_| Ok(Some(vec![0xff, 0xfe])),
        );
        assert!(matches!(binary, Err(ApplyError::Read { .. })));
    }

    #[test]
    fn file_rules() {
        let catalog = Catalog::parse(FIXTURES_TOML).unwrap();
        let head = "[snippets]\ns = \"x\"\n";
        let scenario = |fixture: &str, name: &str, edits: &str| {
            format!("[[scenario]]\nfixture = \"{fixture}\"\nname = \"{name}\"\nedits = [{edits}]\n")
        };
        let case = |fixture: &str, name: &str, fields: &str| {
            format!(
                "[[scenario]]\nfixture = \"{fixture}\"\nname = \"{name}\"\nclass = \"adversarial\"\n{fields}edits = [{{ delete = \"a\" }}]\n"
            )
        };
        let complete: String = catalog
            .fixtures()
            .iter()
            .flat_map(|f| {
                ScenarioName::SCRIPTED
                    .map(|n| scenario(f.name.as_str(), n.as_str(), "{ delete = \"a\" }"))
            })
            .chain(
                catalog
                    .adversarial()
                    .iter()
                    .map(|a| case(a.name.as_str(), LEAF_PASS, "expect = \"pass\"\n")),
            )
            .collect();
        assert!(Scenarios::parse(&format!("{head}{complete}"), &catalog).is_ok());
        let err = |extra: &str| {
            Scenarios::parse(&format!("{head}{complete}{extra}"), &catalog)
                .unwrap_err()
                .to_string()
        };
        assert!(err(&scenario("go-small", "leaf", "{ delete = \"a\" }")).contains("defined twice"));
        assert!(err(&scenario("nope", "leaf", "{ delete = \"a\" }")).contains("no such fixture"));
        assert!(err(&scenario("go-small", "baseline", "{ delete = \"a\" }")).contains("implicit"));
        assert!(err(&scenario("go-small", "leaf", "")).contains("implicit"));
        assert!(
            err(&scenario(
                "go-small",
                "leaf",
                "{ append = \"a\", snippet = \"q\" }"
            ))
            .contains("no snippet")
        );
        assert!(
            err(&scenario("go-small", "leaf", "{ append = \"a\" }")).contains("needs a snippet")
        );
        for bad in [
            "{ append = \"a\", delete = \"b\", snippet = \"s\" }",
            "{ delete = \"a\", snippet = \"s\" }",
            "{ rename = \"a\" }",
            "{ replace = \"a\", snippet = \"s\" }",
            "{ replace = \"a\", find = \"\", snippet = \"s\" }",
            "{ create = \"a\", to = \"b\", snippet = \"s\" }",
        ] {
            assert!(
                err(&scenario("go-small", "leaf", bad)).contains("exactly one"),
                "{bad}"
            );
        }
        assert!(
            err(&scenario("go-small", "leaf", "{ delete = \"../a\" }")).contains("relative path")
        );
        // Adversarial cases.
        let fail = "expect = \"fail\"\n";
        let parsed = Scenarios::parse(
            &format!(
                "{head}{complete}{}{}{}{}",
                case(
                    "adv-go",
                    "a",
                    &format!("{fail}breaks = \"example.com/adv/x\"\nwhy = \"unmapped\"\n")
                ),
                case("adv-go", "b", fail),
                case(
                    "adv-go",
                    "c",
                    &format!("{fail}limit = true\nbreaks = \"u\"\n")
                ),
                case("adv-go", "d", "expect = \"full\"\nwhy = \"global\"\n"),
            ),
            &catalog,
        )
        .unwrap();
        let expects: Vec<&Expect> = parsed.cases("adv-go").map(|(_, c)| &c.expect).collect();
        let unit = |s: &str| Unit(s.to_owned());
        assert_eq!(
            expects,
            [
                &Expect::Pass,
                &Expect::Fail {
                    breaks: Some(unit("example.com/adv/x")),
                    why: Some(Why::Unmapped)
                },
                &Expect::Fail {
                    breaks: None,
                    why: None
                },
                &Expect::Limit { breaks: unit("u") },
                &Expect::Full { why: Why::Global },
            ]
        );
        assert_eq!(
            parsed.edits_named("adv-go", "d"),
            Some(&[Edit::Delete { path: p("a") }][..])
        );
        for bad in [
            "",
            "expect = \"pass\"\nbreaks = \"u\"\n",
            "expect = \"pass\"\nwhy = \"global\"\n",
            "expect = \"full\"\n",
            "expect = \"full\"\nwhy = \"global\"\nbreaks = \"u\"\n",
            "expect = \"fail\"\nlimit = true\n",
            "expect = \"fail\"\nlimit = true\nbreaks = \"u\"\nwhy = \"global\"\n",
            "expect = \"pass\"\nlimit = true\n",
            "expect = \"full\"\nwhy = \"global\"\nlimit = false\nbreaks = \"u\"\n",
        ] {
            assert!(
                err(&case("adv-go", "x", bad)).contains("a case expects"),
                "{bad}"
            );
        }
        assert!(
            err(&case(
                "adv-go",
                "x",
                "expect = \"fail\"\nbreaks = \"a b\"\n"
            ))
            .contains("not a unit")
        );
        assert!(
            err(&case("adv-go", "x", "expect = \"fail\"\nwhy = \"plan\"\n"))
                .contains("unknown variant")
        );
        assert!(err(&case("adv-go", LEAF_PASS, fail)).contains("defined twice"));
        assert!(err(&case("adv-go", "leaf", fail)).contains("not a case name"));
        assert!(err(&case("go-small", "x", fail)).contains("no such adversarial fixture"));
        assert!(
            err(&scenario("adv-go", "leaf", "{ delete = \"a\" }"))
                .contains("class = \"adversarial\"")
        );
        assert!(
            err("[[scenario]]\nfixture = \"go-small\"\nname = \"leaf\"\nwhy = \"global\"\nedits = []\n")
                .contains("adversarial cases' fields")
        );
        assert!(
            err("[[scenario]]\nfixture = \"adv-go\"\nname = \"x\"\nclass = \"adversarial\"\nexpect = \"pass\"\nedits = []\n")
                .contains("a case has edits")
        );
        assert!(
            err(&case("adv-go", "x", "expect = \"pass\"\n")
                .replace("delete = \"a\"", "append = \"a\", snippet = \"q\""))
            .contains("no snippet")
        );
        // Every adversarial fixture has leaf_pass, expecting pass.
        let without_leaf = complete.replace(
            &case("adv-go", LEAF_PASS, "expect = \"pass\"\n"),
            &case("adv-go", LEAF_PASS, "expect = \"full\"\nwhy = \"global\"\n"),
        );
        let missing = Scenarios::parse(&format!("{head}{without_leaf}"), &catalog)
            .unwrap_err()
            .to_string();
        assert!(
            missing.contains("missing scenarios: adv-go/leaf_pass (expecting pass)"),
            "{missing}"
        );

        let partial: String = complete.lines().skip(4).map(|l| format!("{l}\n")).collect();
        let missing = Scenarios::parse(&format!("{head}{partial}"), &catalog)
            .unwrap_err()
            .to_string();
        assert!(
            missing.contains("missing scenarios: rust-small/leaf"),
            "{missing}"
        );
    }

    #[test]
    fn names() {
        for n in std::iter::once(ScenarioName::Baseline).chain(ScenarioName::SCRIPTED) {
            assert_eq!(n.as_str().parse::<ScenarioName>(), Ok(n));
            assert_eq!(
                serde_json::to_string(&n).unwrap(),
                format!("\"{}\"", n.as_str())
            );
        }
    }
}
