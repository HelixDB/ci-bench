//! A leaf: no package depends on it.

/// The sum of `a` and `b`.
#[must_use]
pub const fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    #[test]
    fn adds() {
        assert_eq!(super::add(2, 3), 5);
    }
}
