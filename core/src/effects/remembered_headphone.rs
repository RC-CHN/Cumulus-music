use crate::{Error, Result, effects::catalog::EffectId, storage::StateStore};
use serde::{Deserialize, Serialize};

const KIND: &str = "headphone-tuning-v1";
const KEY: &str = "last-configured";

/// A local selection, not a material handle or a request to enable anything.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RememberedHeadphone {
    pub brand: EffectId,
    pub model: EffectId,
    pub revision: String,
}

impl RememberedHeadphone {
    pub(crate) fn read(store: &StateStore, owner: &str) -> Result<Option<Self>> {
        store
            .get(owner, KIND, KEY)?
            .map(|value| {
                let value: Self = serde_json::from_value(value)
                    .map_err(|_| Error::State("invalid remembered headphone selection"))?;
                if value.revision.len() != 32
                    || !value
                        .revision
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                {
                    return Err(Error::State("invalid remembered headphone selection"));
                }
                Ok(value)
            })
            .transpose()
    }

    pub(crate) fn record(
        store: &StateStore,
        owner: &str,
        brand: EffectId,
        model: EffectId,
    ) -> Result<()> {
        let value = Self {
            brand,
            model,
            revision: uuid::Uuid::new_v4().simple().to_string(),
        };
        store.put(owner, KIND, KEY, &serde_json::to_value(value)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn malformed_record_is_not_returned_and_confirmed_choice_can_replace_it() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path()).unwrap();
        store
            .put(
                "1",
                KIND,
                KEY,
                &json!({"brand":0,"model":7,"revision":"invalid"}),
            )
            .unwrap();
        assert!(matches!(
            RememberedHeadphone::read(&store, "1"),
            Err(Error::State("invalid remembered headphone selection"))
        ));
        assert!(RememberedHeadphone::read(&store, "2").unwrap().is_none());
        let brand = EffectId::new(4).unwrap();
        let model = EffectId::new(7).unwrap();
        RememberedHeadphone::record(&store, "1", brand, model).unwrap();
        let repaired = RememberedHeadphone::read(&store, "1").unwrap().unwrap();
        assert_eq!((repaired.brand, repaired.model), (brand, model));
        assert!(RememberedHeadphone::read(&store, "2").unwrap().is_none());
    }
}
