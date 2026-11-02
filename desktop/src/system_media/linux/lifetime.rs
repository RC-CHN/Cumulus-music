//! A torn concurrent snapshot is not the retirement of an account's media bus.
pub(super) fn retain_read_error(
    error: &cumulus_core::Error,
    active_epoch: Option<&str>,
    bus_epoch: Option<&str>,
) -> bool {
    matches!(error, cumulus_core::Error::StaleResponse)
        && active_epoch.is_some()
        && active_epoch == bus_epoch
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_torn_snapshot_keeps_only_the_current_accounts_registered_bus() {
        let stale = cumulus_core::Error::StaleResponse;
        assert!(retain_read_error(&stale, Some("1"), Some("1")));
        for (active, bus) in [
            (Some("2"), Some("1")),
            (None, Some("1")),
            (Some("1"), None),
            (None, None),
        ] {
            assert!(!retain_read_error(&stale, active, bus));
        }
    }
    #[test]
    fn unavailable_or_failed_reads_are_not_treated_as_transport_transitions() {
        for error in [
            cumulus_core::Error::State("unavailable"),
            cumulus_core::Error::Cancelled,
            cumulus_core::Error::QueueChanged,
        ] {
            assert!(!retain_read_error(&error, Some("1"), Some("1")));
        }
    }
}
