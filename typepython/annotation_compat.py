from __future__ import annotations

import ast
import inspect
import sys
from dataclasses import dataclass
from enum import Enum
from types import ModuleType
from typing import Any

try:
    import annotationlib as _annotationlib
except ImportError:  # pragma: no cover - exercised on pre-3.14 hosts
    _annotationlib = None

HAS_ANNOTATIONLIB = _annotationlib is not None


class AnnotationFormat(str, Enum):
    VALUE = "value"
    FORWARDREF = "forwardref"
    STRING = "string"


@dataclass(frozen=True)
class AnnotationSupport:
    value: bool
    forwardref: bool
    string: bool


class AnnotationConsumer(str, Enum):
    TYPING_GET_TYPE_HINTS = "typing.get_type_hints"
    INSPECT_GET_ANNOTATIONS = "inspect.get_annotations"
    ANNOTATIONLIB_GET_ANNOTATIONS = "annotationlib.get_annotations"
    DATACLASS_DECORATOR = "dataclasses.dataclass"
    FASTAPI_ROUTE_DECORATOR = "fastapi.route_decorator"
    FASTAPI_DEPENDS = "fastapi.Depends"
    PYDANTIC_BASEMODEL = "pydantic.BaseModel"
    PYDANTIC_FIELD = "pydantic.Field"


@dataclass(frozen=True)
class AnnotationAuditFinding:
    code: str
    message: str
    line: int
    column: int


@dataclass(frozen=True)
class AnnotationAudit:
    consumers: tuple[AnnotationConsumer, ...]
    findings: tuple[AnnotationAuditFinding, ...]

    @property
    def safe_for_runtime_introspection(self) -> bool:
        return not self.findings


@dataclass
class _AuditScope:
    kind: str
    runtime_names: frozenset[str]
    type_checking_only_names: frozenset[str]
    resolved_names: dict[str, str]


def supported_formats() -> AnnotationSupport:
    if HAS_ANNOTATIONLIB:
        return AnnotationSupport(value=True, forwardref=True, string=True)
    return AnnotationSupport(value=True, forwardref=False, string=False)


def audit_source(source: str, *, filename: str = "<source>") -> AnnotationAudit:
    tree = ast.parse(source, filename=filename)
    visitor = _AnnotationAuditVisitor(
        future_annotations=_uses_future_annotations(tree),
    )
    visitor.visit(tree)
    return AnnotationAudit(
        consumers=tuple(sorted(visitor.consumers, key=lambda consumer: consumer.value)),
        findings=tuple(visitor.findings),
    )


def get_annotations(
    obj: Any,
    *,
    globals: dict[str, Any] | None = None,
    locals: dict[str, Any] | None = None,
    eval_str: bool = False,
    format: AnnotationFormat | str = AnnotationFormat.VALUE,
) -> dict[str, Any]:
    normalized = _normalize_format(format)
    if HAS_ANNOTATIONLIB:
        return _annotationlib.get_annotations(
            obj,
            globals=globals,
            locals=locals,
            eval_str=eval_str,
            format=_annotationlib_format(normalized),
        )
    if normalized is not AnnotationFormat.VALUE:
        raise NotImplementedError(
            "annotation formats other than VALUE require Python 3.14+ annotationlib"
        )
    return _fallback_get_annotations(
        obj,
        globals=globals,
        locals=locals,
        eval_str=eval_str,
    )


def _normalize_format(format: AnnotationFormat | str) -> AnnotationFormat:
    if isinstance(format, AnnotationFormat):
        return format
    return AnnotationFormat(format)


def _annotationlib_format(format: AnnotationFormat) -> Any:
    assert _annotationlib is not None
    if format is AnnotationFormat.VALUE:
        return _annotationlib.Format.VALUE
    if format is AnnotationFormat.FORWARDREF:
        return _annotationlib.Format.FORWARDREF
    return _annotationlib.Format.STRING


def _fallback_get_annotations(
    obj: Any,
    *,
    globals: dict[str, Any] | None,
    locals: dict[str, Any] | None,
    eval_str: bool,
) -> dict[str, Any]:
    if hasattr(inspect, "get_annotations"):
        return inspect.get_annotations(
            obj,
            globals=globals,
            locals=locals,
            eval_str=eval_str,
        )

    raw = _legacy_raw_annotations(obj)
    if raw is None:
        return {}
    annotations = dict(raw)
    if not eval_str:
        return annotations

    globalns, localns = _legacy_eval_namespaces(obj, globals, locals)
    evaluated: dict[str, Any] = {}
    for name, value in annotations.items():
        if isinstance(value, str):
            evaluated[name] = eval(value, globalns, localns)
        else:
            evaluated[name] = value
    return evaluated


def _legacy_raw_annotations(obj: Any) -> dict[str, Any] | None:
    if isinstance(obj, type):
        return obj.__dict__.get("__annotations__")
    if isinstance(obj, ModuleType):
        return getattr(obj, "__annotations__", None)
    return getattr(obj, "__annotations__", None)


def _legacy_eval_namespaces(
    obj: Any,
    globals: dict[str, Any] | None,
    locals: dict[str, Any] | None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    if globals is not None or locals is not None:
        return globals or {}, locals or globals or {}

    if isinstance(obj, ModuleType):
        namespace = vars(obj)
        return namespace, namespace
    if isinstance(obj, type):
        module = sys.modules.get(getattr(obj, "__module__", ""))
        globalns = vars(module) if module is not None else {}
        return globalns, dict(vars(obj))

    globalns = getattr(obj, "__globals__", None)
    if globalns is not None:
        return globalns, globalns
    module = sys.modules.get(getattr(obj, "__module__", ""))
    namespace = vars(module) if module is not None else {}
    return namespace, namespace


class _AnnotationAuditVisitor(ast.NodeVisitor):
    def __init__(self, *, future_annotations: bool) -> None:
        self.consumers: set[AnnotationConsumer] = set()
        self.findings: list[AnnotationAuditFinding] = []
        self._future_annotations = future_annotations
        self._scopes: list[_AuditScope] = []

    def visit_Module(self, node: ast.Module) -> None:
        self._visit_scope("module", node.body)

    def visit_If(self, node: ast.If) -> None:
        if (
            self._canonical_name(node.test) == "typing.TYPE_CHECKING"
            or _is_type_checking_guard(node.test)
        ):
            for statement in node.orelse:
                self.visit(statement)
            return
        self.generic_visit(node)

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        for decorator in node.decorator_list:
            consumer = self._decorator_consumer(decorator)
            if consumer is not None:
                self.consumers.add(consumer)
        self._record_annotation_findings(_callable_annotations(node))
        self._visit_definition_expressions(node)
        self._visit_scope("function", node.body, parameters=_parameter_names(node))
        self._bind_runtime_name(node.name, None)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        for decorator in node.decorator_list:
            consumer = self._decorator_consumer(decorator)
            if consumer is not None:
                self.consumers.add(consumer)
        self._record_annotation_findings(_callable_annotations(node))
        self._visit_definition_expressions(node)
        self._visit_scope("function", node.body, parameters=_parameter_names(node))
        self._bind_runtime_name(node.name, None)

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        if any(self._canonical_name(base) == "pydantic.BaseModel" for base in node.bases):
            self.consumers.add(AnnotationConsumer.PYDANTIC_BASEMODEL)
        for decorator in node.decorator_list:
            consumer = self._decorator_consumer(decorator)
            if consumer is not None:
                self.consumers.add(consumer)
        for expression in [*node.decorator_list, *node.bases]:
            self.visit(expression)
        for keyword in node.keywords:
            self.visit(keyword.value)
        self._visit_scope("class", node.body)
        self._bind_runtime_name(node.name, None)

    def visit_Import(self, node: ast.Import) -> None:
        for alias in node.names:
            local_name = alias.asname or alias.name.split(".", 1)[0]
            canonical_name = alias.name if alias.asname else alias.name.split(".", 1)[0]
            self._bind_runtime_name(local_name, canonical_name)

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        if node.module is None:
            return
        for alias in node.names:
            if alias.name == "*":
                continue
            self._bind_runtime_name(
                alias.asname or alias.name,
                f"{node.module}.{alias.name}",
            )

    def visit_Assign(self, node: ast.Assign) -> None:
        self.visit(node.value)
        resolved = self._assigned_value_name(node.value)
        for target in node.targets:
            self._bind_assignment_target(target, resolved)

    def visit_AnnAssign(self, node: ast.AnnAssign) -> None:
        if self._scopes and self._scopes[-1].kind in {"module", "class"}:
            self._record_annotation_findings([node.annotation])
        if node.value is not None:
            self.visit(node.value)
            self._bind_assignment_target(node.target, self._assigned_value_name(node.value))

    def visit_AugAssign(self, node: ast.AugAssign) -> None:
        self.visit(node.value)
        self._bind_assignment_target(node.target, None)

    def visit_NamedExpr(self, node: ast.NamedExpr) -> None:
        self.visit(node.value)
        self._bind_assignment_target(node.target, self._assigned_value_name(node.value))

    def visit_Call(self, node: ast.Call) -> None:
        consumer = _call_consumer(self._canonical_name(node.func))
        if consumer is not None:
            self.consumers.add(consumer)
        self.generic_visit(node)

    def _visit_definition_expressions(
        self,
        node: ast.FunctionDef | ast.AsyncFunctionDef,
    ) -> None:
        for expression in node.decorator_list:
            self.visit(expression)
        for default in [*node.args.defaults, *node.args.kw_defaults]:
            if default is not None:
                self.visit(default)

    def _visit_scope(
        self,
        kind: str,
        statements: list[ast.stmt],
        *,
        parameters: set[str] | None = None,
    ) -> None:
        runtime_names, type_checking_only_names = _scope_names(statements)
        runtime_names.update(parameters or ())
        self._scopes.append(
            _AuditScope(
                kind=kind,
                runtime_names=frozenset(runtime_names),
                type_checking_only_names=frozenset(type_checking_only_names),
                resolved_names={},
            )
        )
        for statement in statements:
            self.visit(statement)
        self._scopes.pop()

    def _canonical_name(self, expression: ast.expr) -> str | None:
        dotted = _dotted_name(expression)
        if dotted is None:
            return None
        root, separator, suffix = dotted.partition(".")
        inside_function = bool(self._scopes and self._scopes[-1].kind == "function")
        for scope in reversed(self._scopes):
            if inside_function and scope.kind == "class":
                continue
            if root not in scope.runtime_names:
                continue
            resolved_root = scope.resolved_names.get(root)
            if resolved_root is None:
                return None
            return resolved_root if not separator else f"{resolved_root}.{suffix}"
        return None

    def _assigned_value_name(self, value: ast.expr) -> str | None:
        if isinstance(value, ast.Call):
            factory = self._canonical_name(value.func)
            if factory in {"fastapi.FastAPI", "fastapi.APIRouter"}:
                return f"{factory}.instance"
            return None
        return self._canonical_name(value)

    def _bind_assignment_target(self, target: ast.expr, resolved: str | None) -> None:
        if isinstance(target, ast.Name):
            self._bind_runtime_name(target.id, resolved)
            return
        if isinstance(target, (ast.List, ast.Tuple)):
            for element in target.elts:
                self._bind_assignment_target(element, None)

    def _bind_runtime_name(self, name: str, resolved: str | None) -> None:
        if not self._scopes:
            return
        if resolved is None:
            self._scopes[-1].resolved_names.pop(name, None)
        else:
            self._scopes[-1].resolved_names[name] = resolved

    def _decorator_consumer(self, decorator: ast.expr) -> AnnotationConsumer | None:
        expression = decorator.func if isinstance(decorator, ast.Call) else decorator
        canonical = self._canonical_name(expression)
        if canonical == "dataclasses.dataclass":
            return AnnotationConsumer.DATACLASS_DECORATOR
        if _is_fastapi_route_decorator_name(canonical):
            return AnnotationConsumer.FASTAPI_ROUTE_DECORATOR
        return None

    def _record_annotation_findings(self, annotations: list[ast.expr]) -> None:
        type_checking_only_names = set().union(
            *(scope.type_checking_only_names for scope in self._scopes)
        )
        enclosing_function_names = set().union(
            *(
                scope.runtime_names
                for scope in self._scopes
                if scope.kind == "function"
            )
        )
        for annotation in annotations:
            names = _annotation_names(annotation)
            blocked = sorted(names & type_checking_only_names)
            if blocked:
                self.findings.append(
                    AnnotationAuditFinding(
                        code="TPY-A002",
                        message=(
                            "annotation references TYPE_CHECKING-only import(s) "
                            f"{', '.join(blocked)}; runtime annotation evaluation can fail or "
                            "reintroduce import cycles"
                        ),
                        line=annotation.lineno,
                        column=annotation.col_offset + 1,
                    )
                )

            if not self._annotation_is_deferred(annotation):
                continue
            local_names = sorted((names - set(blocked)) & enclosing_function_names)
            if local_names:
                self.findings.append(
                    AnnotationAuditFinding(
                        code="TPY-A001",
                        message=(
                            "deferred annotation in a local scope references enclosing "
                            "function-local name(s) "
                            f"{', '.join(local_names)}; those names may be unavailable to runtime "
                            "consumers such as typing.get_type_hints or "
                            "annotationlib.get_annotations"
                        ),
                        line=annotation.lineno,
                        column=annotation.col_offset + 1,
                    )
                )

    def _annotation_is_deferred(self, annotation: ast.expr) -> bool:
        return self._future_annotations or (
            isinstance(annotation, ast.Constant) and isinstance(annotation.value, str)
        )


def _callable_annotations(
    node: ast.FunctionDef | ast.AsyncFunctionDef,
) -> list[ast.expr]:
    annotations: list[ast.expr] = []
    for arg in [*node.args.posonlyargs, *node.args.args, *node.args.kwonlyargs]:
        if arg.annotation is not None:
            annotations.append(arg.annotation)
    if node.args.vararg is not None and node.args.vararg.annotation is not None:
        annotations.append(node.args.vararg.annotation)
    if node.args.kwarg is not None and node.args.kwarg.annotation is not None:
        annotations.append(node.args.kwarg.annotation)
    if node.returns is not None:
        annotations.append(node.returns)
    return annotations


def _parameter_names(node: ast.FunctionDef | ast.AsyncFunctionDef) -> set[str]:
    names = {
        arg.arg
        for arg in [*node.args.posonlyargs, *node.args.args, *node.args.kwonlyargs]
    }
    if node.args.vararg is not None:
        names.add(node.args.vararg.arg)
    if node.args.kwarg is not None:
        names.add(node.args.kwarg.arg)
    return names


def _uses_future_annotations(tree: ast.Module) -> bool:
    return any(
        isinstance(statement, ast.ImportFrom)
        and statement.module == "__future__"
        and any(alias.name == "annotations" for alias in statement.names)
        for statement in tree.body
    )


def _scope_names(statements: list[ast.stmt]) -> tuple[set[str], set[str]]:
    collector = _ScopeNameCollector()
    for statement in statements:
        collector.visit(statement)
    runtime_names = collector.runtime_names - collector.global_or_nonlocal_names
    type_checking_only_names = (
        collector.type_checking_only_names
        - collector.runtime_names
        - collector.global_or_nonlocal_names
    )
    return runtime_names, type_checking_only_names


class _ScopeNameCollector(ast.NodeVisitor):
    def __init__(self) -> None:
        self.runtime_names: set[str] = set()
        self.type_checking_only_names: set[str] = set()
        self.global_or_nonlocal_names: set[str] = set()
        self.typing_module_names: set[str] = {"typing"}
        self.type_checking_guard_names: set[str] = {"TYPE_CHECKING"}

    def visit_If(self, node: ast.If) -> None:
        dotted = _dotted_name(node.test)
        is_type_checking_guard = dotted in self.type_checking_guard_names
        if dotted is not None and "." in dotted:
            owner, name = dotted.rsplit(".", 1)
            is_type_checking_guard = (
                owner in self.typing_module_names and name == "TYPE_CHECKING"
            )
        if is_type_checking_guard:
            type_only = _TypeCheckingImportCollector()
            for statement in node.body:
                type_only.visit(statement)
            self.type_checking_only_names.update(type_only.names)
            for statement in node.orelse:
                self.visit(statement)
            return
        self.generic_visit(node)

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        self.runtime_names.add(node.name)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self.runtime_names.add(node.name)

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        self.runtime_names.add(node.name)

    def visit_Lambda(self, node: ast.Lambda) -> None:
        return

    def visit_ListComp(self, node: ast.ListComp) -> None:
        return

    def visit_SetComp(self, node: ast.SetComp) -> None:
        return

    def visit_DictComp(self, node: ast.DictComp) -> None:
        return

    def visit_GeneratorExp(self, node: ast.GeneratorExp) -> None:
        return

    def visit_Import(self, node: ast.Import) -> None:
        self.typing_module_names.update(
            alias.asname or "typing" for alias in node.names if alias.name == "typing"
        )
        self.runtime_names.update(
            alias.asname or alias.name.split(".", 1)[0] for alias in node.names
        )

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        if node.module == "typing":
            self.type_checking_guard_names.update(
                alias.asname or alias.name
                for alias in node.names
                if alias.name == "TYPE_CHECKING"
            )
        self.runtime_names.update(
            alias.asname or alias.name for alias in node.names if alias.name != "*"
        )

    def visit_Name(self, node: ast.Name) -> None:
        if isinstance(node.ctx, ast.Store):
            self.runtime_names.add(node.id)

    def visit_ExceptHandler(self, node: ast.ExceptHandler) -> None:
        if node.name is not None:
            self.runtime_names.add(node.name)
        self.generic_visit(node)

    def visit_Global(self, node: ast.Global) -> None:
        self.global_or_nonlocal_names.update(node.names)

    def visit_Nonlocal(self, node: ast.Nonlocal) -> None:
        self.global_or_nonlocal_names.update(node.names)

    def visit_MatchAs(self, node: ast.AST) -> None:
        name = getattr(node, "name", None)
        if name is not None:
            self.runtime_names.add(name)
        self.generic_visit(node)

    def visit_MatchStar(self, node: ast.AST) -> None:
        name = getattr(node, "name", None)
        if name is not None:
            self.runtime_names.add(name)


class _TypeCheckingImportCollector(ast.NodeVisitor):
    def __init__(self) -> None:
        self.names: set[str] = set()

    def visit_Import(self, node: ast.Import) -> None:
        self.names.update(alias.asname or alias.name.split(".", 1)[0] for alias in node.names)

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        self.names.update(
            alias.asname or alias.name for alias in node.names if alias.name != "*"
        )

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        return

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        return

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        return

    def visit_Lambda(self, node: ast.Lambda) -> None:
        return


def _annotation_names(annotation: ast.expr) -> set[str]:
    if isinstance(annotation, ast.Constant) and isinstance(annotation.value, str):
        try:
            annotation = ast.parse(annotation.value, mode="eval").body
        except SyntaxError:
            return set()
    return {child.id for child in ast.walk(annotation) if isinstance(child, ast.Name)}


def _is_type_checking_guard(test: ast.expr) -> bool:
    return _dotted_name(test) in {"TYPE_CHECKING", "typing.TYPE_CHECKING"}


def _call_consumer(dotted: str | None) -> AnnotationConsumer | None:
    if dotted == "typing.get_type_hints":
        return AnnotationConsumer.TYPING_GET_TYPE_HINTS
    if dotted == "inspect.get_annotations":
        return AnnotationConsumer.INSPECT_GET_ANNOTATIONS
    if dotted == "annotationlib.get_annotations":
        return AnnotationConsumer.ANNOTATIONLIB_GET_ANNOTATIONS
    if dotted == "fastapi.Depends":
        return AnnotationConsumer.FASTAPI_DEPENDS
    if dotted == "pydantic.Field":
        return AnnotationConsumer.PYDANTIC_FIELD
    return None


def _is_fastapi_route_decorator_name(dotted: str | None) -> bool:
    if dotted is None:
        return False
    route_suffixes = {
        "get",
        "post",
        "put",
        "delete",
        "patch",
        "options",
        "head",
        "websocket",
        "api_route",
    }
    owner, separator, suffix = dotted.rpartition(".")
    return (
        bool(separator)
        and owner in {"fastapi.FastAPI.instance", "fastapi.APIRouter.instance"}
        and suffix in route_suffixes
    )


def _dotted_name(expr: ast.expr) -> str | None:
    if isinstance(expr, ast.Name):
        return expr.id
    if isinstance(expr, ast.Attribute):
        parent = _dotted_name(expr.value)
        if parent is None:
            return None
        return f"{parent}.{expr.attr}"
    return None
