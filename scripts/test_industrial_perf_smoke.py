from __future__ import annotations

import json
import pathlib
import tempfile
import unittest

from scripts import industrial_perf_smoke


class IndustrialPerfSmokeTests(unittest.TestCase):
    def test_create_workspace_generates_large_fixture_shape(self) -> None:
        with tempfile.TemporaryDirectory(prefix="typepython-industrial-perf-test-") as tmp:
            root = pathlib.Path(tmp)
            project = industrial_perf_smoke.create_workspace(
                root,
                industrial_perf_smoke.WorkspaceOptions(
                    modules=4,
                    external_stubs=3,
                    target_python="3.13",
                ),
            )

            config = project.joinpath("typepython.toml").read_text(encoding="utf-8")
            self.assertIn('target_python = "3.13"', config)
            self.assertIn('type_roots = ["typestubs"]', config)
            self.assertIn("python_executable =", config)

            self.assertTrue(project.joinpath("src/app/mod_0000.tpy").is_file())
            leaf = project.joinpath("src/app/mod_0003.tpy").read_text(encoding="utf-8")
            self.assertIn("from app.mod_0002 import value_0002", leaf)
            self.assertIn("def consume_0003(values: list[int]) -> int:", leaf)

            self.assertTrue(project.joinpath("typestubs/vendor_0002/__init__.pyi").is_file())
            self.assertTrue(
                project.joinpath("typestubs/namespace_pkg/service_0002/__init__.pyi").is_file()
            )
            self.assertEqual(
                project.joinpath("typestubs/framework-stubs/py.typed").read_text(
                    encoding="utf-8"
                ),
                "partial\n",
            )

    def test_payload_serializes_steps_and_fixture_metadata(self) -> None:
        step = industrial_perf_smoke.TimedStep(
            label="warm_check",
            command=["typepython", "check"],
            seconds=0.25,
            return_code=0,
            peak_rss_bytes=1024,
            stdout_bytes=10,
            stderr_bytes=0,
        )
        payload = industrial_perf_smoke.build_payload(
            pathlib.Path("/tmp/workspace"),
            industrial_perf_smoke.WorkspaceOptions(
                modules=16,
                external_stubs=8,
                target_python="3.12",
            ),
            "check",
            [step],
        )

        rendered = json.dumps(payload)
        self.assertIn("warm_check", rendered)
        self.assertEqual(payload["modules"], 16)
        self.assertEqual(payload["external_stubs"], 8)
        self.assertEqual(payload["steps"][0]["peak_rss_bytes"], 1024)


if __name__ == "__main__":
    unittest.main()
