from __future__ import annotations

import importlib.util
import pathlib
import tempfile
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("flagship_core_smoke.py")
SPEC = importlib.util.spec_from_file_location("flagship_core_smoke", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
flagship_core_smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(flagship_core_smoke)


class FlagshipCoreSmokeTests(unittest.TestCase):
    def test_assert_flagship_outputs_requires_core_fragments_and_runtime_erasure(self) -> None:
        with tempfile.TemporaryDirectory(prefix="flagship-core-smoke-test-") as tmp:
            project = pathlib.Path(tmp)
            build_root = project / ".typepython" / "build" / "app"
            build_root.mkdir(parents=True)
            (build_root / "__init__.py").write_text(
                "class LookupResult:\n    pass\n",
                encoding="utf-8",
            )
            (build_root / "__init__.pyi").write_text(
                "\n".join(flagship_core_smoke.EXPECTED_STUB_FRAGMENTS),
                encoding="utf-8",
            )
            (build_root / "py.typed").write_text("", encoding="utf-8")

            flagship_core_smoke.assert_flagship_outputs(project)

    def test_assert_flagship_outputs_rejects_typepython_only_runtime_text(self) -> None:
        with tempfile.TemporaryDirectory(prefix="flagship-core-smoke-test-") as tmp:
            project = pathlib.Path(tmp)
            build_root = project / ".typepython" / "build" / "app"
            build_root.mkdir(parents=True)
            (build_root / "__init__.py").write_text(
                "sealed class LookupResult:\n    pass\n",
                encoding="utf-8",
            )
            (build_root / "__init__.pyi").write_text(
                "\n".join(flagship_core_smoke.EXPECTED_STUB_FRAGMENTS),
                encoding="utf-8",
            )
            (build_root / "py.typed").write_text("", encoding="utf-8")

            with self.assertRaises(SystemExit):
                flagship_core_smoke.assert_flagship_outputs(project)


if __name__ == "__main__":
    unittest.main()
