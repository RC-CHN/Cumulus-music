use super::*;

fn number(value: &Value) -> Result<Option<u64>> {
    match value {
        Value::Null => Ok(None),
        Value::Number(n) => n
            .as_u64()
            .map(Some)
            .ok_or(Error::Response("invalid cloud size")),
        Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s
            .parse()
            .map(Some)
            .map_err(|_| Error::Response("cloud size overflow")),
        _ => Err(Error::Response("invalid cloud numeric field")),
    }
}
fn metadata(raw: &Value, id: &str) -> Result<Value> {
    let simple = &raw["simpleSong"];
    if !simple.is_null() && !simple.is_object() {
        return Err(Error::Response("invalid cloud song metadata"));
    }
    let title = simple["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| raw["songName"].as_str())
        .or_else(|| raw["fileName"].as_str())
        .unwrap_or("");
    let artists = simple["ar"]
        .as_array()
        .filter(|a| !a.is_empty())
        .cloned()
        .unwrap_or_else(|| vec![json!({"name":raw["artist"].as_str().unwrap_or("")})]);
    let mut album = simple["al"].as_object().cloned().unwrap_or_default();
    if album
        .get("name")
        .and_then(Value::as_str)
        .is_none_or(|s| s.is_empty())
    {
        album.insert("name".into(), json!(raw["album"].as_str().unwrap_or("")));
    }
    Ok(
        json!({"id":id,"name":title,"ar":artists,"al":album,"dt":number(&simple["dt"])?.unwrap_or(0),"fee":simple["fee"].as_u64().unwrap_or(0),"privilege":simple["privilege"],"_cloud_sort":{"title":row_string(raw,"songName"),"artist":row_string(raw,"artist"),"album":row_string(raw,"album")}}),
    )
}
fn row_string(row: &Value, key: &str) -> String {
    row[key].as_str().unwrap_or("").into()
}
impl CloudApi {
    /// Complete, authoritative account list plus capacity; never commits a
    /// truncated first page as a successful snapshot (M/3127 uses 200/page).
    pub async fn snapshot(&self) -> Result<CloudSnapshot> {
        let mut items = Vec::new();
        let mut seen = BTreeSet::new();
        let mut capacity = None;
        let mut used = None;
        for _ in 0..1000 {
            let response = self
                .api
                .call(
                    "/api/v1/cloud/get",
                    json!({"offset":items.len(),"limit":200}),
                    false,
                    &[200],
                )
                .await?;
            for (saved, next) in [
                (&mut capacity, number(&response["maxSize"])?),
                (&mut used, number(&response["size"])?),
            ] {
                if let Some(next) = next {
                    if saved.is_some_and(|old| old != next) {
                        return Err(Error::Response("cloud capacity changed while paging"));
                    }
                    *saved = Some(next);
                }
            }
            let rows = response["data"]
                .as_array()
                .ok_or(Error::Response("missing cloud list"))?;
            if rows.len() > 200 {
                return Err(Error::Response("cloud page exceeds requested limit"));
            }
            for row in rows {
                let song_id = crate::protocol::api::identifier(&row["songId"])?;
                if !seen.insert(song_id.clone()) {
                    return Err(Error::Response("cloud list repeated an item"));
                }
                let song = metadata(row, &song_id)?;
                items.push(CloudEntry {
                    song_id,
                    song,
                    file_name: row["fileName"].as_str().unwrap_or("").into(),
                    file_size: number(&row["fileSize"])?.unwrap_or(0),
                    added_at: number(&row["addTime"])?.unwrap_or(0),
                    bitrate: number(&row["bitrate"])?.unwrap_or(0),
                });
            }
            let more = response
                .get("hasMore")
                .or_else(|| response.get("more"))
                .unwrap_or(&Value::Null);
            if !more.is_null() && !more.is_boolean() {
                return Err(Error::Response("invalid cloud page flag"));
            }
            if response["hasMore"].is_boolean()
                && response["more"].is_boolean()
                && response["hasMore"] != response["more"]
            {
                return Err(Error::Response("conflicting cloud pagination flags"));
            }
            if more == true && rows.is_empty() {
                return Err(Error::Response("empty nonfinal cloud page"));
            }
            if more == false || (more.is_null() && rows.len() < 200) {
                return CloudSnapshot::new(items, used, capacity);
            }
        }
        Err(Error::Response("cloud list exceeds pagination limit"))
    }
}
