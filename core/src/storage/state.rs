//! Account-scoped SQLite state, compatible with the recovered Python schema.
//! All read/modify/write workflows must use `transaction`, not separate calls.
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

// SQLite maintains this index on the original journal rows even for writes
// from clients that predate bounded receipt reads. Unknown/corrupt phases are
// included so they cannot silently unblock a new save.
const STYLE_TAG_UNRESOLVED: &str = "kind='style-tag-intents-v1' AND
    (CASE WHEN json_valid(value) THEN coalesce(json_extract(value, '$.phase'), '') ELSE '' END)
    NOT IN ('unchanged','confirmed','rejected','accepted_current_state')";
const DOWNLOAD_SONG_ID: &str =
    "CASE WHEN json_valid(value) THEN CAST(json_extract(value,'$.song_id') AS TEXT) END";
const DOWNLOAD_SONG_TASK: &str = "kind='download_tasks_v1' AND
    CASE WHEN json_valid(value) THEN coalesce(json_extract(value,'$.origin.kind'),'song')='song'
    ELSE 0 END";

const DOWNLOAD_VOICE_ID: &str =
    "CASE WHEN json_valid(value) THEN CAST(json_extract(value,'$.origin.voice_id') AS TEXT) END";
const DOWNLOAD_VOICE_TASK: &str = "kind='download_tasks_v1' AND
    CASE WHEN json_valid(value) THEN json_extract(value,'$.origin.kind')='voice' ELSE 0 END";
const CLOUD_UPLOAD_SOURCE_PATH: &str =
    "CASE WHEN json_valid(value) THEN json_extract(value,'$.source.path') END";
const INDEXED_FLAC_DOWNLOAD_CLAIMS: &str = "kind IN ('offline_music_v1','download_directory_v1',
    'download_tasks_v1','download_file_operations_v1')";

#[derive(Clone, Copy)]
pub(crate) enum PlaybackDownloadKind {
    Song,
    Voice,
}

pub(crate) struct PlaybackDownloadRow {
    pub id: String,
    pub status: String,
    pub path: Option<PathBuf>,
    pub moving: bool,
}

pub struct StateStore {
    directory: Option<PathBuf>,
    // Fields drop in order: close SQLite and release the shared native lease
    // before scheduling a cleanup that needs the exclusive lease.
    connection: Mutex<Connection>,
    _qr_candidate_lease: Option<fs::File>,
    after_close: AfterClose,
}

#[derive(Default)]
struct AfterClose(Option<Box<dyn FnOnce() + Send + Sync>>);

impl Drop for AfterClose {
    fn drop(&mut self) {
        if let Some(operation) = self.0.take() {
            operation();
        }
    }
}

pub struct StateTransaction<'a> {
    connection: &'a Connection,
}

#[derive(Debug, PartialEq)]
pub struct StateRow {
    pub owner: String,
    pub key: String,
    pub value: Value,
}

impl StateStore {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref().to_owned();
        let mut builder = super::private_directory_builder();
        builder.recursive(true);
        builder.create(&directory)?;
        let lease = super::QrCandidateStore::open_lease(&directory)?;
        let path = directory.join("core.sqlite3");
        let mut options = fs::OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        drop(file);
        let connection = Connection::open(&path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
        let mut store = Self::initialize(connection, Some(directory))?;
        store._qr_candidate_lease = lease;
        Ok(store)
    }

    /// Private candidate credentials must not survive a process restart.
    pub(crate) fn volatile_credentials() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA journal_mode=MEMORY;")?;
        Self::initialize(connection, None)
    }

    fn initialize(mut connection: Connection, directory: Option<PathBuf>) -> Result<Self> {
        match Self::initialize_schema(&mut connection, TransactionBehavior::Deferred) {
            Err(error) if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy) => {
                // A legacy database may need to upgrade a read transaction to
                // write. Retry with the writer lock first so SQLite can wait
                // for a competing opener instead of failing the lock upgrade.
                Self::initialize_schema(&mut connection, TransactionBehavior::Immediate)?;
            }
            result => result?,
        }
        Ok(Self {
            directory,
            connection: Mutex::new(connection),
            _qr_candidate_lease: None,
            after_close: AfterClose::default(),
        })
    }

    fn initialize_schema(
        connection: &mut Connection,
        behavior: TransactionBehavior,
    ) -> rusqlite::Result<()> {
        // The table and indexes become visible together, with one durable
        // commit on a fresh store. A complete schema needs no writer lock.
        let schema = connection.transaction_with_behavior(behavior)?;
        schema.execute_batch(
            "CREATE TABLE IF NOT EXISTS state (
               owner TEXT NOT NULL, kind TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
               PRIMARY KEY(owner,kind,key));",
        )?;
        schema.execute_batch(&format!(
            "CREATE INDEX IF NOT EXISTS style_tag_unresolved_v1 ON state(owner,key)
             WHERE {STYLE_TAG_UNRESOLVED};
             CREATE INDEX IF NOT EXISTS download_song_v1 ON state(owner,({DOWNLOAD_SONG_ID}))
             WHERE {DOWNLOAD_SONG_TASK};
             CREATE INDEX IF NOT EXISTS download_voice_v1 ON state(owner,({DOWNLOAD_VOICE_ID}))
             WHERE {DOWNLOAD_VOICE_TASK};
             CREATE INDEX IF NOT EXISTS indexed_flac_download_claims_v1 ON state(kind,key)
             WHERE {INDEXED_FLAC_DOWNLOAD_CLAIMS};
             CREATE INDEX IF NOT EXISTS cloud_upload_source_path_v1 ON state(owner,({CLOUD_UPLOAD_SOURCE_PATH}))
             WHERE kind='cloud_uploads_v1';"
        ))?;
        schema.commit()
    }

    pub(crate) fn on_qr_candidate_close(
        &mut self,
        operation: impl FnOnce() + Send + Sync + 'static,
    ) {
        self.after_close.0 = Some(Box::new(operation));
    }

    pub fn directory(&self) -> &Path {
        self.directory
            .as_deref()
            .expect("volatile credentials have no filesystem directory")
    }

    pub fn get(&self, owner: &str, kind: &str, key: &str) -> Result<Option<Value>> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        StateTransaction {
            connection: &connection,
        }
        .get(owner, kind, key)
    }

    /// Observe only the requested keys, in request order, in one read snapshot.
    pub(crate) fn get_ordered(
        &self,
        owner: &str,
        kind: &str,
        keys: &[String],
    ) -> Result<Vec<Option<Value>>> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        let mut statement = connection.prepare_cached(
            "SELECT state.value FROM json_each(?3) AS requested
             LEFT JOIN state ON state.owner=?1 AND state.kind=?2 AND state.key=requested.value
             ORDER BY requested.key",
        )?;
        let keys = serde_json::to_string(keys)?;
        let values = statement.query_map(params![owner, kind, keys], |row| {
            row.get::<_, Option<String>>(0)
        })?;
        values
            .map(|value| {
                value?
                    .map(|value| serde_json::from_str(&value).map_err(Error::from))
                    .transpose()
            })
            .collect()
    }

    pub(crate) fn playback_download_rows(
        &self,
        owner: &str,
        id: &str,
        kind: PlaybackDownloadKind,
    ) -> Result<Vec<PlaybackDownloadRow>> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        StateTransaction {
            connection: &connection,
        }
        .playback_download_rows(owner, id, kind)
    }

    /// Read the exact completed song publication, if unique. The song
    /// expression index avoids materializing another user's or track's tasks.
    pub(crate) fn completed_song_publication(
        &self,
        owner: &str,
        song: &str,
    ) -> Result<Option<(String, Value)>> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        let mut query = connection.prepare_cached(&format!(
            "SELECT key, value FROM state INDEXED BY download_song_v1
             WHERE owner=? AND {DOWNLOAD_SONG_TASK} AND ({DOWNLOAD_SONG_ID})=?
               AND json_extract(value,'$.status')='complete' LIMIT 2"
        ))?;
        let mut rows = query.query([owner, song])?;
        let Some(first) = rows.next()? else {
            return Ok(None);
        };
        let (key, value): (String, String) = (first.get(0)?, first.get(1)?);
        if rows.next()?.is_some() {
            return Ok(None);
        }
        Ok(Some((key, serde_json::from_str(&value)?)))
    }

    /// Whether this exact ordinary source is represented by a visible task in
    /// the current owner's cloud journal. No tickets or transfer data leave
    /// SQLite. A hash is required by the caller before reporting a duplicate.
    pub(crate) fn visible_cloud_upload_source(
        &self,
        owner: &str,
        path: &str,
        bytes: u64,
        sha256: Option<&str>,
    ) -> Result<bool> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        let mut query = connection.prepare_cached(&format!(
            "SELECT EXISTS(
                 SELECT 1 FROM state AS upload INDEXED BY cloud_upload_source_path_v1
                 WHERE owner=?1 AND kind='cloud_uploads_v1' AND ({CLOUD_UPLOAD_SOURCE_PATH})=?2
                 AND CASE WHEN json_valid(value) THEN
                   json_extract(value,'$.id')=key
                   AND json_extract(value,'$.phase')<>'cancelled'
                   AND json_extract(value,'$.source.bytes')=?3
                   AND (?4 IS NULL OR json_extract(value,'$.source.sha256')=?4)
                   ELSE 0 END
                 AND NOT EXISTS (
                     SELECT 1 FROM state AS schedule WHERE owner=upload.owner
                     AND kind='cloud_schedule_v1' AND key=upload.key
                     AND CASE WHEN json_valid(value) THEN
                         json_extract(value,'$.id') IS NOT key
                         OR coalesce(json_extract(value,'$.hidden'),0)<>0
                         OR coalesce(json_extract(value,'$.removing'),0)<>0
                       ELSE 1 END
                 )
             )"
        ))?;
        let bytes = i64::try_from(bytes)
            .map_err(|_| Error::InvalidInput("invalid recorded cloud upload length"))?;
        Ok(query.query_row(params![owner, path, bytes, sha256], |row| row.get(0))?)
    }

    /// Boolean exclusion only: an ordinary indexed source must not take a
    /// file owned by any download account, including when the player is a
    /// guest. Inputs are an exact canonical index path and its offline key.
    pub(crate) fn download_claims_indexed_path(&self, key: &str, path: &str) -> Result<bool> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        // Use exact index seeks for owner-independent keys and avoid scanning
        // unrelated state namespaces when a track has no claimed path.
        let mut query = connection.prepare_cached(&format!(
            "SELECT EXISTS(
                SELECT 1 FROM state
                    WHERE {INDEXED_FLAC_DOWNLOAD_CLAIMS} AND kind='offline_music_v1' AND key=?1
                UNION ALL
                SELECT 1 FROM state
                    WHERE {INDEXED_FLAC_DOWNLOAD_CLAIMS} AND kind='download_directory_v1' AND key='active'
                UNION ALL
                SELECT 1 FROM state
                    WHERE {INDEXED_FLAC_DOWNLOAD_CLAIMS} AND kind='download_directory_v1' AND key='settings'
                    AND CASE WHEN json_valid(value) THEN
                      (json_extract(value,'$.root')='/' OR
                       substr(?2,1,length(json_extract(value,'$.root'))+1)=
                           json_extract(value,'$.root')||'/')
                    ELSE 1 END
                UNION ALL
                SELECT 1 FROM state
                    WHERE {INDEXED_FLAC_DOWNLOAD_CLAIMS} AND kind='download_tasks_v1'
                    AND CASE WHEN json_valid(value) THEN
                      (json_extract(value,'$.target')=?2 OR
                       json_extract(value,'$.destination.path')=?2 OR
                       (json_extract(value,'$.destination.kind')='named' AND
                        (json_extract(value,'$.root')='/' OR
                         substr(?2,1,length(json_extract(value,'$.root'))+1)=
                             json_extract(value,'$.root')||'/')))
                    ELSE 1 END
                UNION ALL
                SELECT 1 FROM state
                    WHERE {INDEXED_FLAC_DOWNLOAD_CLAIMS} AND kind='download_file_operations_v1'
                    AND CASE WHEN json_valid(value) THEN
                      (json_extract(value,'$.phase') IS NULL OR
                       json_extract(value,'$.phase') NOT IN ('complete','cancelled'))
                    ELSE 1 END
            )"
        ))?;
        Ok(query.query_row([key, path], |row| row.get(0))?)
    }

    /// Includes commits from other connections and mutations made through this
    /// connection. Capture inside a read/write transaction when binding it to
    /// cached content; a post-commit sample could conceal a competing write.
    pub(crate) fn change_stamp(&self) -> Result<(i64, i64)> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        StateTransaction {
            connection: &connection,
        }
        .change_stamp()
    }

    pub fn put(&self, owner: &str, kind: &str, key: &str, value: &Value) -> Result<()> {
        self.transaction(|tx| tx.put(owner, kind, key, value))
    }

    pub fn delete(&self, owner: &str, kind: &str, key: &str) -> Result<()> {
        self.transaction(|tx| tx.delete(owner, kind, key))
    }

    pub fn values(&self, owner: &str, kind: &str) -> Result<BTreeMap<String, Value>> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        StateTransaction {
            connection: &connection,
        }
        .values(owner, kind)
    }

    pub fn clear_owner(&self, owner: &str) -> Result<()> {
        self.transaction(|tx| {
            tx.connection
                .execute("DELETE FROM state WHERE owner=?", [owner])?;
            Ok(())
        })
    }

    /// Read several keys from one committed snapshot without reserving SQLite's
    /// write lock. The closure receives only the read methods of StateTransaction.
    pub(crate) fn read_snapshot<T>(
        &self,
        operation: impl FnOnce(&StateTransaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let value = operation(&StateTransaction { connection: &tx })?;
        tx.commit()?;
        Ok(value)
    }

    /// The closure must only access the supplied transaction. The connection
    /// lock and BEGIN IMMEDIATE cover the complete operation across writers.
    pub fn transaction<T>(
        &self,
        operation: impl FnOnce(&mut StateTransaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = operation(&mut StateTransaction { connection: &tx })?;
        tx.commit()?;
        Ok(value)
    }
}

impl StateTransaction<'_> {
    pub(crate) fn playback_download_rows(
        &self,
        owner: &str,
        id: &str,
        kind: PlaybackDownloadKind,
    ) -> Result<Vec<PlaybackDownloadRow>> {
        let (index, task, identity) = match kind {
            PlaybackDownloadKind::Song => {
                ("download_song_v1", DOWNLOAD_SONG_TASK, DOWNLOAD_SONG_ID)
            }
            PlaybackDownloadKind::Voice => {
                ("download_voice_v1", DOWNLOAD_VOICE_TASK, DOWNLOAD_VOICE_ID)
            }
        };
        let mut query = self.connection.prepare_cached(&format!(
            "SELECT key, json_extract(value,'$.status'), json_extract(value,'$.target'),
                    json_extract(value,'$.file_operation') IS NOT NULL
             FROM state INDEXED BY {index}
             WHERE owner=? AND {task} AND ({identity})=?"
        ))?;
        let rows = query.query_map([owner, id], |row| {
            Ok(PlaybackDownloadRow {
                id: row.get(0)?,
                status: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                path: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
                moving: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn unresolved_style_tag(&self, owner: &str) -> Result<Option<(String, Value)>> {
        let mut statement = self.connection.prepare_cached(&format!(
            "SELECT key,value FROM state INDEXED BY style_tag_unresolved_v1
             WHERE owner=? AND {STYLE_TAG_UNRESOLVED} ORDER BY key LIMIT 2"
        ))?;
        let mut rows = statement.query([owner])?;
        let first = rows
            .next()?
            .map(|row| Ok::<_, Error>((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .transpose()?;
        if rows.next()?.is_some() {
            return Err(Error::State("multiple unresolved style tag intents"));
        }
        first
            .map(|(key, value)| Ok((key, serde_json::from_str(&value)?)))
            .transpose()
    }

    pub(crate) fn change_stamp(&self) -> Result<(i64, i64)> {
        Ok(self.connection.query_row(
            "SELECT data_version,total_changes() FROM pragma_data_version",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }
    /// Keyset pagination keeps migration work bounded even for large legacy
    /// namespaces. Keys inserted before the cursor must be indexed by writers.
    pub fn values_page(
        &self,
        owner: &str,
        kind: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<BTreeMap<String, Value>> {
        if !(1..=1024).contains(&limit) {
            return Err(Error::InvalidInput("invalid state page size"));
        }
        // Keep the key comparison explicit so SQLite can seek the compound
        // primary key instead of scanning earlier rows for an OR predicate.
        let sql = if after.is_some() {
            "SELECT key,value FROM state WHERE owner=? AND kind=? AND key>? ORDER BY key LIMIT ?"
        } else {
            "SELECT key,value FROM state WHERE owner=? AND kind=? AND key>=? ORDER BY key LIMIT ?"
        };
        let mut statement = self.connection.prepare(sql)?;
        let records = statement.query_map(
            params![owner, kind, after.unwrap_or(""), limit as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        records
            .map(|row| {
                let (key, raw) = row?;
                Ok((key, serde_json::from_str(&raw)?))
            })
            .collect()
    }
    pub fn values(&self, owner: &str, kind: &str) -> Result<BTreeMap<String, Value>> {
        let mut statement = self
            .connection
            .prepare("SELECT key,value FROM state WHERE owner=? AND kind=? ORDER BY key")?;
        let records = statement.query_map(params![owner, kind], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        records
            .map(|row| {
                let (key, value) = row?;
                Ok((key, serde_json::from_str(&value)?))
            })
            .collect()
    }
    pub fn get(&self, owner: &str, kind: &str, key: &str) -> Result<Option<Value>> {
        self.get_raw(owner, kind, key)?
            .map(|value| serde_json::from_str(&value).map_err(Error::from))
            .transpose()
    }

    /// Preserve malformed JSON during explicit recovery. Only available inside
    /// a transaction, so backing up and replacing the record remain atomic.
    pub fn get_raw(&self, owner: &str, kind: &str, key: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM state WHERE owner=? AND kind=? AND key=?",
                params![owner, kind, key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn put(&mut self, owner: &str, kind: &str, key: &str, value: &Value) -> Result<()> {
        let json = serde_json::to_string(value)?;
        self.connection.execute(
            "INSERT INTO state VALUES(?,?,?,?) ON CONFLICT(owner,kind,key) DO UPDATE SET value=excluded.value",
            params![owner, kind, key, json])?;
        Ok(())
    }

    pub fn delete(&mut self, owner: &str, kind: &str, key: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM state WHERE owner=? AND kind=? AND key=?",
            params![owner, kind, key],
        )?;
        Ok(())
    }

    pub fn rows(&self, kind: &str) -> Result<Vec<StateRow>> {
        let mut statement = self
            .connection
            .prepare("SELECT owner,key,value FROM state WHERE kind=? ORDER BY owner,key")?;
        let records = statement.query_map([kind], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        records
            .map(|row| {
                let (owner, key, value) = row?;
                Ok(StateRow {
                    owner,
                    key,
                    value: serde_json::from_str(&value)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod indexed_flac_exclusion {
    use super::StateStore;
    use serde_json::json;

    #[test]
    fn native_local_admission_cannot_adopt_another_owners_download_paths() {
        let root = tempfile::tempdir().unwrap();
        let store = StateStore::open(root.path()).unwrap();
        assert!(
            !store
                .download_claims_indexed_path("id", "/music/song.flac")
                .unwrap()
        );
        store
            .put("other-account", "offline_music_v1", "id", &json!({}))
            .unwrap();
        assert!(
            store
                .download_claims_indexed_path("id", "/music/song.flac")
                .unwrap()
        );
        assert!(
            !store
                .download_claims_indexed_path("different-id", "/music/own.flac")
                .unwrap()
        );
        store.put("other-account", "download_tasks_v1", "task", &json!({
            "target":"/music/direct.flac","destination":{"kind":"path","path":"/music/requested.flac"}
        })).unwrap();
        for path in ["/music/direct.flac", "/music/requested.flac"] {
            assert!(
                store
                    .download_claims_indexed_path("other-id", path)
                    .unwrap()
            );
        }
        store
            .put(
                "other-account",
                "download_directory_v1",
                "settings",
                &json!({"root":"/music/managed"}),
            )
            .unwrap();
        assert!(
            store
                .download_claims_indexed_path("other-id", "/music/managed/sub/song.flac")
                .unwrap()
        );
        assert!(
            !store
                .download_claims_indexed_path("other-id", "/music/managed-elsewhere/song.flac")
                .unwrap()
        );
        store
            .put(
                "other-account",
                "download_file_operations_v1",
                "pending",
                &json!({"phase":"prepared"}),
            )
            .unwrap();
        assert!(
            store
                .download_claims_indexed_path("other-id", "/music/unrelated.flac")
                .unwrap()
        );
        store
            .put(
                "other-account",
                "download_file_operations_v1",
                "pending",
                &json!({"phase":"complete"}),
            )
            .unwrap();
        assert!(
            !store
                .download_claims_indexed_path("other-id", "/music/unrelated.flac")
                .unwrap()
        );
        store
            .transaction(|tx| {
                tx.connection.execute(
                    "INSERT INTO state VALUES('other-account','download_tasks_v1','corrupt','{')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(
            store
                .download_claims_indexed_path("other-id", "/music/unrelated.flac")
                .unwrap()
        );
    }
}
