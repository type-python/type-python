from __future__ import annotations

import pathlib
import re
import subprocess
from collections.abc import Collection


_MACOS_PLATFORM_TAG = re.compile(r"^macosx_(\d+)_(\d+)_(.+)$")
_MACOS_ARCHITECTURES = {
    frozenset({"arm64"}): "arm64",
    frozenset({"x86_64"}): "x86_64",
    frozenset({"arm64", "x86_64"}): "universal2",
}


def resolve_macos_platform_tag(
    requested_tag: str,
    *,
    actual_arches: Collection[str],
    actual_minimum: tuple[int, ...],
    explicit: bool,
) -> str:
    """Resolve a wheel tag from the actual bundled Mach-O slices and deployment floor."""

    match = _MACOS_PLATFORM_TAG.fullmatch(requested_tag)
    if match is None:
        raise RuntimeError(f"unsupported macOS wheel platform tag: {requested_tag}")

    requested_version = (int(match.group(1)), int(match.group(2)))
    requested_arch = match.group(3)
    actual_arch_set = frozenset(actual_arches)
    actual_arch = _MACOS_ARCHITECTURES.get(actual_arch_set)
    if actual_arch is None:
        rendered = ", ".join(sorted(actual_arch_set)) or "none"
        raise RuntimeError(
            "bundled TypePython CLI has unsupported Mach-O architectures: " + rendered
        )

    actual_version = _normalized_macos_version(actual_minimum)
    if explicit and requested_arch != actual_arch:
        if requested_arch == "universal2":
            detail = "universal2 requires both arm64 and x86_64 slices"
        else:
            detail = (
                f"requested {requested_arch}, but the binary contains {actual_arch}"
            )
        raise RuntimeError(
            f"requested macOS wheel tag {requested_tag} does not match the bundled "
            f"TypePython CLI ({detail}); Cargo builds the selected Rust target, not ARCHFLAGS"
        )

    version = max(requested_version, actual_version) if explicit else actual_version
    arch = requested_arch if explicit else actual_arch
    return f"macosx_{version[0]}_{version[1]}_{arch}"


def macos_binary_platform_tag(
    requested_tag: str,
    *,
    binary: pathlib.Path,
    explicit: bool,
) -> str:
    """Inspect a built TypePython CLI and return its truthful macOS wheel tag."""

    if not binary.is_file():
        raise FileNotFoundError(
            f"missing bundled TypePython CLI for wheel tagging: {binary}"
        )

    try:
        arches = subprocess.run(
            ["lipo", "-archs", str(binary)],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.split()
    except (OSError, subprocess.CalledProcessError) as error:
        raise RuntimeError(
            f"unable to inspect Mach-O architectures for {binary}: {error}"
        ) from error

    try:
        from wheel.macosx_libfile import extract_macosx_min_system_version

        minimum = extract_macosx_min_system_version(str(binary))
    except (ImportError, OSError, ValueError) as error:
        raise RuntimeError(
            f"unable to inspect Mach-O deployment target for {binary}: {error}"
        ) from error
    if minimum is None:
        raise RuntimeError(
            f"bundled TypePython CLI is not a supported Mach-O binary: {binary}"
        )

    return resolve_macos_platform_tag(
        requested_tag,
        actual_arches=arches,
        actual_minimum=tuple(minimum),
        explicit=explicit,
    )


def _normalized_macos_version(version: tuple[int, ...]) -> tuple[int, int]:
    if len(version) < 2:
        raise RuntimeError(f"invalid Mach-O deployment target: {version!r}")
    major, minor = version[:2]
    return (major, 0) if major > 10 else (major, minor)
