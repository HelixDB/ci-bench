import os
import subprocess
import sys
from pathlib import Path

SRC = Path(__file__).parent.parent / "src"


def test_runs_the_cli_module():
    env = {**os.environ, "PYTHONPATH": str(SRC)}
    out = subprocess.run(
        [sys.executable, "-m", "adv.cli"], env=env, capture_output=True, text=True, check=True
    )
    assert out.stdout.strip() == "adv-cli 1"
