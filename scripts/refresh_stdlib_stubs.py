from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys
from collections.abc import Iterable


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
STDLIB_ROOT = REPO_ROOT / "stdlib"
BASELINE_PATH = STDLIB_ROOT / "BASELINE.toml"
REFRESH_STATS_PATH = STDLIB_ROOT / "REFRESH_STATS.json"
VERSIONS_PATH = STDLIB_ROOT / "VERSIONS"
DEFAULT_TYPESHED_STDLIB = pathlib.Path("stdlib")
LOCAL_PATCH_ROOT = REPO_ROOT / "stdlib-patches"
PIN_RE = re.compile(r'(?m)^typeshed_commit\s*=\s*"([0-9a-f]{40})"\s*$')
VERSION_LINE_RE = re.compile(r"^(?P<module>[A-Za-z0-9_\.]+):\s+(?P<range>\d+\.\d+-(?:\d+\.\d+)?)")
SUPPORT_PACKAGE_ALLOWLIST = {"numpy", "pandas", "requests", "torch"}


@dataclasses.dataclass(frozen=True)
class SnapshotStats:
    files: int
    bytes: int
    sha256: str
    version_entries: int
    missing_version_entries: tuple[str, ...]
    stale_version_entries: tuple[str, ...]

    def to_json(self, *, typeshed_commit: str) -> str:
        payload = {
            "schema_version": 1,
            "typeshed_commit": typeshed_commit,
            "stdlib_files": self.files,
            "stdlib_bytes": self.bytes,
            "stdlib_sha256": self.sha256,
            "version_entries": self.version_entries,
            "missing_version_entries": list(self.missing_version_entries),
            "stale_version_entries": list(self.stale_version_entries),
        }
        return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def read_typeshed_commit() -> str:
    baseline = BASELINE_PATH.read_text(encoding="utf-8")
    match = PIN_RE.search(baseline)
    if match is None:
        raise SystemExit(
            f"{BASELINE_PATH.relative_to(REPO_ROOT)} must contain "
            'bundled_stdlib.typeshed_commit = "<40 hex chars>"'
        )
    return match.group(1)


def git_commit_at(path: pathlib.Path) -> str:
    result = subprocess.run(
        ["git", "-C", str(path), "rev-parse", "HEAD"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return result.stdout.strip()


def iter_stdlib_files(root: pathlib.Path) -> Iterable[pathlib.Path]:
    for path in sorted(root.rglob("*")):
        if path.is_file() and path.name != ".DS_Store":
            yield path


def module_name_for_stub(path: pathlib.Path) -> str | None:
    if path.suffix != ".pyi":
        return None
    relative = path.relative_to(STDLIB_ROOT).with_suffix("")
    parts = relative.parts
    if not parts:
        return None
    if parts[-1] == "__init__":
        parts = parts[:-1]
    if not parts:
        return None
    return ".".join(parts)


def requires_version_entry(module: str) -> bool:
    root = module.split(".", 1)[0]
    return root not in SUPPORT_PACKAGE_ALLOWLIST


def version_entries() -> set[str]:
    entries: set[str] = set()
    for line in VERSIONS_PATH.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        match = VERSION_LINE_RE.match(stripped)
        if match is None:
            raise SystemExit(f"invalid stdlib/VERSIONS line: {line}")
        entries.add(match.group("module"))
    return entries


def snapshot_stats() -> SnapshotStats:
    digest = hashlib.sha256()
    file_count = 0
    byte_count = 0
    modules: set[str] = set()
    for path in iter_stdlib_files(STDLIB_ROOT):
        if path in {BASELINE_PATH, REFRESH_STATS_PATH, VERSIONS_PATH}:
            continue
        relative = path.relative_to(STDLIB_ROOT).as_posix()
        contents = path.read_bytes()
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        digest.update(contents)
        file_count += 1
        byte_count += len(contents)
        module = module_name_for_stub(path)
        if module is not None:
            modules.add(module)

    entries = version_entries()
    missing = tuple(
        sorted(
            module
            for module in modules
            if requires_version_entry(module)
            and module not in entries
            and not has_parent_module(module, entries)
        )
    )
    stale = tuple(sorted(entry for entry in entries if entry not in modules and not has_parent_module(entry, modules)))
    return SnapshotStats(
        files=file_count,
        bytes=byte_count,
        sha256=digest.hexdigest(),
        version_entries=len(entries),
        missing_version_entries=missing,
        stale_version_entries=stale,
    )


def has_parent_module(entry: str, modules: set[str]) -> bool:
    parts = entry.split(".")
    return any(".".join(parts[:index]) in modules for index in range(1, len(parts)))


def copy_typeshed_stdlib(typeshed_root: pathlib.Path) -> None:
    source_root = typeshed_root / DEFAULT_TYPESHED_STDLIB
    if not source_root.is_dir():
        raise SystemExit(f"typeshed stdlib directory not found: {source_root}")

    for path in list(iter_stdlib_files(STDLIB_ROOT)):
        if path in {BASELINE_PATH, REFRESH_STATS_PATH}:
            continue
        path.unlink()
    for directory in sorted((path for path in STDLIB_ROOT.rglob("*") if path.is_dir()), reverse=True):
        if directory == STDLIB_ROOT:
            continue
        try:
            directory.rmdir()
        except OSError:
            pass

    for source in sorted(source_root.rglob("*")):
        if source.is_dir():
            continue
        relative = source.relative_to(source_root)
        target = STDLIB_ROOT / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)


def apply_local_patches() -> None:
    if not LOCAL_PATCH_ROOT.is_dir():
        return
    for patch in sorted(LOCAL_PATCH_ROOT.glob("*.patch")):
        subprocess.run(["git", "apply", str(patch)], cwd=REPO_ROOT, check=True)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Refresh or validate the bundled stdlib stubs from a pinned typeshed checkout.",
    )
    parser.add_argument("--typeshed-root", type=pathlib.Path, help="local typeshed checkout")
    parser.add_argument("--write", action="store_true", help="copy stubs and rewrite refresh stats")
    parser.add_argument("--check", action="store_true", help="fail if baseline stats are stale")
    args = parser.parse_args()

    pinned_commit = read_typeshed_commit()
    if args.typeshed_root is not None:
        actual_commit = git_commit_at(args.typeshed_root)
        if actual_commit != pinned_commit:
            raise SystemExit(
                f"typeshed checkout is at {actual_commit}, but stdlib/BASELINE.toml pins {pinned_commit}"
            )
        if args.write:
            copy_typeshed_stdlib(args.typeshed_root)
            apply_local_patches()

    stats = snapshot_stats()
    rendered = stats.to_json(typeshed_commit=pinned_commit)
    if stats.missing_version_entries:
        missing = ", ".join(stats.missing_version_entries[:10])
        raise SystemExit(f"stdlib/VERSIONS is missing {len(stats.missing_version_entries)} module(s): {missing}")

    if args.write:
        REFRESH_STATS_PATH.write_text(rendered, encoding="utf-8")
        return 0

    if args.check:
        try:
            expected = REFRESH_STATS_PATH.read_text(encoding="utf-8")
        except FileNotFoundError as error:
            raise SystemExit("stdlib/REFRESH_STATS.json is missing; run refresh with --write") from error
        if expected != rendered:
            raise SystemExit("stdlib/REFRESH_STATS.json is stale; run scripts/refresh_stdlib_stubs.py --write")

    sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
