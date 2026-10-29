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

pub struct StateStore {
    directory: PathBuf,
    connection: Mutex<Connection>,
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
        connection.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS state (
               owner TEXT NOT NULL, kind TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
               PRIMARY KEY(owner,kind,key));",
        )?;
        Ok(Self {
            directory,
            connection: Mutex::new(connection),
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn get(&self, owner: &str, kind: &str, key: &str) -> Result<Option<Value>> {
        let connection = self.connection.lock().map_err(|_| Error::LockPoisoned)?;
        StateTransaction {
            connection: &connection,
        }
        .get(owner, kind, key)
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
