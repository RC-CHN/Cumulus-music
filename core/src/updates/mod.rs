//! Read-only Cumulus distribution metadata. No account or updater integration.
mod github;
mod package;

pub use github::GitHubReleases;
pub use package::{PackageFailure, PackageFormat, PackageVerification, PublishedPackages};
use serde::{Deserialize, Serialize};
use std::{fmt, future::Future, pin::Pin};

pub const LATEST_ENDPOINT: &str =
    "https://api.github.com/repos/RC-CHN/Cumulus-music/releases/latest";
const RELEASE_PAGE: &str = "https://github.com/RC-CHN/Cumulus-music/releases/tag/";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Version(u64, u64, u64);

impl Version {
    fn parse(raw: &str) -> Option<Self> {
        let mut parts = raw.split('.');
        let mut part = || {
            let value = parts.next()?;
            (!value.is_empty()
                && (value == "0" || !value.starts_with('0'))
                && value.bytes().all(|c| c.is_ascii_digit()))
            .then(|| value.parse().ok())
            .flatten()
        };
        let version = Self(part()?, part()?, part()?);
        parts.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateFailure {
    Network,
    RateLimited,
    Unavailable,
    InvalidMetadata,
    Busy,
    NoOffer,
    Browser,
}

/// Native-only validated release identity. The renderer never supplies a URL.
#[derive(Clone, Copy)]
pub struct ReleasePage(Version);

impl ReleasePage {
    pub fn url(self) -> String {
        format!("{RELEASE_PAGE}v{}", self.0)
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum UpdateCheck {
    Available {
        current_version: String,
        latest_version: String,
    },
    NoNewer {
        current_version: String,
        latest_version: String,
    },
    NoPublishedRelease {
        current_version: String,
    },
}

pub struct UpdateResult {
    pub view: UpdateCheck,
    pub release: Option<ReleasePage>,
    pub packages: Option<PublishedPackages>,
}

pub enum LatestReleaseReply {
    Published(Vec<u8>),
    NotFound,
}

pub type ReleaseFuture<'a> =
    Pin<Box<dyn Future<Output = Result<LatestReleaseReply, UpdateFailure>> + Send + 'a>>;

pub trait LatestReleaseSource: Send + Sync {
    fn latest(&self) -> ReleaseFuture<'_>;
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
}

fn published_repo_page(raw: &str, version: Version) -> bool {
    let Ok(parsed) = url::Url::parse(raw) else {
        return false;
    };
    let expected = format!("v{version}");
    let segments = parsed.path().split('/').collect::<Vec<_>>();
    parsed.scheme() == "https"
        && parsed.host_str() == Some("github.com")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && matches!(segments.as_slice(), ["", owner, repo, "releases", "tag", tag]
            if owner.eq_ignore_ascii_case("RC-CHN") && repo.eq_ignore_ascii_case("Cumulus-music") && *tag == expected)
}

pub async fn check_latest(
    current: &str,
    source: &dyn LatestReleaseSource,
) -> Result<UpdateResult, UpdateFailure> {
    let (base, is_candidate) = match current.split_once("-rc.") {
        Some((base, rc))
            if !rc.starts_with('0')
                && rc.bytes().all(|digit| digit.is_ascii_digit())
                && rc.parse::<u64>().is_ok() =>
        {
            (base, true)
        }
        Some(_) => return Err(UpdateFailure::InvalidMetadata),
        None => (current, false),
    };
    let installed = Version::parse(base).ok_or(UpdateFailure::InvalidMetadata)?;
    let current_version = current.to_owned();
    let LatestReleaseReply::Published(body) = source.latest().await? else {
        return Ok(UpdateResult {
            view: UpdateCheck::NoPublishedRelease { current_version },
            release: None,
            packages: None,
        });
    };
    if body.len() > github::MAX_REPLY {
        return Err(UpdateFailure::InvalidMetadata);
    }
    let release: Release =
        serde_json::from_slice(&body).map_err(|_| UpdateFailure::InvalidMetadata)?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .and_then(Version::parse)
        .ok_or(UpdateFailure::InvalidMetadata)?;
    if release.draft
        || release.prerelease
        || release
            .published_at
            .as_deref()
            .is_none_or(|date| chrono::DateTime::parse_from_rfc3339(date).is_err())
        || !published_repo_page(&release.html_url, version)
    {
        return Err(UpdateFailure::InvalidMetadata);
    }
    let packages = PublishedPackages::from_assets(&release.assets, version)
        .ok_or(UpdateFailure::InvalidMetadata)?;
    let latest_version = version.to_string();
    if version > installed || (is_candidate && version == installed) {
        Ok(UpdateResult {
            view: UpdateCheck::Available {
                current_version,
                latest_version,
            },
            release: Some(ReleasePage(version)),
            packages: Some(packages),
        })
    } else {
        Ok(UpdateResult {
            view: UpdateCheck::NoNewer {
                current_version,
                latest_version,
            },
            release: None,
            packages: Some(packages),
        })
    }
}
