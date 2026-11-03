use super::super::SelfProfileView;
use crate::{
    Error, Result,
    protocol::{api::official_text_length, wire::serialization::trim_js},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use time::{Date, Month, OffsetDateTime, UtcOffset};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfilePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub birthday: Option<ProfileBirthday>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<ProfileRegion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileBirthday {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    /// UTC offset on the selected calendar day, positive east of UTC.
    pub utc_offset_minutes: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRegion {
    pub province: u32,
    pub city: u32,
}

impl ProfileBirthday {
    fn local(self) -> Result<(Date, UtcOffset)> {
        let invalid = || Error::InvalidInput("invalid profile birthday");
        if !(1920..=9999).contains(&self.year) || self.utc_offset_minutes.unsigned_abs() > 14 * 60 {
            return Err(invalid());
        }
        let month = Month::try_from(self.month).map_err(|_| invalid())?;
        let date = Date::from_calendar_date(self.year, month, self.day).map_err(|_| invalid())?;
        let offset = UtcOffset::from_whole_seconds(i32::from(self.utc_offset_minutes) * 60)
            .map_err(|_| invalid())?;
        Ok((date, offset))
    }

    pub(super) fn milliseconds(self) -> Result<i64> {
        let (date, offset) = self.local()?;
        let millis = date.midnight().assume_offset(offset).unix_timestamp() * 1000;
        if millis == 0 {
            return Err(Error::InvalidInput(
                "profile birthday encodes an unset value",
            ));
        }
        Ok(millis)
    }

    fn validate_today(self) -> Result<()> {
        let (date, offset) = self.local()?;
        if date > OffsetDateTime::now_utc().to_offset(offset).date() {
            return Err(Error::InvalidInput("profile birthday is in the future"));
        }
        Ok(())
    }
}

impl ProfilePatch {
    pub(super) fn normalized(mut self) -> Result<Self> {
        if let Some(name) = self.nickname.as_mut() {
            let trimmed = trim_js(name);
            if !(4..=30).contains(&official_text_length(trimmed))
                || !trimmed.chars().all(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(c, '-' | '_')
                        || ('\u{4e00}'..='\u{9fa5}').contains(&c)
                })
            {
                return Err(Error::InvalidInput("invalid profile nickname"));
            }
            *name = trimmed.to_owned();
        }
        if self.signature.as_ref().is_some_and(|text| {
            official_text_length(text) > 300 || text.chars().any(|c| c.is_control() && c != '\n')
        }) {
            return Err(Error::InvalidInput("invalid profile signature"));
        }
        if self.gender.is_some_and(|gender| !matches!(gender, 1 | 2)) {
            return Err(Error::InvalidInput("unsupported profile gender"));
        }
        if let Some(birthday) = self.birthday {
            birthday.milliseconds()?;
        }
        if let Some(region) = self.region
            && (region.province == 0 || region.city == 0)
        {
            return Err(Error::InvalidInput(
                "unknown profile province and city pair",
            ));
        }
        Ok(self)
    }

    pub(super) fn validate_today(&self) -> Result<()> {
        if let Some(birthday) = self.birthday {
            birthday.validate_today()?;
        }
        Ok(())
    }

    pub(super) fn changed_from(&self, profile: &SelfProfileView) -> Result<Self> {
        let mut patch = self.clone();
        if patch.nickname == profile.nickname {
            patch.nickname = None;
        }
        if patch.signature == profile.signature {
            patch.signature = None;
        }
        if patch.gender == profile.gender {
            patch.gender = None;
        }
        if let Some(birthday) = patch.birthday
            && Some(birthday.milliseconds()?) == profile.birthday
        {
            patch.birthday = None;
        }
        if let Some(region) = patch.region
            && Some(region.province) == profile.province
            && Some(region.city) == profile.city
        {
            patch.region = None;
        }
        Ok(patch)
    }

    pub(super) fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub(super) fn wire(&self) -> Result<Value> {
        let mut patch = Map::new();
        if let Some(name) = &self.nickname {
            patch.insert("nickname".into(), json!(name));
        }
        if let Some(signature) = &self.signature {
            patch.insert("signature".into(), json!(signature));
        }
        if let Some(gender) = self.gender {
            patch.insert("gender".into(), json!(gender));
        }
        if let Some(birthday) = self.birthday {
            patch.insert("birthday".into(), json!(birthday.milliseconds()?));
        }
        if let Some(region) = self.region {
            patch.insert("province".into(), json!(region.province));
            patch.insert("city".into(), json!(region.city));
        }
        Ok(Value::Object(patch))
    }
}
