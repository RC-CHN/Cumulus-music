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


def verify_parametric(report):
    failure = "packaged parametric equalizer verification missing"
    assert isinstance(report, dict) and report.get("nativeLibmpv") is True and (
        report.get("hardwareOutput") is False
    ), failure
    cases = report.get("cases")
    assert isinstance(cases, list) and len(cases) == 2, failure
    for rate, case in zip((44100, 96000), cases, strict=True):
        assert isinstance(case, dict) and case.get("sourceSampleRate") == rate, failure
        stats = case.get("stats")
        assert isinstance(stats, dict) and stats.get("active_instances") == 2 and (
            stats.get("failed_blocks") == 0
        ), failure
        assert type(stats.get("processed_samples")) is int and stats["processed_samples"] > 1024, failure
        assert type(stats.get("revision")) is int and stats["revision"] > 0 and (
            stats.get("observed_revision") == stats["revision"]
        ), failure
