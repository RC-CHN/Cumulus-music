use serde::Serialize;

/// Public information shared by every host, without paths or credentials.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub core_version: &'static str,
    pub protocol_version: u32,
}

/// The entry point for host-independent use cases.
///
/// Music services will be migrated here from the reference implementation;
/// creating this application does not load or authenticate a reference session.
#[derive(Default)]
pub struct Application;

impl Application {
    pub fn info(&self) -> AppInfo {
        AppInfo {
            name: "Cumulus",
            core_version: env!("CARGO_PKG_VERSION"),
            protocol_version: 1,
        }
    }
}
