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

    def test_audit_source_ignores_eager_and_builtin_nested_annotations(self) -> None:
        audit = annotation_compat.audit_source(
            "class Box:\n"
            "    value: int\n"
            "    def render(self, value: int) -> str:\n        return str(value)\n\n"
            "def outer():\n"
            "    class Local:\n        pass\n"
            "    def build(value: Local) -> Local:\n        return value\n"
            "    return build\n"
        )

        self.assertTrue(audit.safe_for_runtime_introspection)

    def test_audit_source_flags_future_deferred_local_names(self) -> None:
        audit = annotation_compat.audit_source(
            "from __future__ import annotations\n\n"
            "def outer():\n"
            "    class Local:\n        pass\n"
            "    def build(value: Local) -> Local:\n        return value\n"
            "    return build\n"
        )

        self.assertFalse(audit.safe_for_runtime_introspection)
        self.assertEqual({finding.code for finding in audit.findings}, {"TPY-A001"})
        self.assertTrue(all("Local" in finding.message for finding in audit.findings))

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

    def test_audit_source_resolves_consumer_import_aliases(self) -> None:
        audit = annotation_compat.audit_source(
            "import annotationlib as al\n"
            "import dataclasses as dc\n"
            "import inspect as ins\n"
            "import typing as t\n"
            "from fastapi import Depends as Dep, FastAPI as API\n"
            "from pydantic import BaseModel as Model, Field as PField\n\n"
            "app = API()\n\n"
            "@dc.dataclass\n"
            "class Record:\n    name: str\n\n"
            "class User(Model):\n    name: str = PField()\n\n"
            "@app.post('/users')\n"
            "def load(current: str = Dep()) -> User:\n"
            "    t.get_type_hints(User)\n"
            "    ins.get_annotations(User)\n"
            "    al.get_annotations(User)\n"
            "    return User(name=current)\n"
        )

        self.assertEqual(
            set(audit.consumers),
            set(annotation_compat.AnnotationConsumer),
        )

    def test_audit_source_does_not_guess_consumers_from_unrelated_names(self) -> None:
        audit = annotation_compat.audit_source(
            "def get_type_hints(value):\n    return value\n"
            "def Depends():\n    return None\n"
            "def Field():\n    return None\n"
            "def dataclass(value):\n    return value\n\n"
            "class BaseModel:\n    pass\n\n"
            "class App:\n"
            "    def get(self, path):\n"
            "        return lambda value: value\n\n"
            "app = App()\n\n"
            "@dataclass\n"
            "class User(BaseModel):\n"
            "    name: str = Field()\n\n"
            "@app.get('/users')\n"
            "def load(current: str = Depends()) -> User:\n"
            "    get_type_hints(User)\n"
            "    return User()\n"
        )

        self.assertEqual(audit.consumers, ())

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

    def test_type_checking_imports_follow_lexical_scope(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING\n\n"
            "class Container:\n"
            "    if TYPE_CHECKING:\n"
            "        from models import User as ClassUser\n"
            "    item: 'ClassUser'\n"
            "    def build(self, item: 'ClassUser') -> None:\n        return None\n\n"
            "def first():\n"
            "    if TYPE_CHECKING:\n"
            "        from models import User as LocalUser\n"
            "    local: 'LocalUser'\n"
            "    def build(item: 'LocalUser') -> None:\n        return None\n"
            "    return build\n\n"
            "def outside(item: 'ClassUser') -> None:\n    return None\n"
        )

        type_checking_findings = [
            finding for finding in audit.findings if finding.code == "TPY-A002"
        ]
        self.assertEqual(len(type_checking_findings), 3)
        self.assertEqual(
            sum("ClassUser" in finding.message for finding in type_checking_findings),
            2,
        )
        self.assertEqual(
            sum("LocalUser" in finding.message for finding in type_checking_findings),
            1,
        )

    def test_module_type_checking_imports_cover_module_class_and_function_annotations(
        self,
    ) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING\n"
            "if TYPE_CHECKING:\n"
            "    from models import User\n\n"
            "current: 'User'\n\n"
            "class Container:\n"
            "    item: 'User'\n"
            "    def build(self, item: 'User') -> None:\n        return None\n\n"
            "def load(item: 'User') -> None:\n    return None\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A002"]
        self.assertEqual(len(findings), 4)

    def test_runtime_binding_satisfies_type_checking_import_name(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING\n"
            "if TYPE_CHECKING:\n"
            "    from models import User\n"
            "User = object\n\n"
            "def load(item: 'User') -> None:\n    return None\n"
        )

        self.assertTrue(audit.safe_for_runtime_introspection)

    def test_type_checking_guard_import_aliases_are_resolved(self) -> None:
        audit = annotation_compat.audit_source(
            "import typing as t\n"
            "from typing import TYPE_CHECKING as TC\n"
            "if t.TYPE_CHECKING:\n"
            "    from models import User\n"
            "if TC:\n"
            "    from models import Group\n\n"
            "def load(user: 'User', group: 'Group') -> None:\n    return None\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A002"]
        self.assertEqual(len(findings), 2)


if __name__ == "__main__":
    unittest.main()
