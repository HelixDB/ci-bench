//! `pricing.toml`: dated tables of the runners' published per-minute list prices.
//!
//! A run is priced with the latest table whose `as_of` is on or before the run's day (GitHub's
//! `created_at`), so a price change adds a table and never reprices old results. Each runner of
//! [`Runner::ALL`] must be in every table, once.

use serde::Deserialize;

use crate::catalog::{Arch, Provider, Runner};

/// The checked-in price tables.
pub const PRICING_TOML: &str = include_str!("../../pricing.toml");

/// How a job's duration becomes billed time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rounding {
    /// Each job rounded up to whole minutes (GitHub's documented rule).
    Minute,
    /// Per second.
    Second,
}

/// One runner's price.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rate {
    /// The `runs-on` label.
    pub label: String,
    /// Who sells it.
    pub provider: Provider,
    /// Its architecture.
    pub arch: Arch,
    /// vCPUs this repository's jobs get.
    pub vcpus: u32,
    /// List price, USD per minute.
    pub usd_per_minute: f64,
    /// Billing granularity.
    pub rounding: Rounding,
    /// Where the price is published.
    pub source: String,
}

impl Rate {
    /// Billed seconds and USD for a job that ran `seconds`.
    ///
    /// ```
    /// use ci_bench::pricing::{Pricing, PRICING_TOML};
    /// use ci_bench::catalog::Runner;
    /// let pricing = Pricing::parse(PRICING_TOML).unwrap();
    /// let rate = pricing.rate("2026-10-26", Runner::UbuntuLatest).unwrap();
    /// assert_eq!(rate.bill(61.0), (120.0, 0.012));
    /// assert_eq!(rate.bill(0.0), (0.0, 0.0));
    /// ```
    #[must_use]
    pub fn bill(&self, seconds: f64) -> (f64, f64) {
        let billed = match self.rounding {
            Rounding::Minute => (seconds.max(0.0) / 60.0).ceil() * 60.0,
            Rounding::Second => seconds.max(0.0).ceil(),
        };
        // Cents to 1e-6 dollars: no float dust in the JSON.
        let usd = (billed / 60.0 * self.usd_per_minute * 1e6).round() / 1e6;
        (billed, usd)
    }
}

/// One dated table.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// The day the prices were read (`YYYY-MM-DD`).
    pub as_of: String,
    /// Every runner's price.
    pub runner: Vec<Rate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    table: Vec<Table>,
}

/// Why the price file is unusable.
#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    /// Not TOML of this schema.
    #[error("pricing.toml: {0}")]
    Toml(#[from] toml::de::Error),
    /// A rule is broken.
    #[error("pricing.toml: {0}")]
    Invalid(String),
}

/// Every table, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Pricing {
    tables: Vec<Table>,
}

fn is_day(s: &str) -> bool {
    crate::timing::unix(&format!("{s}T00:00:00Z")).is_some() && s.len() == 10
}

impl Pricing {
    /// Parses and checks the tables: valid, strictly increasing days; every [`Runner`] exactly
    /// once per table with its provider and architecture; positive finite prices.
    ///
    /// # Errors
    /// [`PricingError`] naming the rule.
    pub fn parse(text: &str) -> Result<Self, PricingError> {
        let raw: Raw = toml::from_str(text)?;
        let invalid = |why: String| Err(PricingError::Invalid(why));
        if raw.table.is_empty() {
            return invalid("no tables".to_owned());
        }
        for (i, t) in raw.table.iter().enumerate() {
            if !is_day(&t.as_of) {
                return invalid(format!("{:?} is not a YYYY-MM-DD day", t.as_of));
            }
            if i > 0 && raw.table[i - 1].as_of >= t.as_of {
                return invalid(format!(
                    "{}: tables must be in increasing as_of order",
                    t.as_of
                ));
            }
            for r in Runner::ALL {
                let rates: Vec<&Rate> = t.runner.iter().filter(|x| x.label == r.label()).collect();
                let [rate] = rates.as_slice() else {
                    return invalid(format!("{}: {r} must appear exactly once", t.as_of));
                };
                if rate.provider != r.provider() || rate.arch != r.arch() {
                    return invalid(format!("{}: {r} has the wrong provider or arch", t.as_of));
                }
                if !(rate.usd_per_minute.is_finite() && rate.usd_per_minute > 0.0)
                    || rate.vcpus == 0
                {
                    return invalid(format!("{}: {r} needs a positive price and vCPUs", t.as_of));
                }
            }
            if let Some(stray) = t
                .runner
                .iter()
                .find(|x| Runner::of_label(&x.label).is_none())
            {
                return invalid(format!("{}: unknown runner {}", t.as_of, stray.label));
            }
        }
        Ok(Self { tables: raw.table })
    }

    /// The table in force on `day` (`YYYY-MM-DD`): the latest whose `as_of` is not after it.
    ///
    /// ```
    /// let p = ci_bench::pricing::Pricing::parse(ci_bench::pricing::PRICING_TOML).unwrap();
    /// assert!(p.table("2026-10-26").is_some());
    /// assert!(p.table("2020-01-01").is_none(), "before any table");
    /// ```
    #[must_use]
    pub fn table(&self, day: &str) -> Option<&Table> {
        self.tables.iter().rev().find(|t| t.as_of.as_str() <= day)
    }

    /// `runner`'s rate on `day`.
    #[must_use]
    pub fn rate(&self, day: &str, runner: Runner) -> Option<&Rate> {
        self.table(day)?
            .runner
            .iter()
            .find(|r| r.label == runner.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(as_of: &str, skip: Option<Runner>, extra: &str) -> String {
        let rows: String = Runner::ALL
            .into_iter()
            .filter(|r| Some(*r) != skip)
            .map(|r| {
                let provider = match r.provider() {
                    Provider::Github => "github",
                    Provider::Blacksmith => "blacksmith",
                };
                let arch = match r.arch() {
                    Arch::X86_64 => "x86_64",
                    Arch::Aarch64 => "aarch64",
                };
                format!(
                    "[[table.runner]]\nlabel = \"{r}\"\nprovider = \"{provider}\"\narch = \"{arch}\"\nvcpus = 2\nusd_per_minute = 0.004\nrounding = \"second\"\nsource = \"x\"\n"
                )
            })
            .collect();
        format!("[[table]]\nas_of = \"{as_of}\"\n{rows}{extra}")
    }

    #[test]
    fn the_checked_in_tables_parse() {
        let p = Pricing::parse(PRICING_TOML).unwrap();
        for r in Runner::ALL {
            assert!(p.rate("2026-10-26", r).is_some(), "{r}");
        }
        let arm = p.rate("2027-01-01", Runner::Blacksmith2404Arm).unwrap();
        assert_eq!(arm.usd_per_minute, 0.0025);
        assert_eq!(arm.bill(30.0), (60.0, 0.0025));
    }

    #[test]
    fn tables_by_date() {
        let text = format!(
            "{}{}",
            table("2026-01-01", None, ""),
            table("2026-06-01", None, "")
        );
        let p = Pricing::parse(&text).unwrap();
        assert_eq!(p.table("2026-05-31").unwrap().as_of, "2026-01-01");
        assert_eq!(p.table("2026-06-01").unwrap().as_of, "2026-06-01");
        let rate = p.rate("2026-06-02", Runner::UbuntuLatest).unwrap();
        assert_eq!(rate.bill(90.4), (91.0, 0.006_067));
    }

    #[test]
    fn rules() {
        let bad = |text: &str| Pricing::parse(text).unwrap_err().to_string();
        assert!(bad("table = []").contains("no tables"));
        assert!(bad(&table("2026-13-01", None, "")).contains("not a YYYY-MM-DD"));
        let reversed = format!(
            "{}{}",
            table("2026-06-01", None, ""),
            table("2026-01-01", None, "")
        );
        assert!(bad(&reversed).contains("increasing"));
        assert!(bad(&table("2026-01-01", Some(Runner::UbuntuLatest), "")).contains("exactly once"));
        let twice = table(
            "2026-01-01",
            None,
            "[[table.runner]]\nlabel = \"ubuntu-latest\"\nprovider = \"github\"\narch = \"x86_64\"\nvcpus = 2\nusd_per_minute = 1.0\nrounding = \"minute\"\nsource = \"x\"\n",
        );
        assert!(bad(&twice).contains("exactly once"));
        let stray = table(
            "2026-01-01",
            None,
            "[[table.runner]]\nlabel = \"macos-latest\"\nprovider = \"github\"\narch = \"aarch64\"\nvcpus = 2\nusd_per_minute = 1.0\nrounding = \"minute\"\nsource = \"x\"\n",
        );
        assert!(bad(&stray).contains("unknown runner"));
        let wrong_arch =
            table("2026-01-01", None, "").replacen("arch = \"x86_64\"", "arch = \"aarch64\"", 1);
        assert!(bad(&wrong_arch).contains("wrong provider or arch"));
        let free = table("2026-01-01", None, "").replacen(
            "usd_per_minute = 0.004",
            "usd_per_minute = 0.0",
            1,
        );
        assert!(bad(&free).contains("positive"));
    }
}
