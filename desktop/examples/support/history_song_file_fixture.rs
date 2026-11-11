//! Native history and library identities for a closed file-manager probe.
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    history::{HistoryKind, HistoryRecord, HistoryService},
    library::LocalLibrary,
    storage::StateStore,
};
use serde_json::json;
use std::{fs, path::Path, sync::Arc};

const SAMPLE: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.mp3");
const TRACKS: [(&str, Option<&str>); 5] = [
    ("历史原始音频.mp3", None),
    ("匹配原始音频.mp3", Some("6")),
    ("匹配但下载优先.mp3", Some("2")),
    ("歧义甲.mp3", Some("1")),
    ("歧义乙.mp3", Some("1")),
];

pub fn seed(session: Arc<AccountSession>, store: Arc<StateStore>, root: &Path) -> Result<String> {
    let token = session.token()?;
    if token.uid.as_deref() != Some("2") {
        return Err(Error::State("unexpected history file proof account"));
    }
    let source = root.join("reveal-library");
    fs::create_dir(&source)?;
    for (name, _) in TRACKS {
        fs::write(source.join(name), SAMPLE)?;
    }
    let library = LocalLibrary::new(store)?;
    library.scan(&source)?;
    let files = library.list(Default::default())?;
    let mut history_id = None;
    for (name, mapping) in TRACKS {
        let indexed = files
            .iter()
            .find(|item| item.path.file_name().and_then(|name| name.to_str()) == Some(name))
            .ok_or(Error::State("missing synthetic history file"))?;
        if let Some(song) = mapping {
            library.map_online(&session, &indexed.id, &json!(song))?;
        } else {
            let history = HistoryService::new(session.clone())?;
            let mut metadata = indexed.metadata.clone();
            metadata.title = "历史原始音频".into();
            let record = HistoryRecord::local(
                HistoryKind::Song,
                json!({"local_id":indexed.id,"local_revision":indexed.revision,
                       "metadata":metadata}),
                1793180800000,
            )?;
            history_id = Some(record.id.clone());
            history.record_for(&token, HistoryKind::Song, record)?;
        }
    }
    history_id.ok_or(Error::State("missing synthetic history identity"))
}
