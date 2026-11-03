use super::*;
use crate::{catalog::CatalogApi, protocol::api::Api};
use serde_json::json;
pub struct FmDiscoveryApi {
    api: Arc<dyn Api>,
}
impl FmDiscoveryApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn page(&self, query: &FmDiscoveryQuery) -> Result<FmDiscoveryPage> {
        query.validate()?;
        match query {
            FmDiscoveryQuery::Home => {
                let now = time::OffsetDateTime::now_utc();
                let client_time = format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    now.year(),
                    u8::from(now.month()),
                    now.day(),
                    now.hour(),
                    now.minute(),
                    now.second()
                );
                let features = CatalogApi::new(self.api.clone())
                    .daily_mix(&client_time)
                    .await?;
                let tabs = self
                    .api
                    .call("/api/secondary/tab/list", json!({}), false, &[200])
                    .await?;
                normalize::home(&features, &tabs)
            }
            FmDiscoveryQuery::Zone { page_code, tab_key } => {
                let first = self
                    .api
                    .call(
                        "/api/zone/tab/get",
                        json!({"pageCode":page_code,"tabKey":tab_key}),
                        false,
                        &[200],
                    )
                    .await;
                match first.and_then(|v| normalize::zone(&v, tab_key, false)) {
                    Ok(page) => Ok(page),
                    Err(e)
                        if e.is_service_code(301)
                            || matches!(e, Error::StaleResponse | Error::CredentialChanged) =>
                    {
                        Err(e)
                    }
                    Err(_) => {
                        let value = self
                            .api
                            .call(
                                "/api/zone/get",
                                json!({"pageCode":page_code}),
                                false,
                                &[200],
                            )
                            .await?;
                        normalize::zone(&value, "default", true)
                    }
                }
            }
        }
    }
}
