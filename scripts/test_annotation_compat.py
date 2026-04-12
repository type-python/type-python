from __future__ import annotations

import types
import unittest

from typepython import annotation_compat


class AnnotationCompatTests(unittest.TestCase):
    def test_value_annotations_work_for_functions_classes_and_modules(self) -> None:
        def f(value: int) -> str:
            return str(value)

        class Box:
            value: int

        module = types.ModuleType("demo_module")
        module.__annotations__ = {"answer": int}

        self.assertEqual(
            annotation_compat.get_annotations(f, eval_str=True),
            {"value": int, "return": str},
        )
        self.assertEqual(annotation_compat.get_annotations(Box, eval_str=True), {"value": int})
        self.assertEqual(annotation_compat.get_annotations(module), {"answer": int})

    def test_eval_str_fallback_handles_string_annotations(self) -> None:
        namespace: dict[str, object] = {}
        exec(
            "def build(value: int) -> str:\n    return str(value)\n",
            namespace,
            namespace,
        )
        build = namespace["build"]

        annotations = annotation_compat.get_annotations(build, eval_str=True)

        self.assertEqual(annotations["value"], int)
        self.assertEqual(annotations["return"], str)

    def test_non_value_formats_are_gated_without_annotationlib(self) -> None:
        support = annotation_compat.supported_formats()
        if support.forwardref and support.string:
            annotations = annotation_compat.get_annotations(
                lambda value: value,
                format=annotation_compat.AnnotationFormat.STRING,
            )
            self.assertIsInstance(annotations, dict)
            return

        with self.assertRaises(NotImplementedError):
            annotation_compat.get_annotations(
                lambda value: value,
                format=annotation_compat.AnnotationFormat.STRING,
            )


if __name__ == "__main__":
    unittest.main()
