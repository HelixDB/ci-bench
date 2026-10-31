/// `s` with its first letter upper-cased.
#[must_use]
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn capitalizes() {
        assert_eq!(super::capitalize("adv"), "Adv");
        assert_eq!(super::capitalize(""), "");
    }
}
