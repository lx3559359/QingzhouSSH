from __future__ import annotations

import argparse
import json
import os
import shutil
from pathlib import Path

from modelscope_hub import HubApi


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


def upload(api: HubApi, repo_id: str, release_dir: Path) -> None:
    repo_type = "studio"
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


def download(api: HubApi, repo_id: str, release_dir: Path, output_dir: Path) -> None:
    repo_type = "studio"
    metadata, names = release_contract(release_dir)
    version = str(metadata["version"])
    output_dir.mkdir(parents=True, exist_ok=True)
    for name in names:
        remote_path = f"releases/v{version}/{name}"
        downloaded = Path(api.download_file(repo_id, repo_type, remote_path, revision="master"))
        shutil.copyfile(downloaded, output_dir / name)
    latest = Path(api.download_file(repo_id, repo_type, "releases/latest.json", revision="master"))
    shutil.copyfile(latest, output_dir / "latest.json")
    print(json.dumps({"source": "modelscope", "version": version, "downloaded": len(names) + 1}))


def main() -> None:
    parser = argparse.ArgumentParser(description="Publish or read back a QingzhouSSH ModelScope release")
    parser.add_argument("mode", choices=("upload", "download"))
    parser.add_argument("--repo-id", required=True)
    parser.add_argument("--release-directory", required=True)
    parser.add_argument("--output-directory")
    args = parser.parse_args()

    release_dir = project_path(args.release_directory, must_exist=True)
    token = os.environ.get("MODELSCOPE_API_TOKEN")
    if args.mode == "upload" and not token:
        raise RuntimeError("MODELSCOPE_API_TOKEN is required for upload")
    api = HubApi(token=token)
    if args.mode == "upload":
        upload(api, args.repo_id, release_dir)
        return
    if not args.output_directory:
        raise ValueError("--output-directory is required for download")
    output_dir = project_path(args.output_directory, must_exist=False)
    download(api, args.repo_id, release_dir, output_dir)


if __name__ == "__main__":
    main()
