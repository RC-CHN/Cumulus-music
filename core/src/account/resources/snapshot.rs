use super::ResourceKind;
use crate::{
    Error, Result,
    account::AccountApi,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) async fn read(api: Arc<dyn Api>, uid: &str, kind: ResourceKind) -> Result<Value> {
    if kind == ResourceKind::Mvs {
        return Ok(serde_json::to_value(
            crate::mv::MvApi::new(api).collection().await?,
        )?);
    }
    let api = AccountApi::new(api);
    match kind {
        ResourceKind::Liked => Ok(json!(api.liked_ids(&json!(uid)).await?)),
        ResourceKind::Albums => Ok(json!(api.collected_albums().await?)),
        ResourceKind::Artists => Ok(json!(api.collected_artists().await?)),
        ResourceKind::Settings => api.settings().await,
        ResourceKind::Mvs => unreachable!("handled before AccountApi"),
    }
}
pub(super) fn value(kind: ResourceKind, key: &str, snapshot: &Value) -> Result<Value> {
    if kind == ResourceKind::Settings {
        return snapshot.get(key).cloned().ok_or(Error::Response(
            "setting is absent; original state cannot be restored",
        ));
    }
    let rows = snapshot
        .as_array()
        .ok_or(Error::Response("resource list is missing"))?;
    let mut found = false;
    for row in rows {
        let id = identifier(if kind == ResourceKind::Liked {
            row
        } else {
            &row["id"]
        })
        .map_err(|_| Error::Response("resource has an invalid identifier"))?;
        found |= id == key;
    }
    Ok(json!(found))
}
pub(super) fn same_type(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(_), Value::Bool(_)) => true,
        (Value::Number(a), Value::Number(b)) => {
            a.is_i64() == b.is_i64() && a.is_u64() == b.is_u64() && a.is_f64() == b.is_f64()
        }
        _ => false,
    }
}
