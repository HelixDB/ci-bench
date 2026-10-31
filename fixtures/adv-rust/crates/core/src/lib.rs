//! The fixture's core library: an embedded greeting, a `util` module in `util/mod.rs` and the
//! arithmetic `adv-app` tests.

mod math;
pub mod util;

pub use math::double;

/// The greeting, embedded from `greeting.txt` at compile time.
pub const GREETING: &str = include_str!("greeting.txt");

/// The release channel, from `.cargo/config.toml`'s `[env]`.
pub const CHANNEL: &str = env!("ADV_CHANNEL");

#[cfg(test)]
mod tests {
    #[test]
    fn channel_comes_from_the_cargo_config() {
        assert_eq!(super::CHANNEL, "stable");
    }
}
