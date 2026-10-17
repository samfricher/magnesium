#!/usr/bin/env python3
"""Validate active learner-model/v2 bundle integrity and explicit release evidence."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import stat
import sys
import tarfile
import zipfile
from pathlib import Path, PurePosixPath


MARKER = b"<!-- RESERVED_FOR_EXPERT_AUTHORING: DO_NOT_ACTIVATE -->\n"
WORKFLOW_STAGES = {
    "orient-and-probe", "plan-and-route", "teach-and-practice",
    "interpret-and-repair", "review-and-transfer", "consolidate-and-resume",
    "papers", "papers-loop",
}
CONTRACT_FILES = {
    "contracts/state-and-evidence.md",
    "contracts/source-use.md",
    "contracts/tool-recipes.md",
}
ADAPTER_FILES = {"adapters/codex.md", "adapters/claude.md", "adapters/opencode.md"}
HEX256 = re.compile(r"^[0-9a-f]{64}$")
CODE_STATES = {
    "CODE_READY_FOR_REVIEW",
    "POLICY_AUTHORED_AWAITING_LIVE_ACCEPTANCE",
    "RELEASE_ELIGIBLE",
}


class GateError(ValueError):
    pass


def _read_json(path: Path, label: str) -> dict:
    if path.is_symlink() or not path.is_file():
        raise GateError(f"{label} is not a regular file: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise GateError(f"invalid {label}: {exc}") from exc
    if not isinstance(value, dict):
        raise GateError(f"{label} must be a JSON object")
    return value


def _digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _regular_bytes(root: Path, relative: str) -> bytes:
    path = root / relative
    current = root
    for part in PurePosixPath(relative).parts:
        current = current / part
        try:
            info = current.lstat()
        except OSError as exc:
            raise GateError(f"missing learning bundle file {relative}: {exc}") from exc
        if stat.S_ISLNK(info.st_mode):
            raise GateError(f"learning bundle path contains a symlink: {relative}")
    if not path.is_file():
        raise GateError(f"learning bundle path is not a regular file: {relative}")
    return path.read_bytes()


def _bundle_paths(root: Path) -> set[str]:
    found: set[str] = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            raise GateError(f"learning bundle contains a symlink: {path.relative_to(root)}")
        if path.is_file():
            found.add(path.relative_to(root).as_posix())
        elif not path.is_dir():
            raise GateError(f"learning bundle contains a non-file entry: {path.relative_to(root)}")
    return found


def _manifest_entries(root: Path, *, status: str) -> list[dict[str, object]]:
    manifest = _read_json(root / "manifest.json", "learning bundle manifest")
    if set(manifest) != {
        "bundle_version", "status", "minimum_learning_step_schema", "entrypoint", "files"
    } or manifest != {
        **manifest,
        "bundle_version": 2,
        "status": status,
        "minimum_learning_step_schema": 2,
        "entrypoint": "MAGNESIUM.md",
    }:
        raise GateError("learning bundle manifest has an unsupported shape or version")
    entries = manifest["files"]
    if not isinstance(entries, list) or len(entries) != 16:
        raise GateError("learning bundle manifest must declare exactly 16 runtime-consumed files")
    paths: set[str] = set()
    stages: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise GateError("learning bundle file metadata must be an object")
        path = entry.get("path")
        role = entry.get("role")
        stage = entry.get("stage")
        host = entry.get("host")
        if (not isinstance(path, str) or not path or "\\" in path
                or PurePosixPath(path).as_posix() != path
                or any(part in {".", ".."} for part in PurePosixPath(path).parts)
                or path.startswith("/") or path in paths):
            raise GateError("learning manifest has an unsafe or duplicate path")
        paths.add(path)
        if (set(entry) - {"path", "role", "required_for_activation", "sha256", "stage", "host"}
                or entry.get("required_for_activation") is not True
                or not isinstance(entry.get("sha256"), str)
                or not HEX256.fullmatch(entry["sha256"])):
            raise GateError(f"invalid closed file metadata: {path}")
        if path.startswith("workflows/"):
            expected_stage = path.removeprefix("workflows/").removesuffix(".md")
            if role != "workflow" or stage != expected_stage or host is not None:
                raise GateError(f"workflow metadata does not match its path: {path}")
            stages.add(stage)
        elif path.startswith("adapters/"):
            expected_host = path.removeprefix("adapters/").removesuffix(".md")
            if path not in ADAPTER_FILES or role != "adapter" or host != expected_host or stage is not None:
                raise GateError(f"adapter metadata does not match its path: {path}")
        elif path.startswith("contracts/"):
            if path not in CONTRACT_FILES or role != "contract" or stage is not None or host is not None:
                raise GateError(f"contract metadata does not match its path: {path}")
        elif path == "MAGNESIUM.md":
            if role != "orchestrator" or stage is not None or host is not None:
                raise GateError("entrypoint metadata is invalid")
        elif path == "teaching-policy.md":
            if role != "policy" or stage is not None or host is not None:
                raise GateError("policy metadata is invalid")
        else:
            raise GateError(f"unexpected learning bundle path: {path}")
    if stages != WORKFLOW_STAGES:
        raise GateError("learning bundle must include each required workflow stage")
    required_runtime_paths = {
        "MAGNESIUM.md",
        "teaching-policy.md",
        *CONTRACT_FILES,
        *ADAPTER_FILES,
        *(f"workflows/{stage}.md" for stage in WORKFLOW_STAGES),
    }
    if paths != required_runtime_paths:
        raise GateError(
            "learning manifest must contain exactly the runtime assets: "
            f"missing={sorted(required_runtime_paths - paths)}, "
            f"unexpected={sorted(paths - required_runtime_paths)}"
        )
    return entries


def check_release_report(path: Path) -> None:
    report = _read_json(path, "learning release status")
    expected_keys = {
        "schema_version", "code_status", "policy_status", "actual_host_status",
        "human_learning_status", "measured_claims", "limitations",
    }
    if (
        set(report) != expected_keys
        or type(report.get("schema_version")) is not int
        or report["schema_version"] != 1
    ):
        raise GateError("learning release report has an unsupported or incomplete shape")
    code = report["code_status"]
    policy = report["policy_status"]
    host = report["actual_host_status"]
    human = report["human_learning_status"]
    claims = report["measured_claims"]
    if code not in CODE_STATES:
        raise GateError("unknown code status")
    if policy not in {"NOT_AUTHORED", "AUTHORED"}:
        raise GateError("unknown policy status")
    if host not in {"NOT_TESTED", "PASS", "FAIL"}:
        raise GateError("unknown actual-host status")
    if human not in {"NOT_MEASURED", "MEASURED"}:
        raise GateError("unknown human-learning status")
    if not isinstance(claims, list) or any(not isinstance(item, str) or not item.strip() for item in claims):
        raise GateError("measured_claims must contain non-empty claim names")
    if not isinstance(report["limitations"], list) or any(
        not isinstance(item, str) or not item.strip() for item in report["limitations"]
    ):
        raise GateError("limitations must contain non-empty descriptions")
    if code == "CODE_READY_FOR_REVIEW":
        if host != "NOT_TESTED" or human != "NOT_MEASURED" or claims:
            raise GateError("code-review state cannot claim measured host or human behavior")
    if code == "POLICY_AUTHORED_AWAITING_LIVE_ACCEPTANCE":
        if policy != "AUTHORED" or host not in {"NOT_TESTED", "FAIL"}:
            raise GateError("authored-policy state requires authored policy and an explicit untested or failed host status")
    if code == "RELEASE_ELIGIBLE" and (policy != "AUTHORED" or host != "PASS"):
        raise GateError("release eligibility requires authored policy and actual-host PASS")
    if host == "PASS" and policy != "AUTHORED":
        raise GateError("actual-host PASS requires authored policy")
    if human == "MEASURED" and not claims:
        raise GateError("measured human-learning status requires named measured claims")
    if human == "NOT_MEASURED" and any("human learning" in item.lower() for item in claims):
        raise GateError("unmeasured human learning cannot appear in measured_claims")


def check_candidate_bundle(root: Path) -> None:
    """Validate complete active assets against their declared content hashes."""
    entries = _manifest_entries(root, status="active")
    expected_paths = {"manifest.json", *(entry["path"] for entry in entries)}
    actual = _bundle_paths(root)
    if actual != expected_paths:
        raise GateError("candidate bundle is partial or contains unrecognized files")
    manifest_bytes = _regular_bytes(root, "manifest.json")
    try:
        manifest = json.loads(manifest_bytes)
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise GateError(f"invalid candidate manifest: {exc}") from exc
    if not isinstance(manifest, dict) or set(manifest) != {
        "bundle_version", "status", "minimum_learning_step_schema", "entrypoint", "files"
    }:
        raise GateError("candidate manifest has an unsupported shape")
    if (
        type(manifest["bundle_version"]) is not int
        or manifest["bundle_version"] != 2
        or manifest["status"] != "active"
        or type(manifest["minimum_learning_step_schema"]) is not int
        or manifest["minimum_learning_step_schema"] != 2
        or manifest["entrypoint"] != "MAGNESIUM.md"
        or not isinstance(manifest["files"], list)
        or len(manifest["files"]) != len(entries)
    ):
        raise GateError("candidate bundle version, schema, activation status, or file set is invalid")
    expected_entries = []
    for item in manifest["files"]:
        path = item["path"]
        data = _regular_bytes(root, path)
        if not data.strip() or data == MARKER or MARKER in data:
            raise GateError(f"candidate contains blank or reserved-marker content: {path}")
        entry = dict(item)
        entry["sha256"] = _digest(data)
        if item != entry:
            raise GateError(f"candidate manifest entry does not match file bytes or metadata: {path}")
        expected_entries.append(entry)
    if manifest["files"] != expected_entries:
        raise GateError("candidate manifest paths are incomplete, reordered, duplicated, or mixed-version")


def check_source_bundle(source_root: Path) -> None:
    root = source_root / "src/workspace/learning"
    manifest = _read_json(root / "manifest.json", "learning bundle manifest")
    if manifest.get("status") != "active":
        raise GateError("production bundle must be active learner-model/v2")
    check_candidate_bundle(root)


def check_archive(path: Path) -> None:
    if path.is_symlink() or not path.is_file():
        raise GateError("release archive is not a regular file")
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            entries = []
            for item in archive.infolist():
                mode = (item.external_attr >> 16) & 0xFFFF
                file_type = stat.S_IFMT(mode)
                entries.append((item.filename, item.is_dir() or file_type not in (0, stat.S_IFREG)))
    elif path.name.endswith(".tar.gz"):
        with tarfile.open(path, "r:gz") as archive:
            entries = [(item.name, not item.isreg()) for item in archive]
    else:
        raise GateError("release archive must be zip or tar.gz")
    for name, unsafe_type in entries:
        parts = PurePosixPath(name).parts
        if (not parts or name.startswith("/") or "\\" in name
                or PurePosixPath(name).as_posix() != name
                or any(part in {".", ".."} for part in parts) or unsafe_type):
            raise GateError(f"unsafe release archive entry: {name}")
        if any(part in {"tests", "fixtures", "fixture", "synthetic"} for part in parts):
            raise GateError(f"synthetic test content cannot enter release artifacts: {name}")
    names = [name for name, _ in entries]
    if len(names) != len(set(names)):
        raise GateError("release archive contains duplicate entries")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--source-root", type=Path)
    result.add_argument("--status", type=Path)
    result.add_argument("--candidate-bundle", type=Path)
    result.add_argument("--archive", type=Path)
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        checks = 0
        if args.source_root:
            check_source_bundle(args.source_root)
            if args.status:
                check_release_report(args.status)
            checks += 1
        if args.candidate_bundle:
            check_candidate_bundle(args.candidate_bundle)
            checks += 1
        if args.archive:
            check_archive(args.archive)
            checks += 1
        if not checks:
            raise GateError("select --source-root, --candidate-bundle, or --archive")
        print(f"learning release gate passed ({checks} check group{'s' if checks != 1 else ''})")
        return 0
    except (GateError, OSError, zipfile.BadZipFile, tarfile.TarError) as exc:
        print(f"learning release gate failed: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
