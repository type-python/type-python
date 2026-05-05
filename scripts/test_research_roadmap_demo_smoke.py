from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest import mock


SCRIPT_PATH = pathlib.Path(__file__).with_name("research_roadmap_demo_smoke.py")
SPEC = importlib.util.spec_from_file_location("research_roadmap_demo_smoke", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
research_roadmap_demo_smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(research_roadmap_demo_smoke)


class ResearchRoadmapDemoSmokeTests(unittest.TestCase):
    def test_assert_demo_outputs_requires_portable_projection_outputs(self) -> None:
        with tempfile.TemporaryDirectory(prefix="research-roadmap-demo-smoke-") as tmp:
            project = pathlib.Path(tmp)
            build_dir = project / ".typepython" / "build" / "app"
            cache_dir = project / ".typepython" / "cache"
            build_dir.mkdir(parents=True)
            cache_dir.mkdir(parents=True)
            portable = (
                "class UserPatch(TypedDict):\n"
                "    id: NotRequired[int]\n\n"
                "class PublicUser(TypedDict):\n"
                "    id: int\n"
            )
            (build_dir / "__init__.py").write_text(portable, encoding="utf-8")
            (build_dir / "__init__.pyi").write_text(portable, encoding="utf-8")
            (build_dir / "py.typed").write_text("", encoding="utf-8")
            (cache_dir / "effects.json").write_text(
                json.dumps(
                    {
                        "modules": [
                            {
                                "effectSummaries": [
                                    {
                                        "name": "load_user",
                                        "effects": ["io.net", "taint.source"],
                                    },
                                    {
                                        "name": "handle",
                                        "effects": ["taint.sanitize"],
                                    },
                                ]
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )

            research_roadmap_demo_smoke.assert_demo_outputs(project)

    def test_main_runs_check_build_then_validates_outputs(self) -> None:
        commands: list[list[str]] = []

        with (
            mock.patch.object(
                research_roadmap_demo_smoke,
                "typepython_command",
                return_value=["/fake/typepython"],
            ),
            mock.patch.object(
                research_roadmap_demo_smoke,
                "run",
                side_effect=lambda command: commands.append(command),
            ),
            mock.patch.object(research_roadmap_demo_smoke, "assert_demo_outputs") as validate,
        ):
            research_roadmap_demo_smoke.main()

        self.assertEqual(
            commands,
            [
                [
                    "/fake/typepython",
                    "check",
                    "--project",
                    str(research_roadmap_demo_smoke.PROJECT),
                ],
                [
                    "/fake/typepython",
                    "build",
                    "--project",
                    str(research_roadmap_demo_smoke.PROJECT),
                ],
            ],
        )
        validate.assert_called_once_with()


if __name__ == "__main__":
    unittest.main()
