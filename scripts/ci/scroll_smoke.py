"""Actual production WebView: bounded scrolling, page/dialog transitions and custom chrome."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    assert not root.exists(), 'proof directory must be new'
    root.parent.mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_PROOF_SMALL='1')
    with root.with_suffix('.log').open('x', encoding='utf-8') as log:
        result = subprocess.run([str(args.binary.resolve())], env=env, stdout=log, stderr=log, timeout=90)
    assert result.returncode == 0, f'UI scroll probe exited {result.returncode}; inspect {root.with_suffix(".log")}'
    proof = json.loads((root / 'result.json').read_text(encoding='utf-8'))
    assert proof['ok'] and proof['customControls'] and proof['dialogFocusRestored'], proof
    for label in ['迷你模式', '最小化', '最大化', '关闭窗口']:
        assert proof['windowButtons'].count(label) == 1, proof['windowButtons']
    assert proof['nativeDecorations'] is False and proof['realMusicRequests'] == 0
    assert 0 < proof['songs']['maxRows'] < 80 and proof['songs']['scrollTop'] > 8000
    # Virtual runners have no physical GPU/refresh rate. Their FPS is diagnostic,
    # not a claim about real hardware; do not make a flaky timing gate out of it.
    print(json.dumps(proof))


if __name__ == '__main__':
    main()
