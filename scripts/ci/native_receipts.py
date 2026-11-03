"""Required synthetic playback evidence in native package manifests."""

PINNED_CHECKS = (
    "ok", "null_output", "plain_flac", "ncm_flac", "plain_aac_m4a",
    "ncm_aac_m4a", "external_references_denied",
    "pinned_handle_survived_path_replacement", "ordinary_load_restored",
)


def verify_pinned(report):
    assert isinstance(report, dict) and type(report.get("remote_requests")) is int and (
        report["remote_requests"] == 0
    ) and all(report.get(name) is True for name in PINNED_CHECKS), (
        "packaged pinned audio verification missing"
    )
