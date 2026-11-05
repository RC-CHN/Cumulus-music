//! M/16 radioOrderMap values, isolated by authenticated account on this device.
use crate::{Result, storage::StateTransaction};
use serde_json::Value;

const KIND: &str = "podcast-radio-order-v1";

pub(crate) fn read(tx: &StateTransaction<'_>, owner: &str, radio: &str) -> Result<Option<bool>> {
    Ok(tx
        .get(owner, KIND, radio)?
        .and_then(|value| value.as_bool()))
}

pub(super) fn write(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    radio: &str,
    ascending: bool,
) -> Result<()> {
    tx.put(owner, KIND, radio, &Value::Bool(ascending))
}
