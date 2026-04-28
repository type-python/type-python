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

    def test_audit_source_detects_runtime_annotation_consumers(self) -> None:
        audit = annotation_compat.audit_source(
            "import inspect\nimport typing\nfrom dataclasses import dataclass\n"
            "@dataclass\nclass User:\n    name: str\n"
            "def inspect_user() -> None:\n    typing.get_type_hints(User)\n    inspect.get_annotations(User)\n"
        )

        self.assertEqual(
            audit.consumers,
            (
                annotation_compat.AnnotationConsumer.DATACLASS_DECORATOR,
                annotation_compat.AnnotationConsumer.INSPECT_GET_ANNOTATIONS,
                annotation_compat.AnnotationConsumer.TYPING_GET_TYPE_HINTS,
            ),
        )
        self.assertTrue(audit.safe_for_runtime_introspection)

    def test_audit_source_flags_nested_local_annotations(self) -> None:
        audit = annotation_compat.audit_source(
            "def outer():\n"
            "    class Local:\n        pass\n"
            "    def build(value: Local) -> 'Local':\n        return value\n"
            "    return build\n"
        )

        self.assertFalse(audit.safe_for_runtime_introspection)
        self.assertEqual({finding.code for finding in audit.findings}, {"TPY-A001"})
        self.assertIn("local scope", audit.findings[0].message)

    def test_audit_source_detects_fastapi_and_pydantic_consumers(self) -> None:
        audit = annotation_compat.audit_source(
            "from fastapi import Depends, FastAPI\n"
            "from pydantic import BaseModel, Field\n\n"
            "app = FastAPI()\n\n"
            "class User(BaseModel):\n"
            "    name: str = Field(alias='user_name')\n\n"
            "@app.get('/users/{name}')\n"
            "def read_user(name: str, current: str = Depends()) -> User:\n"
            "    return User(name=name)\n"
        )

        self.assertEqual(
            audit.consumers,
            (
                annotation_compat.AnnotationConsumer.FASTAPI_DEPENDS,
                annotation_compat.AnnotationConsumer.FASTAPI_ROUTE_DECORATOR,
                annotation_compat.AnnotationConsumer.PYDANTIC_BASEMODEL,
                annotation_compat.AnnotationConsumer.PYDANTIC_FIELD,
            ),
        )
        self.assertTrue(audit.safe_for_runtime_introspection)

    def test_audit_source_flags_type_checking_only_annotation_imports(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING, get_type_hints\n"
            "if TYPE_CHECKING:\n"
            "    from models import User\n\n"
            "def build(user: 'User') -> None:\n"
            "    return None\n\n"
            "get_type_hints(build)\n"
        )

        self.assertFalse(audit.safe_for_runtime_introspection)
        self.assertIn(annotation_compat.AnnotationConsumer.TYPING_GET_TYPE_HINTS, audit.consumers)
        self.assertEqual({finding.code for finding in audit.findings}, {"TPY-A002"})
        self.assertIn("TYPE_CHECKING-only", audit.findings[0].message)


if __name__ == "__main__":
    unittest.main()
