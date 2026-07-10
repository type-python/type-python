from __future__ import annotations

import io
import pathlib
import tarfile
import tempfile
import unittest

from scripts import sdist_smoke


class SourceDistributionSmokeTests(unittest.TestCase):
    def test_manifest_contract_requires_rust_stdlib_templates_and_python_package(self) -> None:
        with tempfile.TemporaryDirectory(prefix="typepython-sdist-contract-test-") as tmp:
            source_root = pathlib.Path(tmp)
            for relative in sdist_smoke.REQUIRED_SDIST_PATHS:
                path = source_root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("fixture\n", encoding="utf-8")

            self.assertEqual(sdist_smoke.missing_required_paths(source_root), [])
            source_root.joinpath("stdlib/BASELINE.toml").unlink()
            self.assertEqual(
                sdist_smoke.missing_required_paths(source_root),
                ["stdlib/BASELINE.toml"],
            )

    def test_extract_sdist_rejects_path_traversal(self) -> None:
        with tempfile.TemporaryDirectory(prefix="typepython-sdist-extract-test-") as tmp:
            root = pathlib.Path(tmp)
            archive_path = root / "unsafe.tar.gz"
            with tarfile.open(archive_path, mode="w:gz") as archive:
                contents = b"escape\n"
                member = tarfile.TarInfo("package/../../escape.txt")
                member.size = len(contents)
                archive.addfile(member, io.BytesIO(contents))

            with self.assertRaisesRegex(SystemExit, "unsafe source distribution path"):
                sdist_smoke.extract_sdist(archive_path, root / "extract")
            self.assertFalse(root.joinpath("escape.txt").exists())

    def test_extract_sdist_returns_single_archive_root(self) -> None:
        with tempfile.TemporaryDirectory(prefix="typepython-sdist-extract-test-") as tmp:
            root = pathlib.Path(tmp)
            archive_path = root / "package.tar.gz"
            with tarfile.open(archive_path, mode="w:gz") as archive:
                contents = b"[build-system]\n"
                member = tarfile.TarInfo("package-1.0.0/pyproject.toml")
                member.size = len(contents)
                archive.addfile(member, io.BytesIO(contents))

            extracted = sdist_smoke.extract_sdist(archive_path, root / "extract")
            self.assertEqual(extracted, root / "extract" / "package-1.0.0")
            self.assertEqual(extracted.joinpath("pyproject.toml").read_bytes(), contents)


if __name__ == "__main__":
    unittest.main()
