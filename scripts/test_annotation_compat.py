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

    def test_eval_str_fallback_merges_partial_explicit_namespaces(self) -> None:
        function_namespace: dict[str, object] = {"GlobalType": bytes}
        exec(
            "def build(value: GlobalType) -> LocalType:\n    return value\n",
            function_namespace,
            function_namespace,
        )
        build = function_namespace["build"]

        function_annotations = annotation_compat.get_annotations(
            build,
            locals={"LocalType": str},
            eval_str=True,
        )

        class Box:
            Alias = int
            value: Alias
            external: ExternalType

        class_annotations = annotation_compat.get_annotations(
            Box,
            globals={"ExternalType": str},
            eval_str=True,
        )

        self.assertEqual(function_annotations, {"value": bytes, "return": str})
        self.assertEqual(class_annotations, {"value": int, "external": str})
        explicit_empty: dict[str, object] = {}
        globalns, _ = annotation_compat._legacy_eval_namespaces(build, explicit_empty, None)
        self.assertIs(globalns, explicit_empty)

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

    def test_audit_source_flags_class_locals_in_direct_method_annotations(self) -> None:
        audit = annotation_compat.audit_source(
            "class Container:\n"
            "    Alias = int\n"
            "    value: 'Alias'\n"
            "    def direct(self, value: 'Alias') -> None:\n"
            "        def nested(item: 'Alias') -> None:\n"
            "            return None\n"
            "        return None\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A001"]
        self.assertEqual(len(findings), 1, audit.findings)
        self.assertIn("Alias", findings[0].message)

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

    def test_audit_source_detects_consumers_in_annotation_metadata(self) -> None:
        audit = annotation_compat.audit_source(
            "from fastapi import Depends as Dep\n"
            "from pydantic import Field as PField\n"
            "from typing import Annotated\n\n"
            "def load(value: Annotated[str, Dep()]) -> None:\n"
            "    return None\n\n"
            "class Model:\n"
            "    value: \"Annotated[str, PField(alias='value')]\"\n"
        )

        self.assertEqual(
            set(audit.consumers),
            {
                annotation_compat.AnnotationConsumer.FASTAPI_DEPENDS,
                annotation_compat.AnnotationConsumer.PYDANTIC_FIELD,
            },
        )

    def test_audit_source_resolves_consumer_import_aliases(self) -> None:
        audit = annotation_compat.audit_source(
            "import annotationlib as al\n"
            "import dataclasses as dc\n"
            "import inspect as ins\n"
            "import typing as t\n"
            "from typing_extensions import get_annotations as te_annotations\n"
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
            "    te_annotations(User)\n"
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

    def test_audit_source_respects_expression_scope_and_with_bindings(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import get_type_hints\n"
            "callbacks = []\n"
            "provider = lambda: None\n"
            "local = lambda get_type_hints: get_type_hints(int)\n"
            "values = [get_type_hints(int) for get_type_hints in callbacks]\n"
            "with provider() as get_type_hints:\n"
            "    get_type_hints(int)\n"
        )

        self.assertEqual(audit.consumers, ())

        outer_iterable = annotation_compat.audit_source(
            "from typing import get_type_hints\n"
            "callbacks = []\n"
            "values = [callback for get_type_hints in get_type_hints(callbacks) "
            "for callback in callbacks]\n"
        )
        self.assertEqual(
            outer_iterable.consumers,
            (annotation_compat.AnnotationConsumer.TYPING_GET_TYPE_HINTS,),
        )

    def test_audit_source_respects_global_and_nonlocal_rebindings(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING, get_type_hints\n"
            "def custom(value):\n    return value\n\n"
            "def global_scope():\n"
            "    global get_type_hints\n"
            "    get_type_hints = custom\n"
            "    get_type_hints(int)\n\n"
            "def deleted_global():\n"
            "    global get_type_hints\n"
            "    del get_type_hints\n"
            "    get_type_hints(int)\n\n"
            "def outer():\n"
            "    from typing import get_type_hints\n"
            "    def inner():\n"
            "        nonlocal get_type_hints\n"
            "        get_type_hints = custom\n"
            "        get_type_hints(int)\n"
            "    return inner\n\n"
            "def shadow_guard():\n"
            "    global TYPE_CHECKING\n"
            "    TYPE_CHECKING = True\n"
            "    def nested():\n"
            "        if TYPE_CHECKING:\n"
            "            from models import User\n"
            "        def load(item: 'User') -> None:\n"
            "            return None\n"
            "        return load\n"
            "    return nested\n"
        )

        self.assertEqual(audit.consumers, ())
        self.assertFalse(any(finding.code == "TPY-A002" for finding in audit.findings))

        before_assignment = annotation_compat.audit_source(
            "from typing import get_type_hints\n"
            "def load():\n"
            "    global get_type_hints\n"
            "    get_type_hints(int)\n"
        )
        self.assertEqual(
            before_assignment.consumers,
            (annotation_compat.AnnotationConsumer.TYPING_GET_TYPE_HINTS,),
        )

    def test_audit_source_does_not_treat_relative_imports_as_consumers(self) -> None:
        audit = annotation_compat.audit_source(
            "import typing\n"
            "from . import typing\n"
            "from .annotationlib import get_annotations as annotation_get\n"
            "from .dataclasses import dataclass\n"
            "from .fastapi import Depends, FastAPI\n"
            "from .inspect import get_annotations as inspect_get\n"
            "from .pydantic import BaseModel, Field\n"
            "from .typing import get_type_hints\n\n"
            "from .typing_extensions import get_annotations as extensions_get\n\n"
            "app = FastAPI()\n\n"
            "@dataclass\n"
            "class User(BaseModel):\n"
            "    name: str = Field()\n\n"
            "@app.get('/users')\n"
            "def load(current: str = Depends()) -> User:\n"
            "    typing.get_type_hints(User)\n"
            "    get_type_hints(User)\n"
            "    inspect_get(User)\n"
            "    annotation_get(User)\n"
            "    extensions_get(User)\n"
            "    return User(name=current)\n"
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

    def test_audit_source_flags_nested_string_forward_references(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING\n"
            "if TYPE_CHECKING:\n"
            "    from models import User\n\n"
            "def optional(value: Optional['User']) -> None:\n    return None\n"
            "def collection(value: list['User']) -> None:\n    return None\n"
            "def explicit(value: ForwardRef('User')) -> None:\n    return None\n"
            "def annotated(value: Annotated['User', 'User']) -> None:\n    return None\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A002"]
        self.assertEqual(len(findings), 4, audit.findings)
        self.assertTrue(all("User" in finding.message for finding in findings))

    def test_audit_source_ignores_string_values_in_annotation_metadata(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING\n"
            "if TYPE_CHECKING:\n"
            "    from models import User\n\n"
            "def literal(value: Literal['User']) -> None:\n    return None\n"
            "def annotated(value: Annotated[int, 'User']) -> None:\n    return None\n"
        )

        self.assertFalse(any(finding.code == "TPY-A002" for finding in audit.findings))

    def test_audit_source_flags_all_type_checking_only_bindings(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING\n"
            "if TYPE_CHECKING:\n"
            "    Alias = object\n"
            "    class Model:\n"
            "        pass\n"
            "    def Factory():\n"
            "        pass\n\n"
            "def load(alias: 'Alias', model: 'Model', factory: 'Factory') -> None:\n"
            "    return None\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A002"]
        self.assertEqual(len(findings), 3)
        rendered = "\n".join(finding.message for finding in findings)
        for name in ("Alias", "Model", "Factory"):
            self.assertIn(name, rendered)

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

    def test_late_or_conditional_runtime_binding_does_not_hide_type_only_name(self) -> None:
        audit = annotation_compat.audit_source(
            "from typing import TYPE_CHECKING, get_type_hints\n"
            "if TYPE_CHECKING:\n"
            "    from models import Group, User\n\n"
            "def load_user(item: 'User') -> None:\n    return None\n"
            "get_type_hints(load_user)\n"
            "User = object\n\n"
            "if object():\n"
            "    Group = object\n"
            "def load_group(item: 'Group') -> None:\n    return None\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A002"]
        self.assertEqual(len(findings), 2)
        self.assertTrue(any("User" in finding.message for finding in findings))
        self.assertTrue(any("Group" in finding.message for finding in findings))

    def test_conditional_control_flow_bindings_do_not_hide_type_only_names(self) -> None:
        cases = {
            "try body": "try:\n    User = object\nexcept Exception:\n    pass\n",
            "except handler": "try:\n    pass\nexcept Exception:\n    User = object\n",
            "while body": "while condition():\n    User = object\n    break\n",
            "for body": "for _ in []:\n    User = object\n",
            "short circuit": "False and (User := object)\n",
            "comprehension": "[(User := object) for _ in []]\n",
            "lambda body": "factory = lambda: (User := object)\n",
            "delete": "User = object\ndel User\n",
        }
        prefix = (
            "from typing import TYPE_CHECKING\n"
            "if TYPE_CHECKING:\n"
            "    from models import User\n"
        )
        suffix = "def load(item: 'User') -> None:\n    return None\n"

        for name, statements in cases.items():
            with self.subTest(name=name):
                audit = annotation_compat.audit_source(prefix + statements + suffix)
                findings = [
                    finding for finding in audit.findings if finding.code == "TPY-A002"
                ]
                self.assertEqual(len(findings), 1, audit.findings)
                self.assertIn("User", findings[0].message)

    def test_definite_control_flow_bindings_satisfy_type_only_names(self) -> None:
        cases = {
            "both if branches": (
                "if condition():\n    User = object\nelse:\n    User = object\n"
            ),
            "finally": "try:\n    pass\nfinally:\n    User = object\n",
            "first bool operand": "(User := object) and False\n",
        }
        prefix = (
            "from typing import TYPE_CHECKING\n"
            "if TYPE_CHECKING:\n"
            "    from models import User\n"
        )
        suffix = "def load(item: 'User') -> None:\n    return None\n"

        for name, statements in cases.items():
            with self.subTest(name=name):
                audit = annotation_compat.audit_source(prefix + statements + suffix)
                self.assertTrue(audit.safe_for_runtime_introspection, audit.findings)

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

    def test_typing_extensions_type_checking_guards_are_resolved(self) -> None:
        audit = annotation_compat.audit_source(
            "import typing_extensions as te\n"
            "from typing_extensions import TYPE_CHECKING as TC\n"
            "if te.TYPE_CHECKING:\n"
            "    from models import User\n"
            "if TC:\n"
            "    from models import Group\n\n"
            "class Container:\n"
            "    if TC:\n"
            "        from models import Item\n"
            "    item: 'Item'\n\n"
            "def load(user: 'User', group: 'Group') -> None:\n    return None\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A002"]
        self.assertEqual(len(findings), 3, audit.findings)

    def test_assigned_type_checking_guard_aliases_are_resolved(self) -> None:
        audit = annotation_compat.audit_source(
            "import typing as imported_typing\n"
            "typing_alias = imported_typing\n"
            "TC: bool = typing_alias.TYPE_CHECKING\n"
            "if TC:\n"
            "    from models import User\n"
            "TC = True\n"
            "if TC:\n"
            "    from models import Group\n\n"
            "def load(user: 'User', group: 'Group') -> None:\n    return None\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A002"]
        self.assertEqual(len(findings), 1, audit.findings)
        self.assertIn("User", findings[0].message)

    def test_function_bodies_see_late_module_type_guard_imports(self) -> None:
        audit = annotation_compat.audit_source(
            "def outer():\n"
            "    if TC:\n"
            "        from models import User\n"
            "    def load(item: User) -> None:\n"
            "        return None\n"
            "    return load\n\n"
            "from typing import TYPE_CHECKING as TC\n"
        )

        findings = [finding for finding in audit.findings if finding.code == "TPY-A002"]
        self.assertEqual(len(findings), 1, audit.findings)
        self.assertIn("User", findings[0].message)

    def test_type_checking_guards_respect_shadowing_and_import_identity(self) -> None:
        sources = {
            "unimported name": (
                "TYPE_CHECKING = True\n"
                "if TYPE_CHECKING:\n"
                "    from models import User\n"
            ),
            "shadowed typing import": (
                "from typing import TYPE_CHECKING\n"
                "TYPE_CHECKING = True\n"
                "if TYPE_CHECKING:\n"
                "    from models import User\n"
            ),
            "unrelated typing alias": (
                "import fake as typing\n"
                "if typing.TYPE_CHECKING:\n"
                "    from models import User\n"
            ),
            "function parameter": (
                "from typing import TYPE_CHECKING\n"
                "def outer(TYPE_CHECKING):\n"
                "    if TYPE_CHECKING:\n"
                "        from models import User\n"
                "    def load(item: 'User') -> None:\n"
                "        return None\n"
                "    return load\n"
            ),
        }

        for name, prefix in sources.items():
            with self.subTest(name=name):
                suffix = (
                    ""
                    if name == "function parameter"
                    else "def load(item: 'User') -> None:\n    return None\n"
                )
                audit = annotation_compat.audit_source(prefix + suffix)
                self.assertFalse(
                    any(finding.code == "TPY-A002" for finding in audit.findings),
                    audit.findings,
                )


if __name__ == "__main__":
    unittest.main()
