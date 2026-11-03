use super::{MusicClient, dto::AccountData};
use crate::{Error, Result};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Serialize)]
pub struct StyleTag {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct StyleCategory {
    pub id: String,
    pub name: String,
    pub tags: Vec<StyleTag>,
}

fn field(row: &Value, id: &str, name: &str) -> Result<(String, String)> {
    let value = &row[id];
    let id = if let Some(id) = value.as_str() {
        id.to_owned()
    } else if let Some(id) = value.as_u64().filter(|id| *id <= 9_007_199_254_740_991) {
        id.to_string()
    } else {
        return Err(Error::Response("invalid style tag identity"));
    };
    if id.is_empty() || id.len() > 80 || id.chars().any(char::is_control) {
        return Err(Error::Response("invalid style tag identity"));
    }
    let name = row[name]
        .as_str()
        .map(str::trim)
        .filter(|name| {
            !name.is_empty() && name.chars().count() <= 80 && !name.chars().any(char::is_control)
        })
        .ok_or(Error::Response("invalid style tag label"))?;
    Ok((id, name.to_owned()))
}

fn rows(value: &Value, max: usize) -> Result<&[Value]> {
    if value.is_null() {
        return Ok(&[]);
    }
    value
        .as_array()
        .filter(|rows| rows.len() <= max)
        .map(Vec::as_slice)
        .ok_or(Error::Response("invalid style tag list"))
}

impl MusicClient {
    pub async fn style_recommendation_categories(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<StyleCategory>>> {
        let token = self.player_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let response = api
                    .call(
                        "/api/homepage/daily/song/config/get",
                        json!({"limit":20}),
                        false,
                        &[200],
                    )
                    .await?;
                if !response["data"].is_object() {
                    return Err(Error::Response("invalid style tag configuration"));
                }
                let mut categories = HashSet::new();
                rows(&response["data"]["categorys"], 64)?
                    .iter()
                    .map(|category| {
                        let (id, name) = field(category, "categoryId", "categoryName")?;
                        if !categories.insert(id.clone()) {
                            return Err(Error::Response("duplicate style category"));
                        }
                        let mut tag_ids = HashSet::new();
                        let tags = rows(&category["tagVOList"], 256)?
                            .iter()
                            .map(|tag| {
                                let (id, name) = field(tag, "tagId", "tagName")?;
                                if !tag_ids.insert(id.clone()) {
                                    return Err(Error::Response("duplicate style tag"));
                                }
                                Ok(StyleTag { id, name })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        Ok(StyleCategory { id, name, tags })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
