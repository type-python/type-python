from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
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

        subprocess.run(
            [cargo, "build", "--release", "-p", "typepython-cli"],
            cwd=ROOT,
            check=True,
        )

        binary_name = "typepython.exe" if os.name == "nt" else "typepython"
        built_binary = ROOT / "target" / "release" / binary_name
        if not built_binary.is_file():
            raise FileNotFoundError(f"missing built TypePython CLI at {built_binary}")

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
        def finalize_options(self) -> None:
            super().finalize_options()
            self.root_is_pure = False

        def get_tag(self) -> tuple[str, str, str]:
            _, _, plat = super().get_tag()
            # The bundled Rust CLI makes the wheel platform-specific, but the
            # Python wrapper itself is not tied to a single CPython minor/ABI.
            return ("py3", "none", plat)

    cmdclass["bdist_wheel"] = cast(type[Command], bdist_wheel)


setup(cmdclass=cmdclass, distclass=BinaryDistribution)
