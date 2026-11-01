"""Read a concurrently replaced synthetic host snapshot with a bounded retry."""
import json
import time


def read_snapshot(path, seconds=2):
    deadline = time.monotonic() + seconds
    while True:
        try:
            return json.loads(path.read_text(encoding='utf-8'))
        except (PermissionError, FileNotFoundError, json.JSONDecodeError):
            # Windows can briefly deny opening a file during replacement.
            # A persistent missing, busy or malformed snapshot still fails.
            if time.monotonic() >= deadline:
                raise
            time.sleep(.025)
