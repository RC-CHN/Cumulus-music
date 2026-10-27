use serde_json::Value;
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Internal errors are deliberately not serializable. Challenge bodies and
/// transport details may contain credentials; hosts must map public errors.
#[derive(thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Shared(std::sync::Arc<Error>),
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),
    #[error("invalid response: {0}")]
    Response(&'static str),
    #[error("request failed login validation while the session is retained")]
    LoginDeferred,
    #[error("response belongs to a previous operation or account generation")]
    StaleResponse,
    #[error("operation was cancelled")]
    Cancelled,
    #[error("queue changed in another host; reload before retrying")]
    QueueChanged,
    #[error("this resource already has an operation in progress")]
    MutationBusy,
    #[error("previous write is unresolved; only read-only reconciliation is allowed")]
    MutationUncertain,
    #[error("credential profile changed in another host; reopen before continuing")]
    CredentialChanged,
    #[error("operation is not available in the current state: {0}")]
    State(&'static str),
    #[error("service rejected {endpoint}")]
    Service {
        endpoint: String,
        code: Value,
        body: Box<Value>,
    },
    #[error("transport request failed")]
    Transport(String),
    /// This music request never reached its transport. SDK/config requests
    /// may have happened, but the remote business mutation was not attempted.
    #[error("request preparation failed before sending: {0}")]
    RequestNotSent(Box<Error>),
    #[error("HTTP request failed with status {status}")]
    HttpStatus {
        status: u16,
        /// A normalized music API path, never an upload URL or credentials.
        endpoint: Option<String>,
        body: Option<Box<Value>>,
    },
    #[error("playlist metadata batch is incomplete")]
    Batch {
        responses: Value,
        failed: Vec<String>,
    },
    #[error("persistent storage failed")]
    Storage(#[from] rusqlite::Error),
    #[error("stored JSON is invalid")]
    Json(#[from] serde_json::Error),
    #[error("local file operation failed")]
    Io(#[from] std::io::Error),
    #[error("state lock is poisoned")]
    LockPoisoned,
    #[error("invalid protocol data: {0}")]
    Protocol(&'static str),
    #[error("invalid media data: {0}")]
    Media(&'static str),
    #[error("audio engine {operation} failed ({code})")]
    Playback { operation: &'static str, code: i32 },
    #[error("operating system random source is unavailable")]
    Entropy,
}

impl Error {
    pub fn service(endpoint: &str, code: Value, body: Value) -> Self {
        Self::Service {
            endpoint: endpoint.into(),
            code,
            body: Box::new(body),
        }
    }

    pub fn is_service_code(&self, expected: i64) -> bool {
        match self.root() {
            Self::Service { code, .. } => code_matches(code, expected),
            Self::HttpStatus {
                body: Some(body), ..
            } => code_matches(&body["code"], expected),
            _ => false,
        }
    }

    /// Shared read operations retain the original typed error for every
    /// subscriber; no loss of 301/rights failure classification.
    pub fn root(&self) -> &Self {
        let mut value = self;
        while let Self::Shared(inner) = value {
            value = inner;
        }
        value
    }
}

pub(crate) fn code_matches(value: &Value, expected: i64) -> bool {
    value.as_i64() == Some(expected) || value.as_str() == Some(expected.to_string().as_str())
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print raw response bodies or request/transport credentials.
        fmt::Display::fmt(self, formatter)
    }
}
