from __future__ import annotations

import json
import os
import pathlib
import runpy
import shutil
import subprocess
import sys
from collections.abc import Callable
from typing import cast

from setuptools import Command, Distribution, setup
from setuptools.command.build_py import build_py as _build_py

try:
    from setuptools.command.bdist_wheel import bdist_wheel as _bdist_wheel
except ImportError:
    try:
        from wheel.bdist_wheel import bdist_wheel as _bdist_wheel
    except ImportError:
        _bdist_wheel = None


ROOT = pathlib.Path(__file__).resolve().parent
macos_binary_platform_tag = cast(
    Callable[..., str],
    runpy.run_path(str(ROOT / "_typepython_build.py"))["macos_binary_platform_tag"],
)


class BinaryDistribution(Distribution):
    # Tell setuptools/wheel that this distribution contains platform-specific
    # binaries so wheel contents are laid out under platlib instead of purelib.
    def has_ext_modules(self) -> bool:
        return True


class build_py(_build_py):
    def run(self) -> None:
        super().run()
        self._copy_rust_cli()
        self._copy_bundled_stdlib()

    def _copy_rust_cli(self) -> None:
        cargo = shutil.which("cargo")
        if cargo is None:
            raise RuntimeError(
                "cargo is required to build the TypePython wheel from source. "
                "Install the workspace MSRV Rust 1.94.0 via ./scripts/bootstrap-rust.sh, "
                "or install a prebuilt type-python wheel for a supported platform."
            )

        target_dir = pathlib.Path(self.build_lib).parent / "typepython-cargo-target"
        build = subprocess.run(
            [
                cargo,
                "build",
                "--release",
                "-p",
                "typepython-cli",
                "--target-dir",
                str(target_dir),
                "--message-format=json-render-diagnostics",
            ],
            cwd=ROOT,
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        )

        binary_name = "typepython.exe" if os.name == "nt" else "typepython"
        built_binary = None
        for line in build.stdout.splitlines():
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                continue
            target = message.get("target", {})
            executable = message.get("executable")
            if (
                message.get("reason") == "compiler-artifact"
                and target.get("name") == "typepython"
                and "bin" in target.get("kind", [])
                and executable
            ):
                built_binary = pathlib.Path(executable)
        if built_binary is None or not built_binary.is_file():
            raise FileNotFoundError(
                "cargo did not report a built TypePython CLI executable for typepython-cli"
            )

        destination_dir = pathlib.Path(self.build_lib) / "typepython" / "bin"
        destination_dir.mkdir(parents=True, exist_ok=True)
        destination = destination_dir / binary_name
        shutil.copy2(built_binary, destination)
        destination.chmod(0o755)

    def _copy_bundled_stdlib(self) -> None:
        source = ROOT / "stdlib"
        if not source.joinpath("BASELINE.toml").is_file():
            raise FileNotFoundError(f"missing bundled TypePython stdlib at {source}")

        destination = pathlib.Path(self.build_lib) / "typepython" / "stdlib"
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(source, destination)


cmdclass = cast(dict[str, type[Command]], {"build_py": build_py})

if _bdist_wheel is not None:

    class bdist_wheel(_bdist_wheel):
        _typepython_tag: tuple[str, str, str] | None = None

        def finalize_options(self) -> None:
            super().finalize_options()
            self.root_is_pure = False

        def get_tag(self) -> tuple[str, str, str]:
            if self._typepython_tag is not None:
                return self._typepython_tag
            _, _, plat = super().get_tag()
            if sys.platform == "darwin":
                binary = (
                    pathlib.Path(self.bdist_dir) / "typepython" / "bin" / "typepython"
                )
                explicit = bool(getattr(self, "plat_name_supplied", False)) or (
                    "_PYTHON_HOST_PLATFORM" in os.environ
                )
                plat = macos_binary_platform_tag(plat, binary=binary, explicit=explicit)
            # The bundled Rust CLI makes the wheel platform-specific, but the
            # Python wrapper itself is not tied to a single CPython minor/ABI.
            self._typepython_tag = ("py3", "none", plat)
            return self._typepython_tag

    cmdclass["bdist_wheel"] = cast(type[Command], bdist_wheel)


setup(cmdclass=cmdclass, distclass=BinaryDistribution)
