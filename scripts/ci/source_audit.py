"""Reject private/generated material in the tracked public source tree."""
from pathlib import Path
import subprocess

paths = subprocess.check_output(["git", "ls-files", "-z"]).decode().split("\0")
for value in filter(None, paths):
    path = Path(value)
    assert not set(path.parts) & {"references", ".build", "node_modules", "target", "__pycache__"}, f"private/generated path: {value}"
    assert path.name not in {".env", "cookies.txt", "cookies.json", "credentials.json"}, f"private path: {value}"
    assert path.suffix not in {".pyc", ".pyo", ".pem", ".key"}, f"unexpected private/generated file: {value}"
    if path.suffix in {".sqlite3", ".db", ".exe", ".dll", ".so", ".dylib"}:
        assert value == "core/tests/fixtures/python-state.sqlite3", f"unexpected database/native binary: {value}"
    assert path.stat().st_size < 25 * 1024 * 1024, f"unexpected large source file: {value}"
print("Public source audit passed; account sessions and recovery archives are not tracked.")
