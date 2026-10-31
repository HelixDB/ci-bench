//! The adversarial corpus checked on local toolchains (`ci-bench corpus`): before a selecting
//! arm is measured against a case, the case is shown to hold what it claims, with the fixture's
//! own commands ([`Adversarial`]).
//!
//! First the fixture as vendored: `install`, then `test` must pass (its cases are not checked
//! otherwise). Then per case, with the case's edits applied, `install` and:
//!
//! | Case | Holds when |
//! |---|---|
//! | `fail` with `breaks = U`, or a `limit` | `install` passes, `test` fails, `only` fails and `except` passes, both with `UNIT=U`: U and only U is broken, so a selection that skips U is a wrong green |
//! | `fail` without `breaks` | `install` or `test` fails (the change breaks the build) |
//! | `pass`, `full` | `install` and `test` pass |
//!
//! Every check runs in a fresh copy of the fixture, removed afterwards, as a fresh CI checkout
//! would: no build output carries over, so no tool's cache hides a change (cargo, for one, does
//! not rebuild a crate when a module file it never read appears beside one it did: an added
//! `util.rs` beside `util/mod.rs` builds green on a warm `target/`). Commands run with `bash -c`
//! in the copy, stdin closed, `UNIT` set for `only` and `except` and [`ENV`] set; each may take
//! [`Check::timeout`].

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use crate::catalog::Adversarial;
use crate::scenario::{self, Case, CaseName, Edit, Expect, Unit};

/// The environment every command gets on top of the caller's: Go's test cache never replays a
/// result, and Python writes no bytecode (a `.pyc` is trusted while its source keeps its size
/// and whole-second mtime, which a same-size edit within a second does).
pub const ENV: [(&str, &str); 2] = [("GOFLAGS", "-count=1"), ("PYTHONDONTWRITEBYTECODE", "1")];

/// How many trailing lines of a command's output a broken case reports.
const TAIL_LINES: usize = 20;

/// One fixture's check.
#[derive(Debug, Clone, Copy)]
pub struct Check<'a> {
    /// The fixture.
    pub fixture: &'a Adversarial,
    /// Its directory in the checkout (`fixtures/<name>`), never written.
    pub source: &'a Path,
    /// An existing directory for the copies (`<fixture>.<case>`, `<fixture>.vendored`).
    pub scratch: &'a Path,
    /// How long one command may run.
    pub timeout: Duration,
}

/// What the check found for the fixture or one of its cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The case; `None` for the fixture as vendored (`install` and `test`).
    pub case: Option<CaseName>,
    /// What held (`Ok`), or what did not (`Err`, with the output's tail).
    pub result: Result<String, String>,
}

/// One finished command.
struct Ran {
    success: bool,
    tail: String,
}

impl Check<'_> {
    /// Checks the fixture as vendored, then each of `cases` in order (module docs). A fixture
    /// that is not green ends the check there.
    pub async fn run(&self, cases: &[(&CaseName, &Case)]) -> Vec<Outcome> {
        let green = self.in_copy("vendored", &[], None).await;
        let ok = green.is_ok();
        let mut outcomes = vec![Outcome {
            case: None,
            result: green,
        }];
        if !ok {
            return outcomes;
        }
        for (name, case) in cases {
            outcomes.push(Outcome {
                case: Some((*name).clone()),
                result: self
                    .in_copy(name.as_str(), &case.edits, Some(&case.expect))
                    .await,
            });
        }
        outcomes
    }

    /// Checks `expect` (`None`: the fixture is green) on a fresh copy with `edits` applied, then
    /// removes the copy.
    async fn in_copy(
        &self,
        label: &str,
        edits: &[Edit],
        expect: Option<&Expect>,
    ) -> Result<String, String> {
        let work = self.scratch.join(format!("{}.{label}", self.fixture.name));
        copy_tree(self.source, &work).map_err(|e| format!("copying the fixture: {e}"))?;
        let result = match scenario::apply_dir(&work, edits) {
            Ok(_) => self.holds(&work, expect).await,
            Err(e) => Err(format!("the edits do not apply: {e}")),
        };
        let removed = std::fs::remove_dir_all(&work).map_err(|e| format!("removing the copy: {e}"));
        result.and_then(|held| removed.map(|()| held))
    }

    /// Whether the copy `work` behaves as `expect` says (module docs).
    async fn holds(&self, work: &Path, expect: Option<&Expect>) -> Result<String, String> {
        let f = self.fixture;
        let Some(expect) = expect else {
            self.require(work, "install", &f.install, None, true)
                .await?;
            self.require(work, "test", &f.test, None, true).await?;
            return Ok("install and test pass".to_owned());
        };
        match expect {
            Expect::Fail { breaks: None, .. } => {
                let install = self.sh(work, &f.install, None).await?;
                if !install.success {
                    return Ok("install fails".to_owned());
                }
                self.require(work, "test", &f.test, None, false).await?;
                Ok("test fails".to_owned())
            }
            Expect::Fail {
                breaks: Some(unit), ..
            }
            | Expect::Limit { breaks: unit } => {
                self.require(work, "install", &f.install, None, true)
                    .await?;
                self.require(work, "test", &f.test, None, false).await?;
                self.require(work, "only", &f.only, Some(unit), false)
                    .await?;
                self.require(work, "except", &f.except, Some(unit), true)
                    .await?;
                Ok(format!(
                    "test fails, only {unit} fails, except {unit} passes"
                ))
            }
            Expect::Pass | Expect::Full { .. } => {
                self.require(work, "install", &f.install, None, true)
                    .await?;
                self.require(work, "test", &f.test, None, true).await?;
                Ok("test passes".to_owned())
            }
        }
    }

    /// Runs `command` in `work` and requires it to pass (`pass`) or fail.
    async fn require(
        &self,
        work: &Path,
        what: &str,
        command: &str,
        unit: Option<&Unit>,
        pass: bool,
    ) -> Result<(), String> {
        let ran = self.sh(work, command, unit).await?;
        if ran.success == pass {
            return Ok(());
        }
        let (want, got) = if pass {
            ("pass", "fails")
        } else {
            ("fail", "passes")
        };
        let unit = unit.map_or(String::new(), |u| format!(" (UNIT={u})"));
        Err(format!(
            "{what}{unit} {got}, it must {want}: {command}\n{}",
            ran.tail
        ))
    }

    /// `bash -c command` in the copy `work` (module docs).
    async fn sh(&self, work: &Path, command: &str, unit: Option<&Unit>) -> Result<Ran, String> {
        let mut child = tokio::process::Command::new("bash");
        child
            .arg("-c")
            .arg(command)
            .current_dir(work)
            .envs(ENV)
            .envs(unit.map(|u| ("UNIT", u.as_str())))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let out = tokio::time::timeout(self.timeout, child.output())
            .await
            .map_err(|_| format!("{command}: timed out after {:?}", self.timeout))?
            .map_err(|e| format!("{command}: {e}"))?;
        let text = [out.stdout, out.stderr].concat();
        let text = String::from_utf8_lossy(&text);
        let lines: Vec<&str> = text.lines().collect();
        Ok(Ran {
            success: out.status.success(),
            tail: lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n"),
        })
    }
}

/// Copies the directory `from` to `to` (created; it must not exist): directories and regular
/// files only (fixtures hold nothing else).
///
/// # Errors
/// An IO error, `to` existing, or an entry that is neither a file nor a directory.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    std::fs::read_dir(from)?.try_for_each(|entry| {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).map(drop)
        } else {
            Err(std::io::Error::other(format!(
                "{}: neither a file nor a directory",
                entry.path().display()
            )))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Edit, Why};

    /// A fixture of text files whose units are the files: a unit passes while it reads `ok`.
    fn fixture() -> Adversarial {
        let toml = r#"name = "adv-text"
toolchain = "go"
install = "test ! -e broken.build"
test = "for f in *.txt; do grep -qx ok \"$f\" || exit 1; done"
only = "grep -qx ok \"$UNIT\""
except = "for f in *.txt; do [ \"$f\" = \"$UNIT\" ] || grep -qx ok \"$f\" || exit 1; done"
"#;
        toml::from_str(toml).unwrap()
    }

    fn source(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn name(s: &str) -> CaseName {
        s.parse().unwrap()
    }

    fn unit(s: &str) -> Option<Unit> {
        Some(s.parse().unwrap())
    }

    fn replace(path: &str, text: &str) -> Edit {
        Edit::Replace {
            path: path.parse().unwrap(),
            find: "ok".into(),
            text: text.into(),
        }
    }

    async fn check(files: &[(&str, &str)], cases: &[(&str, Case)]) -> Vec<Outcome> {
        let src = source(files);
        let scratch = tempfile::tempdir().unwrap();
        let fixture = fixture();
        let named: Vec<(CaseName, &Case)> = cases.iter().map(|(n, c)| (name(n), c)).collect();
        let refs: Vec<(&CaseName, &Case)> = named.iter().map(|(n, c)| (n, *c)).collect();
        Check {
            fixture: &fixture,
            source: src.path(),
            scratch: scratch.path(),
            timeout: Duration::from_secs(30),
        }
        .run(&refs)
        .await
    }

    fn results(outcomes: &[Outcome]) -> Vec<Result<(), String>> {
        outcomes
            .iter()
            .map(|o| o.result.clone().map(drop))
            .collect()
    }

    const FILES: &[(&str, &str)] = &[("a.txt", "ok\n"), ("b.txt", "ok\n"), ("leaf.txt", "ok\n")];

    #[tokio::test]
    async fn breaking_cases_must_break_exactly_their_unit() {
        let leaf = Edit::Append {
            path: "leaf.txt".parse().unwrap(),
            text: String::new(),
        };
        let fail = |breaks: Option<Unit>, edits: Vec<Edit>| Case {
            edits,
            expect: Expect::Fail { breaks, why: None },
        };
        let outcomes = check(
            FILES,
            &[
                (
                    "breaks_a",
                    fail(unit("a.txt"), vec![replace("a.txt", "no"), leaf.clone()]),
                ),
                (
                    "wrong_unit",
                    fail(unit("b.txt"), vec![replace("a.txt", "no")]),
                ),
                (
                    "breaks_two",
                    fail(
                        unit("a.txt"),
                        vec![replace("a.txt", "no"), replace("b.txt", "no")],
                    ),
                ),
                ("breaks_nothing", fail(unit("a.txt"), vec![leaf.clone()])),
                (
                    "limit",
                    Case {
                        edits: vec![replace("b.txt", "no")],
                        expect: Expect::Limit {
                            breaks: "b.txt".parse().unwrap(),
                        },
                    },
                ),
            ],
        )
        .await;
        assert_eq!(outcomes[0].case, None);
        assert_eq!(outcomes[0].result, Ok("install and test pass".to_owned()));
        assert_eq!(
            outcomes[1].result,
            Ok("test fails, only a.txt fails, except a.txt passes".to_owned())
        );
        let errs: Vec<String> = outcomes[2..4]
            .iter()
            .map(|o| o.result.clone().unwrap_err())
            .collect();
        assert!(
            errs[0].starts_with("only (UNIT=b.txt) passes, it must fail"),
            "{}",
            errs[0]
        );
        assert!(
            errs[1].starts_with("except (UNIT=a.txt) fails, it must pass"),
            "{}",
            errs[1]
        );
        assert!(
            outcomes[4]
                .result
                .clone()
                .unwrap_err()
                .starts_with("test passes, it must fail")
        );
        assert!(outcomes[5].result.is_ok(), "{:?}", outcomes[5]);
        assert_eq!(outcomes.len(), 6, "the copy was reset after each case");
    }

    #[tokio::test]
    async fn build_failures_pass_and_full_cases() {
        let build = |edits| Case {
            edits,
            expect: Expect::Fail {
                breaks: None,
                why: Some(Why::Shadowing),
            },
        };
        let green = |edits, expect| Case { edits, expect };
        let marker = Edit::Create {
            path: "broken.build".parse().unwrap(),
            text: String::new(),
        };
        let outcomes = check(
            FILES,
            &[
                ("install_breaks", build(vec![marker])),
                ("test_breaks", build(vec![replace("a.txt", "no")])),
                ("nothing_breaks", build(vec![replace("a.txt", "ok")])),
                ("passes", green(vec![replace("a.txt", "ok")], Expect::Pass)),
                (
                    "full_but_broken",
                    green(
                        vec![replace("a.txt", "no")],
                        Expect::Full { why: Why::Global },
                    ),
                ),
            ],
        )
        .await;
        let got: Vec<_> = outcomes.iter().map(|o| o.result.clone()).collect();
        assert_eq!(got[1], Ok("install fails".to_owned()));
        assert_eq!(got[2], Ok("test fails".to_owned()));
        assert!(
            got[3]
                .clone()
                .unwrap_err()
                .starts_with("test passes, it must fail")
        );
        assert_eq!(got[4], Ok("test passes".to_owned()));
        assert!(
            got[5]
                .clone()
                .unwrap_err()
                .starts_with("test fails, it must pass")
        );
    }

    #[tokio::test]
    async fn every_check_gets_a_fresh_copy() {
        // The test leaves a file behind, as build tools do: a reused copy would fail the next
        // check, and so would a copy that kept an earlier case's edits.
        let mut fixture = fixture();
        fixture.test =
            "test ! -e leftover && touch leftover && test ! -e new && grep -qx ok a.txt".to_owned();
        let src = source(FILES);
        let scratch = tempfile::tempdir().unwrap();
        let p = |s: &str| s.parse().unwrap();
        let cases = [
            Case {
                edits: vec![
                    Edit::Create {
                        path: p("new/x.txt"),
                        text: "ok\n".into(),
                    },
                    Edit::Rename {
                        from: p("a.txt"),
                        to: p("moved/a.txt"),
                    },
                ],
                expect: Expect::Fail {
                    breaks: None,
                    why: None,
                },
            },
            Case {
                edits: vec![Edit::Delete { path: p("b.txt") }],
                expect: Expect::Pass,
            },
            Case {
                edits: vec![replace("leaf.txt", "ok")],
                expect: Expect::Pass,
            },
        ];
        let names = [name("first"), name("second"), name("third")];
        let refs: Vec<(&CaseName, &Case)> = names.iter().zip(&cases).collect();
        let outcomes = Check {
            fixture: &fixture,
            source: src.path(),
            scratch: scratch.path(),
            timeout: Duration::from_secs(30),
        }
        .run(&refs)
        .await;
        assert_eq!(
            results(&outcomes),
            [Ok(()), Ok(()), Ok(()), Ok(())],
            "{outcomes:?}"
        );
        assert_eq!(
            std::fs::read_dir(scratch.path()).unwrap().count(),
            0,
            "every copy is removed"
        );
        let names: Vec<_> = std::fs::read_dir(src.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), FILES.len(), "the source is never written");
    }

    #[tokio::test]
    async fn a_fixture_that_is_not_green_stops_the_check() {
        let case = Case {
            edits: vec![replace("a.txt", "no")],
            expect: Expect::Pass,
        };
        let outcomes = check(&[("a.txt", "no\n")], &[("never", case)]).await;
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].case, None);
        assert!(
            outcomes[0]
                .result
                .clone()
                .unwrap_err()
                .starts_with("test fails, it must pass"),
            "{outcomes:?}"
        );
    }

    #[tokio::test]
    async fn edits_that_do_not_apply_and_timeouts_are_reported() {
        let missing = Case {
            edits: vec![replace("nope.txt", "no")],
            expect: Expect::Pass,
        };
        let outcomes = check(FILES, &[("missing", missing)]).await;
        assert_eq!(
            outcomes[1].result,
            Err("the edits do not apply: nope.txt: no such file".to_owned())
        );

        let src = source(FILES);
        let scratch = tempfile::tempdir().unwrap();
        let mut slow = fixture();
        slow.install = "sleep 5".to_owned();
        let outcomes = Check {
            fixture: &slow,
            source: src.path(),
            scratch: scratch.path(),
            timeout: Duration::from_millis(200),
        }
        .run(&[])
        .await;
        assert_eq!(
            outcomes[0].result,
            Err("sleep 5: timed out after 200ms".to_owned())
        );

        let existing = tempfile::tempdir().unwrap();
        std::fs::create_dir(existing.path().join("adv-text.vendored")).unwrap();
        let outcomes = Check {
            fixture: &slow,
            source: src.path(),
            scratch: existing.path(),
            timeout: Duration::from_secs(1),
        }
        .run(&[])
        .await;
        assert!(
            outcomes[0]
                .result
                .clone()
                .unwrap_err()
                .starts_with("copying the fixture"),
            "the scratch copy must be new"
        );
    }

    #[tokio::test]
    async fn commands_run_without_result_caches() {
        let mut fixture = fixture();
        fixture.install = ENV.map(|(k, v)| format!("[ \"${k}\" = {v} ]")).join(" && ");
        let src = source(FILES);
        let scratch = tempfile::tempdir().unwrap();
        let outcomes = Check {
            fixture: &fixture,
            source: src.path(),
            scratch: scratch.path(),
            timeout: Duration::from_secs(30),
        }
        .run(&[])
        .await;
        assert_eq!(results(&outcomes), [Ok(())], "{outcomes:?}");
    }

    #[test]
    fn copy_tree_refuses_symlinks() {
        let src = source(FILES);
        std::os::unix::fs::symlink("a.txt", src.path().join("link")).unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let err = copy_tree(src.path(), &scratch.path().join("copy")).unwrap_err();
        assert!(
            err.to_string().contains("neither a file nor a directory"),
            "{err}"
        );
    }
}
