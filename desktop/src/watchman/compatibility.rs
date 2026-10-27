use cumulus_core::{Result, identity::WindowsBrowserIdentity};

/// An expression returning a collector-scoped context; contains no hardware IDs
/// or credentials. Global WebView identity is deliberately not patched by this.
pub fn context_script(identity: &WindowsBrowserIdentity) -> Result<String> {
    Ok(format!(
        "({})({})",
        include_str!("compatibility.js"),
        serde_json::to_string(identity)?
    ))
}
