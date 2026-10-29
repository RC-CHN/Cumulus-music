//! Real OS keys -> native Tauri host -> independent Rust core -> libmpv/null.
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/media_host.rs"]
mod proof;
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
fn main() {
    proof::run_shortcuts();
}
