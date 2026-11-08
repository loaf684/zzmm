//! GameBanana (apiv11) integration: browsing, caching, downloads and updates.

use std::sync::LazyLock;

pub mod cache;
pub mod client;
pub mod commands;
pub mod download;
pub mod gamebanana;
pub mod model;

/// GameBanana game ID for Zenless Zone Zero.
pub const GAME_ID: u64 = 19567;

/// Public project/contact page included in the User-Agent so GameBanana can
/// reach the maintainer instead of silently blocking the app. Replace this with
/// the repository or support page if it changes.
pub const CONTACT_URL: &str = "https://github.com/loaf684/zzmm";

/// GameBanana asks API users to identify themselves; a stable UA also lets them
/// contact us instead of blocking the app outright.
pub static USER_AGENT: LazyLock<String> = LazyLock::new(|| {
    format!(
        "ZZMM/{} (ZZMI mod manager; +{})",
        env!("CARGO_PKG_VERSION"),
        CONTACT_URL
    )
});

#[cfg(test)]
mod tests {
    #[test]
    fn user_agent_includes_the_contact_url() {
        assert!(super::USER_AGENT.contains(super::CONTACT_URL));
    }
}
