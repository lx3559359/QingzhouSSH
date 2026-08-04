from __future__ import annotations

import importlib.util
import json
import shutil
import sys
import types
import uuid
from pathlib import Path


PROJECT_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = PROJECT_ROOT / "scripts" / "modelscope-release.py"
sys.modules.setdefault("modelscope_hub", types.SimpleNamespace(HubApi=object))
spec = importlib.util.spec_from_file_location("modelscope_release", SCRIPT_PATH)
if spec is None or spec.loader is None:
    raise RuntimeError("Could not load ModelScope release helper")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

test_root = PROJECT_ROOT / ".local" / f"modelscope-release-test-{uuid.uuid4().hex}"
release_dir = test_root / "release"
output_dir = test_root / "readback"
release_dir.mkdir(parents=True)
metadata = {
    "version": "9.8.7",
    "files": [
        {"name": "installer.exe"},
        {"name": "portable.zip"},
    ],
}
(release_dir / "release-metadata.json").write_text(json.dumps(metadata), encoding="utf-8")

expected_names = [
    "installer.exe",
    "portable.zip",
    "SHA256SUMS",
    "SBOM.spdx.json",
    "release-metadata.json",
]


def fake_run(command: list[str], *, check: bool) -> None:
    if not check or command[:4] != ["git", "-c", "core.autocrlf=false", "clone"]:
        raise AssertionError(f"Unexpected clone command: {command}")
    if command[-2] != "https://modelscope.cn/studios/lx3559359/QingzhouSSH.git":
        raise AssertionError(f"Unexpected Studio clone URL: {command[-2]}")
    clone_root = Path(command[-1])
    release_root = clone_root / "releases" / "v9.8.7"
    release_root.mkdir(parents=True)
    for name in expected_names:
        (release_root / name).write_bytes(f"remote:{name}".encode())
    latest = clone_root / "releases" / "latest.json"
    latest.write_bytes(b"remote:latest")


try:
    module.download(
        "lx3559359/QingzhouSSH",
        release_dir,
        output_dir,
        runner=fake_run,
    )
    for name in expected_names:
        if (output_dir / name).read_bytes() != f"remote:{name}".encode():
            raise AssertionError(f"Readback file differs: {name}")
    if (output_dir / "latest.json").read_bytes() != b"remote:latest":
        raise AssertionError("ModelScope latest.json was not copied")
    try:
        module.download("invalid repo", release_dir, output_dir, runner=fake_run)
    except ValueError:
        pass
    else:
        raise AssertionError("Invalid Studio repository identifiers must be rejected")
finally:
    shutil.rmtree(test_root, ignore_errors=True)

print("PASS: ModelScope Studio readback uses a bounded public git clone")
