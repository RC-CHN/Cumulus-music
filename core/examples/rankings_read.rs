//! Read-only live acceptance of native ranking use cases, in an isolated store.
use cumulus_core::client::{
    MusicClient,
    dto::{RankingPageView, RankingQuery},
};
use serde_json::json;
use std::{fs, path::PathBuf};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let profile = PathBuf::from(args.next().ok_or("existing PC profile required")?);
    let root = PathBuf::from(args.next().ok_or("new output directory required")?);
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root)?;
    let client = MusicClient::open(&root.join("state"), Some(&profile))?;
    let epoch = client.restore_account().await?.epoch;
    let mut report = json!({"implementation":"rust","explicit_remote_writes":0,"checks":{}});
    let mut album = None;
    for (name, query) in [
        ("home", RankingQuery::Home),
        (
            "album_sales",
            RankingQuery::AlbumSales {
                year: time::OffsetDateTime::now_utc().year(),
            },
        ),
        ("album_2018", RankingQuery::AlbumYear { year: 2018 }),
        ("song_sales", RankingQuery::SongSales),
    ] {
        let value = client.ranking_page(&epoch, query.clone(), true).await?;
        fs::write(
            root.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&value)?,
        )?;
        let count = match &value.value.page {
            RankingPageView::Home { groups } => groups.iter().map(|g| g.cards.len()).sum(),
            RankingPageView::AlbumSales { boards, .. } => {
                if album.is_none() {
                    album = boards
                        .iter()
                        .flat_map(|b| b.products.iter())
                        .next()
                        .map(|p| p.id.clone());
                }
                boards.len()
            }
            RankingPageView::SongSales { songs, .. } => songs.len(),
            _ => 0,
        };
        let cached = client.ranking_page(&epoch, query, false).await?;
        assert!(cached.value.cached);
        assert_eq!(
            serde_json::to_value(&value.value.page)?,
            serde_json::to_value(&cached.value.page)?
        );
        report["checks"][name] = json!({"ok":true,"count":count,"cache_equal":true});
    }
    let id = album.ok_or("no album sample")?;
    let value = client
        .ranking_page(&epoch, RankingQuery::Album { id }, true)
        .await?;
    let RankingPageView::Album { tracks, .. } = &value.value.page else {
        return Err("wrong album projection".into());
    };
    report["checks"]["album"] = json!({"ok":true,"tracks":tracks.len()});
    fs::write(root.join("album.json"), serde_json::to_vec_pretty(&value)?)?;
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
