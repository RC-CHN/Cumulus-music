//! Finish already-committed file cleanup in an explicitly selected local store.
//! Never starts a new move/delete intent or operates on a directory by scanning it.
use cumulus_core::{
    Error, Result,
    download::completed::{CompletedDownloads, FilePhase},
    storage::StateStore,
};
use std::{path::PathBuf, sync::Arc};
fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 1 {
        return Err(Error::InvalidInput(
            "usage: finish_file_cleanup EXISTING_STATE_DIRECTORY",
        ));
    }
    let root = PathBuf::from(&args[0]);
    if !root.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing state required"));
    }
    let store = Arc::new(StateStore::open(&root)?);
    let rows = store.transaction(|tx| tx.rows("download_file_operations_v1"))?;
    let mut completed = 0;
    let mut retained = 0;
    for row in rows {
        let files = CompletedDownloads::offline(store.clone(), &row.owner)?;
        let before = files.get(&row.key)?;
        if before.phase != FilePhase::Cleanup {
            continue;
        }
        let after = files.resume(&row.key)?;
        if after.phase != FilePhase::Complete {
            return Err(Error::State("file cleanup unfinished"));
        }
        completed += 1;
        retained += after
            .files
            .iter()
            .filter(|f| f.replacement_retained)
            .count();
    }
    println!(
        "{}",
        serde_json::json!({"committed_cleanups_finished":completed,"replacements_retained":retained})
    );
    Ok(())
}
