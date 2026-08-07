from __future__ import annotations

import argparse
import asyncio
import base64
import gzip
import logging
import os
import re
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
COMMANDS=grep,gzip,awk,systemctl,systemd-run,service,ps,head,df,uptime,uname,free,ip,nmcli,hostname,hostnamectl,timedatectl,swapon,swapoff,mkswap,stat,chmod,chown,fallocate,sha256sum,mktemp,cat,mv,rm,mkdir,rmdir,find,sh,docker,base64,sed,tr,id
SERVICES=qingzhou-fixture.service,qingzhou-verify-fail.service,
CONTAINERS=fixture-container,
INTERFACES=eth0,lo,
ACTIVE_INTERFACES=eth0,lo,
DEFAULT_INTERFACE=eth0
ADDRESSES=eth0|192.0.2.10/24;lo|127.0.0.1/8;
GATEWAYS4=eth0|192.0.2.1;
GATEWAYS6=
DNS_SERVERS=223.5.5.5,1.1.1.1,
CURRENT_TIMEZONE=UTC
CURRENT_TIME=2026-08-07 04:00:00 UTC
NTP_ENABLED=yes
NTP_SYNCHRONIZED=yes
TIMEZONES=UTC,Asia/Shanghai,
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

FAILURE_MARKERS = ("workflow-fail.service", "workflow-fail-script", "workflow-fail.log")
REMOTE_ROOT: Path | None = None


class FixtureServer(asyncssh.SSHServer):
    def password_auth_supported(self) -> bool:
        return True

    def validate_password(self, username: str, password: str) -> bool:
        return username in {"testuser", "root-sim", "sudo-user", "no-priv"} and password == "testpass"


def command_result(command: str, username: str = "testuser") -> tuple[str, str, int]:
    if any(marker in command for marker in FAILURE_MARKERS):
        return "", "workflow fixture injected failure\n", 73
    if "__QZ_OS_BEGIN__" in command:
        return PROBE_OUTPUT, "", 0
    if command.strip() == "id -u":
        return ("0\n", "", 0) if username in {"testuser", "root-sim"} else ("1000\n", "", 0)
    if command.strip() == "sudo -n true":
        return ("", "", 0) if username == "sudo-user" else ("", "sudo: a password is required\n", 1)
    if "qz_addresses=$(ip -o address show" in command and "gatewayfour" in command:
        return (
            "interface=eth0\n"
            "addresses=127.0.0.1/8\n"
            "gatewayfour=127.0.0.1\n"
            "gatewaysix=none\n",
            "",
            0,
        )
    if "backend=networkmanager" in command and "connectionb" in command:
        encode = lambda value: base64.b64encode(value.encode("utf-8")).decode("ascii")
        return (
            "backend=networkmanager\n"
            "interface=eth0\n"
            f"connectionb={encode('fixture-connection')}\n"
            f"ipfourmethodb={encode('manual')}\n"
            f"ipfouraddressesb={encode('127.0.0.1/8')}\n"
            f"ipfourgatewayb={encode('127.0.0.1')}\n"
            f"ipsixmethodb={encode('ignore')}\n"
            "ipsixaddressesb=\n"
            "ipsixgatewayb=\n",
            "",
            0,
        )
    if "rollback_armed" in command:
        run_id = recovery_run_id(command)
        hashes = re.findall(r"[0-9a-f]{64}", command)
        if run_id is None or not hashes:
            return "", "fixture rejected malformed recovery arm command\n", 64
        write_state(f"ip-recovery-{run_id}", f"armed:{hashes[0]}")
        append_ip_event(run_id, "armed")
        return "rollback_armed\n", "", 0
    if "network_applied" in command:
        run_id = recovery_run_id(command)
        if run_id is None or not read_optional_state(f"ip-recovery-{run_id}", "").startswith("armed:"):
            return "", "fixture refused network apply before rollback was armed\n", 65
        recovery = read_state(f"ip-recovery-{run_id}")
        write_state(f"ip-recovery-{run_id}", recovery.replace("armed:", "staged:", 1))
        append_ip_event(run_id, "applied")
        return "network_applied\n", "", 0
    if "target_connection_verified" in command:
        run_id = recovery_run_id(command)
        if run_id is None or not read_optional_state(f"ip-recovery-{run_id}", "").startswith("staged:"):
            return "", "fixture refused network finalize before staged apply\n", 66
        recovery = read_state(f"ip-recovery-{run_id}")
        write_state(f"ip-recovery-{run_id}", recovery.replace("staged:", "committed:", 1))
        append_ip_event(run_id, "finalized")
        return "target_connection_verified\nrollback_cancelled\n", "", 0
    if "printf 'state='" in command and "printf 'scriptsha='" in command:
        run_id = recovery_run_id(command)
        recovery = read_optional_state(f"ip-recovery-{run_id}") if run_id else None
        if run_id is None or recovery is None or ":" not in recovery:
            return "", "fixture recovery state is missing\n", 67
        state, script_hash = recovery.split(":", 1)
        return f"state={state}\nscriptsha={script_hash}\ncommitted={'true' if state == 'committed' else 'false'}\n", "", 0
    if 'find "$qz_dir" -xdev -depth -mindepth 1 -delete' in command:
        run_id = recovery_run_id(command)
        if run_id is None:
            return "", "fixture rejected malformed recovery cleanup command\n", 64
        append_ip_event(run_id, "cleaned")
        return "", "", 0
    if "printf 'manager=systemd\\nservice=%s\\nactive=%s\\nenabled=%s\\n'" in command:
        match = re.search(
            r"systemctl is-active -- '?([A-Za-z0-9_.@-]+)'?", command
        )
        if not match or match.group(1) not in {
            "qingzhou-fixture.service",
            "qingzhou-verify-fail.service",
        }:
            return "", f"fixture rejected service snapshot target: {command}\n", 64
        service = match.group(1)
        return (
            f"manager=systemd\nservice={service}\n"
            f"active={read_state(service_state_key(service))}\n"
            f"enabled={read_state(service_policy_key(service))}\n",
            "",
            0,
        )
    service_action = re.search(
        r"systemctl (start|stop|restart) -- '(qingzhou-(?:fixture|verify-fail)\.service)'",
        command,
    )
    if service_action:
        action, service = service_action.groups()
        write_state(service_state_key(service), "inactive" if action == "stop" else "active")
        if service == "qingzhou-verify-fail.service" and action == "stop":
            write_state("service-verify-fail-once", "pending")
        return "fixture service operation completed\n", "", 0
    if "systemctl is-active --" in command and command.lstrip().startswith("test "):
        service_match = re.search(
            r"systemctl is-active -- '(qingzhou-(?:fixture|verify-fail)\.service)'",
            command,
        )
        expected_match = re.search(r"\)\" = '?([a-z-]+)'?", command)
        if not service_match or not expected_match:
            return "", "fixture rejected malformed service verification\n", 64
        service = service_match.group(1)
        if (
            service == "qingzhou-verify-fail.service"
            and read_optional_state("service-verify-fail-once") == "pending"
        ):
            write_state("service-verify-fail-once", "consumed")
            return "", "fixture injected service verification failure\n", 74
        expected = expected_match.group(1)
        actual = read_state(service_state_key(service))
        return ("service verified\n", "", 0) if actual == expected else (
            "",
            f"service state is {actual}, expected {expected}\n",
            1,
        )
    if "printf 'runtime=docker\\ncontainer=%s\\nstate=%s\\n'" in command:
        match = re.search(r"-- '?(fixture-container)'?", command)
        if not match:
            return "", "fixture rejected container snapshot target\n", 64
        return (
            f"runtime=docker\ncontainer={match.group(1)}\n"
            f"state={read_state(container_state_key(match.group(1)))}\n",
            "",
            0,
        )
    container_action = re.search(
        r"docker '?(start|stop|restart|pause|unpause)'? -- '(fixture-container)'",
        command,
    )
    if container_action and not command.startswith("qz_action="):
        action, container = container_action.groups()
        state = {
            "start": "running",
            "stop": "stopped",
            "restart": "running",
            "pause": "paused",
            "unpause": "running",
        }[action]
        write_state(container_state_key(container), state)
        return "fixture container operation completed\n", "", 0
    if command.startswith("qz_action=") and "docker inspect --format" in command:
        action_match = re.match(r"qz_action='?([a-z]+)'?;", command)
        container_match = re.search(r"-- '(fixture-container)'", command)
        if not action_match or not container_match:
            return "", "fixture rejected malformed container verification\n", 64
        expected = {
            "start": "running",
            "stop": "stopped",
            "restart": "running",
            "pause": "paused",
            "unpause": "running",
        }[action_match.group(1)]
        actual = read_state(container_state_key(container_match.group(1)))
        return ("container verified\n", "", 0) if actual == expected else (
            "",
            f"container state is {actual}, expected {expected}\n",
            1,
        )
    if "printf 'hostname=%s\\n'" in command:
        return f"hostname={read_state('hostname')}\n", "", 0
    if "hostnamectl set-hostname --" in command:
        match = re.search(r"hostnamectl set-hostname -- '([^']+)'", command)
        if not match:
            return "", "fixture rejected malformed hostname command\n", 64
        write_state("hostname", match.group(1))
        return "hostname updated\n", "", 0
    if 'test "$(hostname)" =' in command:
        match = re.search(r'test "\$\(hostname\)" = \'([^\']+)\'', command)
        if not match:
            return "", "fixture rejected malformed hostname verification\n", 64
        expected = match.group(1)
        if expected == "fixture-verify-failure":
            return "", "fixture injected hostname verification failure\n", 74
        return ("hostname verified\n", "", 0) if read_state("hostname") == expected else (
            "",
            "hostname does not match\n",
            1,
        )
    if command.startswith("hostnamectl status --static"):
        hostname = read_state("hostname")
        return f"{hostname}\n{hostname}\n", "", 0
    if "printf 'timezone=%s\\n'" in command:
        return f"timezone={read_state('timezone')}\n", "", 0
    if "timedatectl set-timezone --" in command:
        match = re.search(r"timedatectl set-timezone -- '([^']+)'", command)
        if not match:
            return "", "fixture rejected malformed timezone command\n", 64
        write_state("timezone", match.group(1))
        return "timezone updated\n", "", 0
    if 'test "$(timedatectl show -p Timezone --value)" =' in command:
        match = re.search(
            r'test "\$\(timedatectl show -p Timezone --value\)" = \'([^\']+)\'',
            command,
        )
        if not match:
            return "", "fixture rejected malformed timezone verification\n", 64
        expected = match.group(1)
        actual = read_state("timezone")
        return ("timezone verified\n", "", 0) if actual == expected else (
            "",
            f"timezone is {actual}, expected {expected}\n",
            1,
        )
    if command.strip() == "timedatectl show -p Timezone --value; timedatectl status":
        timezone = read_state("timezone")
        return f"{timezone}\nTime zone: {timezone}\n", "", 0
    if "printf 'ntp=%s\\n'" in command and "qz_ntp=" in command:
        return f"ntp={read_state('ntp')}\n", "", 0
    if command.startswith("timedatectl set-ntp "):
        match = re.match(r"timedatectl set-ntp '?(true|false)'?", command)
        if not match:
            return "", "fixture rejected malformed time sync command\n", 64
        write_state("ntp", match.group(1))
        return "time synchronization updated\n", "", 0
    if command.startswith("qz_target=") and "timedatectl show -p NTP --value" in command:
        match = re.match(r"qz_target='?(true|false)'?;", command)
        if not match:
            return "", "fixture rejected malformed time sync verification\n", 64
        expected = match.group(1)
        actual = read_state("ntp")
        return ("time synchronization verified\n", "", 0) if actual == expected else (
            "",
            f"time synchronization is {actual}, expected {expected}\n",
            1,
        )
    if "printf 'current_time='" in command and "timedatectl show -p NTP --value" in command:
        ntp = read_state("ntp")
        return (
            f"current_time=2026-08-07T04:00:00+00:00\n"
            f"ntp={ntp}\nsynchronized={ntp}\n",
            "",
            0,
        )
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


async def handle_process(process: asyncssh.SSHServerProcess[str]) -> None:
    if process.command:
        if "workflow-cancel-delay" in process.command:
            await asyncio.sleep(10)
            process.stdout.write("workflow-cancel-delay completed\n")
            process.exit(0)
            return
        if "fixture-disconnect" in process.command and "hostnamectl set-hostname" in process.command:
            write_state("disconnect-triggered", "true")
            process.channel.abort()
            await process.channel.wait_closed()
            return
        username = process.get_extra_info("username", "testuser")
        stdout, stderr, status = command_result(process.command, username)
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
    deploy_root = remote_root / "opt" / "qingzhou-app"
    service_root = remote_root / "run" / "qingzhou-fixture"
    log_root.mkdir(parents=True, exist_ok=True)
    temp_root.mkdir(parents=True, exist_ok=True)
    deploy_root.mkdir(parents=True, exist_ok=True)
    service_root.mkdir(parents=True, exist_ok=True)
    (remote_root / "etc").mkdir(parents=True, exist_ok=True)
    (remote_root / "etc" / "fstab").write_text("# fixture fstab\n", encoding="utf-8")
    write_state("hostname", "qingzhou-fixture", remote_root)
    write_state("timezone", "UTC", remote_root)
    write_state("ntp", "true", remote_root)
    for service in ["qingzhou-fixture.service", "qingzhou-verify-fail.service"]:
        write_state(service_state_key(service), "active", remote_root)
        write_state(service_policy_key(service), "enabled", remote_root)
    write_state(container_state_key("fixture-container"), "running", remote_root)
    content = (
        "2026-08-03T09:59:59 fixture ready\n"
        "2026-08-03T10:00:00 ERROR fixture failure\n"
        "2026-08-03T10:00:01 recovery started\n"
    )
    (log_root / "qingzhou.log").write_text(content, encoding="utf-8")
    with gzip.open(log_root / "qingzhou.log.gz", "wt", encoding="utf-8") as archive:
        archive.write(content)
    (deploy_root / "config.yml").write_text("version: fixture-original\n", encoding="utf-8")
    (service_root / "service.state").write_text("active\n", encoding="utf-8")


async def serve(host_key: Path, authorized_keys: Path, remote_root: Path) -> None:
    global REMOTE_ROOT
    REMOTE_ROOT = remote_root
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


def read_state(name: str) -> str:
    if REMOTE_ROOT is None:
        raise RuntimeError("fixture remote root is not initialized")
    return (REMOTE_ROOT / "run" / "qingzhou-fixture" / f"{name}.state").read_text(
        encoding="utf-8"
    ).strip()


def read_optional_state(name: str, default: str | None = None) -> str | None:
    if REMOTE_ROOT is None:
        raise RuntimeError("fixture remote root is not initialized")
    path = REMOTE_ROOT / "run" / "qingzhou-fixture" / f"{name}.state"
    return path.read_text(encoding="utf-8").strip() if path.exists() else default


def recovery_run_id(command: str) -> str | None:
    match = re.search(r"qingzhou-recovery(?:/|-)([0-9a-f-]{36})", command)
    if match:
        return match.group(1)
    match = re.search(r"\b[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\b", command)
    return match.group(0) if match else None


def append_ip_event(run_id: str, event: str) -> None:
    key = f"ip-events-{run_id}"
    current = read_optional_state(key, "") or ""
    write_state(key, ",".join(filter(None, [current, event])))


def service_state_key(service: str) -> str:
    return f"service-{service}"


def service_policy_key(service: str) -> str:
    return f"service-policy-{service}"


def container_state_key(container: str) -> str:
    return f"container-{container}"


def write_state(name: str, value: str, remote_root: Path | None = None) -> None:
    root = remote_root or REMOTE_ROOT
    if root is None:
        raise RuntimeError("fixture remote root is not initialized")
    state_root = root / "run" / "qingzhou-fixture"
    state_root.mkdir(parents=True, exist_ok=True)
    (state_root / f"{name}.state").write_text(f"{value}\n", encoding="utf-8")


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
