from __future__ import annotations

import json
import pathlib
import tempfile
import unittest

from scripts import notebook_ingest


class NotebookIngestTests(unittest.TestCase):
    def write_notebook(self, cells: list[dict[str, object]]) -> pathlib.Path:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        path = pathlib.Path(temp_dir.name) / "analysis.ipynb"
        path.write_text(json.dumps({"cells": cells}), encoding="utf-8")
        return path

    def test_extracts_code_cells_dependencies_and_report_facts(self) -> None:
        path = self.write_notebook(
            [
                {"cell_type": "markdown", "source": "# ignored"},
                {"cell_type": "code", "source": ["import pandas as pd\n", "raw = {'id': 1}\n"]},
                {
                    "cell_type": "code",
                    "source": [
                        "df = pd.DataFrame([raw])\n",
                        "def summarize(value):\n",
                        "    return value\n",
                    ],
                },
                {"cell_type": "code", "source": "print(summarize(df))\n"},
            ]
        )

        report = notebook_ingest.analyze_notebook(path)

        self.assertEqual(len(report.cells), 3)
        self.assertEqual(report.cells[0].dict_like_records, ("raw",))
        self.assertEqual(report.cells[1].uses_previous, ("raw",))
        self.assertEqual(report.cells[1].dataframe_boundaries, ("df",))
        self.assertEqual(report.cells[1].schema_annotations, ("df: DataFrameSchema[unknown]",))
        self.assertEqual(report.cells[1].untyped_functions, ("summarize",))
        self.assertEqual(report.cells[1].implicit_globals, ())
        self.assertEqual(report.cells[2].uses_previous, ("df", "summarize"))
        self.assertEqual(report.cells[2].side_effects, ("print",))
        self.assertIn("# %% notebook cell 2", report.candidate_tpy)
        self.assertIn("def summarize(...): ...", report.pyi_preview)
        self.assertIn("df: DataFrameSchema[unknown]", report.pyi_preview)


if __name__ == "__main__":
    unittest.main()
