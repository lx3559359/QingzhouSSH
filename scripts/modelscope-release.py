from __future__ import annotations

import argparse
import json
import os
import re
import shutil
from pathlib import Path
from typing import Any
from urllib.parse import urlencode
from urllib.request import urlopen

def project_root() -> Path:
    configured = os.environ.get("GITHUB_WORKSPACE")
    return Path(configured).resolve() if configured else Path(__file__).resolve().parents[1]


def project_path(value: str, *, must_exist: bool) -> Path:
    root = project_root()
    candidate = Path(value).resolve()
    try:
        candidate.relative_to(root)
    except ValueError as error:
        raise ValueError(f"path must remain inside the project folder: {candidate}") from error
    if must_exist and not candidate.exists():
        raise FileNotFoundError(candidate)
    return candidate


def release_contract(release_dir: Path) -> tuple[dict[str, object], list[str]]:
    metadata = json.loads((release_dir / "release-metadata.json").read_text(encoding="utf-8"))
    names = [str(item["name"]) for item in metadata["files"]]
    names.extend(["SHA256SUMS", "SBOM.spdx.json", "release-metadata.json"])
    if len(names) != len(set(names)):
        raise ValueError("release contract contains duplicate files")
    return metadata, names


def validate_repo_id(repo_id: str) -> None:
    if re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo_id) is None:
        raise ValueError("invalid ModelScope repository identifier")


def public_file_url(repo_id: str, file_path: str) -> str:
    validate_repo_id(repo_id)
    if (
        not file_path.startswith("releases/")
        or ".." in file_path
        or "\\" in file_path
        or file_path.startswith("/")
    ):
        raise ValueError("ModelScope file must remain inside releases/")
    query = urlencode({"Revision": "master", "FilePath": file_path})
    return f"https://modelscope.cn/api/v1/models/{repo_id}/repo?{query}"


def fetch_public_file(url: str, destination: Path) -> None:
    partial = destination.with_name(destination.name + ".partial")
    try:
        with urlopen(url, timeout=60) as response, partial.open("wb") as output:
            shutil.copyfileobj(response, output)
        partial.replace(destination)
    finally:
        partial.unlink(missing_ok=True)


def ensure_repository(api: Any, repo_id: str) -> None:
    validate_repo_id(repo_id)
    repo_type = "model"
    if not api.repo_exists(repo_id, repo_type):
        api.create_repo(
            repo_id,
            repo_type,
            visibility="public",
            license="Apache-2.0",
            chinese_name="轻舟 SSH 发布镜像",
            description="QingzhouSSH Windows 安装包、便携包与在线更新清单",
        )


def prepare(api: Any, repo_id: str, readme_path: Path) -> None:
    ensure_repository(api, repo_id)
    api.upload_file(repo_id, "model", str(readme_path), "releases/README.md")
    print(json.dumps({"source": "modelscope", "prepared": True}))


def upload(api: Any, repo_id: str, release_dir: Path) -> None:
    ensure_repository(api, repo_id)
    repo_type = "model"
    metadata, names = release_contract(release_dir)
    version = str(metadata["version"])
    for name in names:
        local_path = release_dir / name
        api.upload_file(repo_id, repo_type, str(local_path), f"releases/v{version}/{name}")
    api.upload_file(
        repo_id,
        repo_type,
        str(release_dir / "modelscope" / "latest.json"),
        "releases/latest.json",
    )
    print(json.dumps({"source": "modelscope", "version": version, "uploaded": len(names) + 1}))


def download(
    repo_id: str,
    release_dir: Path,
    output_dir: Path,
    *,
    fetcher: Any = fetch_public_file,
) -> None:
    validate_repo_id(repo_id)
    metadata, names = release_contract(release_dir)
    version = str(metadata["version"])
    output_dir.mkdir(parents=True, exist_ok=True)
    for name in names:
        fetcher(
            public_file_url(repo_id, f"releases/v{version}/{name}"),
            output_dir / name,
        )
    fetcher(public_file_url(repo_id, "releases/latest.json"), output_dir / "latest.json")
    print(json.dumps({"source": "modelscope", "version": version, "downloaded": len(names) + 1}))


def main() -> None:
    parser = argparse.ArgumentParser(description="Publish or read back a QingzhouSSH ModelScope release")
    parser.add_argument("mode", choices=("prepare", "upload", "download"))
    parser.add_argument("--repo-id", required=True)
    parser.add_argument("--release-directory")
    parser.add_argument("--output-directory")
    args = parser.parse_args()

    token = os.environ.get("MODELSCOPE_API_TOKEN")
    if args.mode in ("prepare", "upload") and not token:
        raise RuntimeError("MODELSCOPE_API_TOKEN is required for publication")
    if args.mode in ("prepare", "upload"):
        from modelscope_hub import HubApi

        api = HubApi(token=token)
        if args.mode == "prepare":
            prepare(api, args.repo_id, project_root() / "README.md")
            return
    if not args.release_directory:
        raise ValueError("--release-directory is required for upload and download")
    release_dir = project_path(args.release_directory, must_exist=True)
    if args.mode == "upload":
        upload(api, args.repo_id, release_dir)
        return
    if not args.output_directory:
        raise ValueError("--output-directory is required for download")
    output_dir = project_path(args.output_directory, must_exist=False)
    download(args.repo_id, release_dir, output_dir)


if __name__ == "__main__":
    main()
