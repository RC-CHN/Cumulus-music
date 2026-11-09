These are synthetic warm handoff fixtures for the retained package proof. They
are 24-second, 48 kHz, stereo 880 Hz tones created by
windows_native_open_smoke.create_wave. They do not contain recordings or
account data. Each title identifies the decoded row without trusting its path.

The fixtures were encoded with FFmpeg 8.0.1-3ubuntu2 using the ordinary proof's
media_fixtures encoder commands (FLAC; libmp3lame at 192 kbps with ID3v2.3).
To generate them locally from the repository root:

    PYTHONPATH=scripts/ci python - <<'PY'
    import os, shutil, tempfile
    from pathlib import Path
    from windows_native_open_smoke import media_fixtures
    root = Path('scripts/ci/fixtures/windows_native_open')
    with tempfile.TemporaryDirectory() as temp:
        _, flac, mp3, _ = media_fixtures(Path(temp), Path(shutil.which('ffmpeg')), os.environ)
        shutil.copyfile(flac, root / 'warm.flac')
        shutil.copyfile(mp3, root / 'warm.mp3')
    PY

The retained proof checks the fixed hashes before use. The Windows app still
has to select the correct held file and advance its actual decoder clock.
