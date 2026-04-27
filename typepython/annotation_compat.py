from __future__ import annotations

import inspect
import sys
import ast
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


def supported_formats() -> AnnotationSupport:
    if HAS_ANNOTATIONLIB:
        return AnnotationSupport(value=True, forwardref=True, string=True)
    return AnnotationSupport(value=True, forwardref=False, string=False)


def audit_source(source: str, *, filename: str = "<source>") -> AnnotationAudit:
    tree = ast.parse(source, filename=filename)
    visitor = _AnnotationAuditVisitor()
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
    def __init__(self) -> None:
        self.consumers: set[AnnotationConsumer] = set()
        self.findings: list[AnnotationAuditFinding] = []
        self._scope_depth = 0

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        self._visit_callable_or_class(node)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self._visit_callable_or_class(node)

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        for decorator in node.decorator_list:
            consumer = _decorator_consumer(decorator)
            if consumer is not None:
                self.consumers.add(consumer)
        self._visit_callable_or_class(node)

    def visit_Call(self, node: ast.Call) -> None:
        consumer = _call_consumer(node.func)
        if consumer is not None:
            self.consumers.add(consumer)
        self.generic_visit(node)

    def _visit_callable_or_class(
        self,
        node: ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef,
    ) -> None:
        if self._scope_depth > 0:
            self._record_nested_annotation_findings(node)
        self._scope_depth += 1
        self.generic_visit(node)
        self._scope_depth -= 1

    def _record_nested_annotation_findings(
        self,
        node: ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef,
    ) -> None:
        annotations = _node_annotations(node)
        for annotation in annotations:
            if _annotation_mentions_local_name(annotation):
                self.findings.append(
                    AnnotationAuditFinding(
                        code="TPY-A001",
                        message=(
                            "annotation inside a local scope may be unavailable to runtime "
                            "consumers such as typing.get_type_hints or annotationlib.get_annotations"
                        ),
                        line=annotation.lineno,
                        column=annotation.col_offset + 1,
                    )
                )


def _node_annotations(
    node: ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef,
) -> list[ast.expr]:
    annotations: list[ast.expr] = []
    if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
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

    for statement in node.body:
        if isinstance(statement, ast.AnnAssign) and statement.annotation is not None:
            annotations.append(statement.annotation)
    return annotations


def _annotation_mentions_local_name(annotation: ast.expr) -> bool:
    if isinstance(annotation, ast.Constant) and isinstance(annotation.value, str):
        return True
    return any(isinstance(child, ast.Name) for child in ast.walk(annotation))


def _call_consumer(func: ast.expr) -> AnnotationConsumer | None:
    dotted = _dotted_name(func)
    if dotted in {"typing.get_type_hints", "get_type_hints"}:
        return AnnotationConsumer.TYPING_GET_TYPE_HINTS
    if dotted == "inspect.get_annotations":
        return AnnotationConsumer.INSPECT_GET_ANNOTATIONS
    if dotted == "annotationlib.get_annotations":
        return AnnotationConsumer.ANNOTATIONLIB_GET_ANNOTATIONS
    return None


def _decorator_consumer(decorator: ast.expr) -> AnnotationConsumer | None:
    dotted = _dotted_name(decorator.func if isinstance(decorator, ast.Call) else decorator)
    if dotted in {"dataclass", "dataclasses.dataclass"}:
        return AnnotationConsumer.DATACLASS_DECORATOR
    return None


def _dotted_name(expr: ast.expr) -> str | None:
    if isinstance(expr, ast.Name):
        return expr.id
    if isinstance(expr, ast.Attribute):
        parent = _dotted_name(expr.value)
        if parent is None:
            return None
        return f"{parent}.{expr.attr}"
    return None
