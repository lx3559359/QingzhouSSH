from __future__ import annotations

import argparse
import asyncio
import logging
import os
from pathlib import Path

import asyncssh


PROBE_OUTPUT = """__QZ_OS_BEGIN__
PRETTY_NAME="Ubuntu 24.04 LTS"
NAME="Ubuntu"
VERSION_ID="24.04"
ID=ubuntu
ID_LIKE=debian
__QZ_OS_END__
PKG=apt
SERVICE=systemd
ARCH=x86_64
SHELL=/bin/bash
"""


class FixtureServer(asyncssh.SSHServer):
    def password_auth_supported(self) -> bool:
        return True

    def validate_password(self, username: str, password: str) -> bool:
        return username == "testuser" and password == "testpass"


def handle_process(process: asyncssh.SSHServerProcess[str]) -> None:
    if process.command:
        process.stdout.write(PROBE_OUTPUT)
        process.exit(0)
        return

    process.stderr.write("This fixture accepts exec requests only.\n")
    process.exit(1)


async def serve(host_key: Path, authorized_keys: Path) -> None:
    server = await asyncssh.create_server(
        FixtureServer,
        "127.0.0.1",
        2222,
        server_host_keys=[str(host_key)],
        authorized_client_keys=str(authorized_keys),
        process_factory=handle_process,
        kex_algs=["diffie-hellman-group14-sha256"],
        encryption_algs=["aes256-ctr"],
        mac_algs=["hmac-sha2-256"],
    )
    async with server:
        await server.wait_closed()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="QingzhouSSH project-local SSH fixture")
    parser.add_argument("--host-key", required=True, type=Path)
    parser.add_argument("--authorized-keys", required=True, type=Path)
    return parser.parse_args()


if __name__ == "__main__":
    arguments = parse_args()
    if os.environ.get("QZ_SSH_FIXTURE_DEBUG") == "1":
        logging.basicConfig(level=logging.DEBUG)
        asyncssh.set_debug_level(2)
    asyncio.run(serve(arguments.host_key, arguments.authorized_keys))
