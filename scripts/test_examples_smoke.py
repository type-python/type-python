from __future__ import annotations

import importlib.util
import pathlib
import tempfile
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("examples_smoke.py")
SPEC = importlib.util.spec_from_file_location("examples_smoke", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
examples_smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(examples_smoke)


class ExamplesSmokeTests(unittest.TestCase):
    def test_assert_example_outputs_requires_expected_stub_fragments(self) -> None:
        with tempfile.TemporaryDirectory(prefix="examples-smoke-test-") as tmp:
            project = pathlib.Path(tmp)
            build_root = project / ".typepython" / "build" / "app"
            build_root.mkdir(parents=True)
            (build_root / "__init__.py").write_text(
                "def greet(name: str) -> str:\n    return name\n",
                encoding="utf-8",
            )
            (build_root / "__init__.pyi").write_text(
                "def greet(name: str) -> str: ...\n",
                encoding="utf-8",
            )
            (build_root / "py.typed").write_text("", encoding="utf-8")

            examples_smoke.assert_example_outputs(project, "hello-world")

    def test_assert_example_outputs_rejects_typepython_only_runtime_syntax(self) -> None:
        with tempfile.TemporaryDirectory(prefix="examples-smoke-test-") as tmp:
            project = pathlib.Path(tmp)
            build_root = project / ".typepython" / "build" / "app"
            build_root.mkdir(parents=True)
            (build_root / "__init__.py").write_text(
                "interface Service:\n    pass\n",
                encoding="utf-8",
            )
            (build_root / "__init__.pyi").write_text(
                "def greet(name: str) -> str: ...\n",
                encoding="utf-8",
            )
            (build_root / "py.typed").write_text("", encoding="utf-8")

            with self.assertRaises(SystemExit):
                examples_smoke.assert_example_outputs(project, "hello-world")


if __name__ == "__main__":
    unittest.main()
