from __future__ import annotations

import importlib.util
import json
import shutil
import sys
import types
import urllib.parse
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
healthcheck = test_root / "healthcheck.bin"
release_dir.mkdir(parents=True)
healthcheck.write_bytes(bytes(range(256)))
metadata = {
    "version": "9.8.7",
    "files": [
        {"name": "installer.exe"},
        {"name": "portable.zip"},
    ],
}
(release_dir / "release-metadata.json").write_text(json.dumps(metadata), encoding="utf-8")
(release_dir / "modelscope").mkdir()
(release_dir / "modelscope" / "latest.json").write_bytes(b"local:latest")

expected_names = [
    "installer.exe",
    "portable.zip",
    "SHA256SUMS",
    "SBOM.spdx.json",
    "release-metadata.json",
]
for expected_name in expected_names:
    path = release_dir / expected_name
    if not path.exists():
        path.write_bytes(f"local:{expected_name}".encode())


class FakeApi:
    def __init__(self) -> None:
        self.exists = False
        self.created: list[tuple[tuple[object, ...], dict[str, object]]] = []
        self.uploaded: list[tuple[object, ...]] = []

    def repo_exists(self, repo_id: str, repo_type: str) -> bool:
        if (repo_id, repo_type) != ("lx3559359/QingzhouSSH", "model"):
            raise AssertionError("Unexpected repository existence probe")
        return self.exists

    def create_repo(self, *args: object, **kwargs: object) -> None:
        self.created.append((args, kwargs))
        self.exists = True

    def upload_file(self, *args: object) -> None:
        self.uploaded.append(args)


downloaded_urls: list[str] = []


def fake_fetch(url: str, destination: Path) -> None:
    parsed = urllib.parse.urlparse(url)
    if parsed.netloc != "modelscope.cn" or parsed.path != "/api/v1/models/lx3559359/QingzhouSSH/repo":
        raise AssertionError(f"Unexpected model download URL: {url}")
    query = urllib.parse.parse_qs(parsed.query)
    if query.get("Revision") != ["master"]:
        raise AssertionError(f"Unexpected revision: {url}")
    remote_path = query.get("FilePath", [""])[0]
    downloaded_urls.append(url)
    destination.write_bytes(f"remote:{Path(remote_path).name}".encode())


try:
    fake_api = FakeApi()
    module.ensure_repository(fake_api, "lx3559359/QingzhouSSH")
    if fake_api.created != [
        (
            ("lx3559359/QingzhouSSH", "model"),
            {
                "visibility": "public",
                "license": "Apache-2.0",
                "chinese_name": "轻舟 SSH 发布镜像",
                "description": "QingzhouSSH Windows 安装包、便携包与在线更新清单",
            },
        )
    ]:
        raise AssertionError(f"Model repository creation contract differs: {fake_api.created}")
    module.prepare(fake_api, "lx3559359/QingzhouSSH", healthcheck)
    if fake_api.uploaded != [
        ("lx3559359/QingzhouSSH", "model", str(healthcheck), "releases/healthcheck.bin")
    ]:
        raise AssertionError(f"Binary healthcheck upload contract differs: {fake_api.uploaded}")
    fake_api.uploaded.clear()
    module.upload(fake_api, "lx3559359/QingzhouSSH", release_dir)
    if len(fake_api.uploaded) != len(expected_names) + 1:
        raise AssertionError("Every release file and latest manifest must be uploaded")
    if any(call[1] != "model" for call in fake_api.uploaded):
        raise AssertionError("Release files must be uploaded to a ModelScope model repository")

    module.download(
        "lx3559359/QingzhouSSH",
        release_dir,
        output_dir,
        fetcher=fake_fetch,
    )
    for name in expected_names:
        if (output_dir / name).read_bytes() != f"remote:{name}".encode():
            raise AssertionError(f"Readback file differs: {name}")
    if (output_dir / "latest.json").read_bytes() != b"remote:latest.json":
        raise AssertionError("ModelScope latest.json was not copied")
    if len(downloaded_urls) != len(expected_names) + 1:
        raise AssertionError("Public readback did not fetch every release file")
    try:
        module.download("invalid repo", release_dir, output_dir, fetcher=fake_fetch)
    except ValueError:
        pass
    else:
        raise AssertionError("Invalid ModelScope repository identifiers must be rejected")
finally:
    shutil.rmtree(test_root, ignore_errors=True)

print("PASS: ModelScope model mirror uses public single-file readback")
