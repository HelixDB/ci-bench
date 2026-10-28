//! Reports over line-based files. Depends on no other package of the workspace: its tests read
//! `adv-core`'s test data and run `adv-tool` at run time only.

/// The non-empty lines of `text`, trimmed.
#[must_use]
pub fn lines(text: &str) -> Vec<&str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
}
