//! An application on `adv-core`.

/// The greeting, capitalized.
#[must_use]
pub fn banner() -> String {
    adv_core::util::capitalize(adv_core::GREETING.trim())
}

#[cfg(test)]
mod tests {
    #[test]
    fn banner_is_capitalized() {
        assert!(super::banner().starts_with(char::is_uppercase));
    }
}
