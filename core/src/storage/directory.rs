use std::fs::DirBuilder;

/// Non-recursive builder for state and temporary work directories.
/// Unix requests owner-only access; Windows inherits the parent directory ACL.
/// Callers still validate existing paths and reject unexpected redirects.
pub fn private_directory_builder() -> DirBuilder {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        builder
    }
    #[cfg(not(unix))]
    {
        DirBuilder::new()
    }
}
