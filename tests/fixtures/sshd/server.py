from __future__ import annotations

import argparse
import asyncio
import gzip
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
COMMANDS=grep,gzip,awk,systemctl,service,ps,head,df,uptime,uname,free,ip,hostname,sh
"""

DISK_OUTPUT = """Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/fixture 1048576 262144 786432 25% /
"""

OVERVIEW_OUTPUT = """== system ==
Linux qingzhou-fixture 6.8.0 x86_64 GNU/Linux
== uptime ==
 10:00:00 up 2 days, load average: 0.10, 0.12, 0.08
== memory ==
Mem: 2147483648 536870912 1610612736
== disk ==
Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/fixture 1048576 262144 786432 25% /
== network ==
eth0 UP 127.0.0.1/8
== processes ==
PID USER %CPU %MEM ELAPSED COMMAND
1 testuser 0.0 0.1 02:00 fixture-service
"""

LOG_RECORDS = (
    "__QZ_LOG__\x1f1\x1fcontext\x1f2026-08-03T09:59:59 fixture ready\n"
    "__QZ_LOG__\x1f2\x1fmatch\x1f2026-08-03T10:00:00 ERROR fixture failure\n"
    "__QZ_LOG__\x1f3\x1fcontext\x1f2026-08-03T10:00:01 recovery started\n"
)


class FixtureServer(asyncssh.SSHServer):
    def password_auth_supported(self) -> bool:
        return True

    def validate_password(self, username: str, password: str) -> bool:
        return username == "testuser" and password == "testpass"


def command_result(command: str) -> tuple[str, str, int]:
    if "__QZ_OS_BEGIN__" in command:
        return PROBE_OUTPUT, "", 0
    if command.strip() == "df -P -B1":
        return DISK_OUTPUT, "", 0
    if command.startswith("systemctl status -- qingzhou-fixture.service"):
        return "qingzhou-fixture.service is active (running)\n", "", 0
    if command.startswith("systemctl "):
        return "fixture service operation completed\n", "", 0
    if "__QZ_LOG__" in command and ("qingzhou.log" in command or "qingzhou.log.gz" in command):
        return LOG_RECORDS, "", 0
    if "advanced-command-ok" in command:
        return "advanced-command-ok\n", "", 0
    if "advanced-script-ok" in command:
        return "advanced-script-ok\n", "", 0
    if command.startswith("printf '== system =="):
        return OVERVIEW_OUTPUT, "", 0
    if command.startswith("ps -eo"):
        return "1 testuser 0.0 0.1 02:00 fixture-service\n", "", 0
    return "fixture command completed\n", "", 0


def handle_process(process: asyncssh.SSHServerProcess[str]) -> None:
    if process.command:
        stdout, stderr, status = command_result(process.command)
        if stdout:
            process.stdout.write(stdout)
        if stderr:
            process.stderr.write(stderr)
        process.exit(status)
        return

    process.stderr.write("This fixture accepts exec requests only.\n")
    process.exit(1)


def prepare_remote_root(remote_root: Path) -> None:
    log_root = remote_root / "var" / "log"
    temp_root = remote_root / "tmp"
    log_root.mkdir(parents=True, exist_ok=True)
    temp_root.mkdir(parents=True, exist_ok=True)
    content = (
        "2026-08-03T09:59:59 fixture ready\n"
        "2026-08-03T10:00:00 ERROR fixture failure\n"
        "2026-08-03T10:00:01 recovery started\n"
    )
    (log_root / "qingzhou.log").write_text(content, encoding="utf-8")
    with gzip.open(log_root / "qingzhou.log.gz", "wt", encoding="utf-8") as archive:
        archive.write(content)


async def serve(host_key: Path, authorized_keys: Path, remote_root: Path) -> None:
    prepare_remote_root(remote_root)
    server = await asyncssh.create_server(
        FixtureServer,
        "127.0.0.1",
        2222,
        server_host_keys=[str(host_key)],
        authorized_client_keys=str(authorized_keys),
        process_factory=handle_process,
        sftp_factory=lambda channel: asyncssh.SFTPServer(
            channel, chroot=os.fsencode(remote_root)
        ),
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
    parser.add_argument("--remote-root", required=True, type=Path)
    return parser.parse_args()


if __name__ == "__main__":
    arguments = parse_args()
    if os.environ.get("QZ_SSH_FIXTURE_DEBUG") == "1":
        logging.basicConfig(level=logging.DEBUG)
        asyncssh.set_debug_level(2)
    asyncio.run(serve(arguments.host_key, arguments.authorized_keys, arguments.remote_root))
