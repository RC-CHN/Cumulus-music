use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct DevicePlan {
    pub key: String,
    pub deferred: bool,
    category: Option<String>,
}
pub(super) fn device_plan(config: &Config) -> Result<Vec<DevicePlan>> {
    let mut plan: Vec<DevicePlan> = serde_json::from_str(include_str!("../device-plan.json"))?;
    let options = config.collection_options();
    plan.retain(|field| match field.category.as_deref() {
        None | Some("canvas") => true,
        Some("fonts") => options["ejcd"].as_bool() == Some(true),
        Some("webgl") => options["ews"].as_bool() == Some(true),
        Some(_) => false,
    });
    // The recovered raw configuration parser never maps a canvas toggle.
    // Lc's internal default stays true; an invented raw Lc cannot disable it.
    plan.sort_by_key(|entry| entry.deferred);
    Ok(plan)
}
pub(super) fn environment_keys(expanded: bool) -> Result<Vec<String>> {
    let groups: serde_json::Value = serde_json::from_str(include_str!("../collectors.json"))?;
    groups["environment"]
        .as_array()
        .ok_or(Error::State("missing environment group"))?
        .iter()
        .take(if expanded { 21 } else { 6 })
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or(Error::State("invalid environment key"))
        })
        .collect()
}
