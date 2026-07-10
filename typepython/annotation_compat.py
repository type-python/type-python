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
    definitely_bound_names: set[str]


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
    if isinstance(obj, ModuleType):
        default_globals = vars(obj)
        default_locals = None
    elif isinstance(obj, type):
        module = sys.modules.get(getattr(obj, "__module__", ""))
        default_globals = vars(module) if module is not None else {}
        default_locals = dict(vars(obj))
    else:
        default_globals = getattr(obj, "__globals__", None)
        if default_globals is None:
            module = sys.modules.get(getattr(obj, "__module__", ""))
            default_globals = vars(module) if module is not None else {}
        default_locals = None

    globalns = globals if globals is not None else default_globals
    if locals is not None:
        localns = locals
    elif default_locals is not None:
        localns = default_locals
    else:
        localns = globalns
    return globalns, localns


class _AnnotationAuditVisitor(ast.NodeVisitor):
    def __init__(self, *, future_annotations: bool) -> None:
        self.consumers: set[AnnotationConsumer] = set()
        self.findings: list[AnnotationAuditFinding] = []
        self._future_annotations = future_annotations
        self._scopes: list[_AuditScope] = []

    def visit_Module(self, node: ast.Module) -> None:
        self._visit_scope("module", node.body)

    def visit_If(self, node: ast.If) -> None:
        if self._canonical_name(node.test) == "typing.TYPE_CHECKING":
            for statement in node.orelse:
                self.visit(statement)
            return
        self.visit(node.test)
        before = self._current_scope_state()
        for statement in node.body:
            self.visit(statement)
        body_state = self._current_scope_state()
        self._restore_current_scope_state(before)
        for statement in node.orelse:
            self.visit(statement)
        else_state = self._current_scope_state()
        self._merge_current_scope_states(body_state, else_state)

    def visit_For(self, node: ast.For) -> None:
        self._visit_loop(node)

    def visit_AsyncFor(self, node: ast.AsyncFor) -> None:
        self._visit_loop(node)

    def _visit_loop(self, node: ast.For | ast.AsyncFor) -> None:
        self.visit(node.iter)
        before = self._current_scope_state()
        self._bind_assignment_target(node.target, None)
        for statement in node.body:
            self.visit(statement)
        body_state = self._current_scope_state()
        self._merge_current_scope_states(before, body_state)
        without_else = self._current_scope_state()
        for statement in node.orelse:
            self.visit(statement)
        else_state = self._current_scope_state()
        self._merge_current_scope_states(without_else, else_state)

    def visit_While(self, node: ast.While) -> None:
        self.visit(node.test)
        before = self._current_scope_state()
        for statement in node.body:
            self.visit(statement)
        body_state = self._current_scope_state()
        self._merge_current_scope_states(before, body_state)
        without_else = self._current_scope_state()
        for statement in node.orelse:
            self.visit(statement)
        else_state = self._current_scope_state()
        self._merge_current_scope_states(without_else, else_state)

    def visit_Try(self, node: ast.Try) -> None:
        self._visit_try(node)

    def visit_TryStar(self, node: ast.AST) -> None:
        self._visit_try(node)

    def _visit_try(self, node: ast.AST) -> None:
        before = self._current_scope_state()
        for statement in getattr(node, "body", ()):
            self.visit(statement)
        for statement in getattr(node, "orelse", ()):
            self.visit(statement)
        states = [before, self._current_scope_state()]
        for handler in getattr(node, "handlers", ()):
            self._restore_current_scope_state(before)
            if handler.type is not None:
                self.visit(handler.type)
            if handler.name is not None:
                self._bind_runtime_name(handler.name, None)
            for statement in handler.body:
                self.visit(statement)
            if handler.name is not None:
                self._unbind_runtime_name(handler.name)
            states.append(self._current_scope_state())
        self._merge_multiple_current_scope_states(states)
        for statement in getattr(node, "finalbody", ()):
            self.visit(statement)

    def visit_BoolOp(self, node: ast.BoolOp) -> None:
        if not node.values:
            return
        self.visit(node.values[0])
        for value in node.values[1:]:
            before = self._current_scope_state()
            self.visit(value)
            after = self._current_scope_state()
            self._merge_current_scope_states(before, after)

    def visit_IfExp(self, node: ast.IfExp) -> None:
        self.visit(node.test)
        before = self._current_scope_state()
        self.visit(node.body)
        body_state = self._current_scope_state()
        self._restore_current_scope_state(before)
        self.visit(node.orelse)
        else_state = self._current_scope_state()
        self._merge_current_scope_states(body_state, else_state)

    def visit_Lambda(self, node: ast.Lambda) -> None:
        for default in [*node.args.defaults, *node.args.kw_defaults]:
            if default is not None:
                self.visit(default)
        before = self._current_scope_state()
        self.visit(node.body)
        self._restore_current_scope_state(before)

    def visit_ListComp(self, node: ast.ListComp) -> None:
        self._visit_comprehension(node, (node.elt,))

    def visit_SetComp(self, node: ast.SetComp) -> None:
        self._visit_comprehension(node, (node.elt,))

    def visit_GeneratorExp(self, node: ast.GeneratorExp) -> None:
        self._visit_comprehension(node, (node.elt,))

    def visit_DictComp(self, node: ast.DictComp) -> None:
        self._visit_comprehension(node, (node.key, node.value))

    def _visit_comprehension(
        self,
        node: ast.ListComp | ast.SetComp | ast.GeneratorExp | ast.DictComp,
        values: tuple[ast.expr, ...],
    ) -> None:
        if not node.generators:
            return
        self.visit(node.generators[0].iter)
        before = self._current_scope_state()
        for index, generator in enumerate(node.generators):
            if index > 0:
                self.visit(generator.iter)
            for condition in generator.ifs:
                self.visit(condition)
        for value in values:
            self.visit(value)
        after = self._current_scope_state()
        self._merge_current_scope_states(before, after)

    def visit_Match(self, node: ast.AST) -> None:
        self.visit(getattr(node, "subject"))
        before = self._current_scope_state()
        states = [before]
        for case in getattr(node, "cases", ()):
            self._restore_current_scope_state(before)
            for name in _match_pattern_names(case.pattern):
                self._bind_runtime_name(name, None)
            if case.guard is not None:
                self.visit(case.guard)
            for statement in case.body:
                self.visit(statement)
            states.append(self._current_scope_state())
        self._merge_multiple_current_scope_states(states)

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        for decorator in node.decorator_list:
            consumer = self._decorator_consumer(decorator)
            if consumer is not None:
                self.consumers.add(consumer)
        self._record_annotation_findings(
            _callable_annotations(node),
            include_current_class_names=True,
        )
        self._visit_definition_expressions(node)
        self._visit_scope("function", node.body, parameters=_parameter_names(node))
        self._bind_runtime_name(node.name, None)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        for decorator in node.decorator_list:
            consumer = self._decorator_consumer(decorator)
            if consumer is not None:
                self.consumers.add(consumer)
        self._record_annotation_findings(
            _callable_annotations(node),
            include_current_class_names=True,
        )
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
        for alias in node.names:
            if alias.name == "*":
                continue
            canonical_name = (
                f"{node.module}.{alias.name}"
                if node.level == 0 and node.module is not None
                else None
            )
            self._bind_runtime_name(
                alias.asname or alias.name,
                canonical_name,
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

    def visit_Delete(self, node: ast.Delete) -> None:
        for target in node.targets:
            self._unbind_assignment_target(target)

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
        visible_resolved_names: dict[str, str | None] = {}
        for scope in reversed(self._scopes):
            if kind == "function" and scope.kind == "class":
                continue
            for name in scope.runtime_names:
                if name not in visible_resolved_names:
                    visible_resolved_names[name] = scope.resolved_names.get(name)
        runtime_names, type_checking_only_names = _scope_names(
            statements,
            typing_module_names={
                name
                for name, canonical in visible_resolved_names.items()
                if canonical == "typing"
            },
            type_checking_guard_names={
                name
                for name, canonical in visible_resolved_names.items()
                if canonical == "typing.TYPE_CHECKING"
            },
            shadowed_names=parameters,
        )
        runtime_names.update(parameters or ())
        self._scopes.append(
            _AuditScope(
                kind=kind,
                runtime_names=frozenset(runtime_names),
                type_checking_only_names=frozenset(type_checking_only_names),
                resolved_names={},
                definitely_bound_names=set(parameters or ()),
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
        self._scopes[-1].definitely_bound_names.add(name)
        if resolved is None:
            self._scopes[-1].resolved_names.pop(name, None)
        else:
            self._scopes[-1].resolved_names[name] = resolved

    def _unbind_assignment_target(self, target: ast.expr) -> None:
        if isinstance(target, ast.Name):
            self._unbind_runtime_name(target.id)
            return
        if isinstance(target, (ast.List, ast.Tuple)):
            for element in target.elts:
                self._unbind_assignment_target(element)

    def _unbind_runtime_name(self, name: str) -> None:
        if not self._scopes:
            return
        self._scopes[-1].definitely_bound_names.discard(name)
        self._scopes[-1].resolved_names.pop(name, None)

    def _current_scope_state(self) -> tuple[set[str], dict[str, str]]:
        if not self._scopes:
            return set(), {}
        scope = self._scopes[-1]
        return set(scope.definitely_bound_names), dict(scope.resolved_names)

    def _restore_current_scope_state(
        self,
        state: tuple[set[str], dict[str, str]],
    ) -> None:
        if not self._scopes:
            return
        definitely_bound_names, resolved_names = state
        self._scopes[-1].definitely_bound_names = set(definitely_bound_names)
        self._scopes[-1].resolved_names = dict(resolved_names)

    def _merge_current_scope_states(
        self,
        left: tuple[set[str], dict[str, str]],
        right: tuple[set[str], dict[str, str]],
    ) -> None:
        if not self._scopes:
            return
        left_bound, left_resolved = left
        right_bound, right_resolved = right
        common_bound = left_bound & right_bound
        common_resolved = {
            name: canonical
            for name, canonical in left_resolved.items()
            if name in common_bound and right_resolved.get(name) == canonical
        }
        self._scopes[-1].definitely_bound_names = common_bound
        self._scopes[-1].resolved_names = common_resolved

    def _merge_multiple_current_scope_states(
        self,
        states: list[tuple[set[str], dict[str, str]]],
    ) -> None:
        if not states:
            return
        merged = states[0]
        for state in states[1:]:
            self._restore_current_scope_state(merged)
            self._merge_current_scope_states(merged, state)
            merged = self._current_scope_state()
        self._restore_current_scope_state(merged)

    def _decorator_consumer(self, decorator: ast.expr) -> AnnotationConsumer | None:
        expression = decorator.func if isinstance(decorator, ast.Call) else decorator
        canonical = self._canonical_name(expression)
        if canonical == "dataclasses.dataclass":
            return AnnotationConsumer.DATACLASS_DECORATOR
        if _is_fastapi_route_decorator_name(canonical):
            return AnnotationConsumer.FASTAPI_ROUTE_DECORATOR
        return None

    def _record_annotation_findings(
        self,
        annotations: list[ast.expr],
        *,
        include_current_class_names: bool = False,
    ) -> None:
        enclosing_function_names = set().union(
            *(
                scope.runtime_names
                for scope in self._scopes
                if scope.kind == "function"
            )
        )
        if (
            include_current_class_names
            and self._scopes
            and self._scopes[-1].kind == "class"
        ):
            enclosing_function_names.update(self._scopes[-1].runtime_names)
        for annotation in annotations:
            names = _annotation_names(annotation)
            blocked = sorted(name for name in names if self._is_unbound_type_only_name(name))
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

    def _is_unbound_type_only_name(self, name: str) -> bool:
        for scope in reversed(self._scopes):
            if name in scope.type_checking_only_names:
                return name not in scope.definitely_bound_names
            if name in scope.definitely_bound_names:
                return False
        return False

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


def _scope_names(
    statements: list[ast.stmt],
    *,
    typing_module_names: set[str] | None = None,
    type_checking_guard_names: set[str] | None = None,
    shadowed_names: set[str] | None = None,
) -> tuple[set[str], set[str]]:
    collector = _ScopeNameCollector(
        typing_module_names=typing_module_names,
        type_checking_guard_names=type_checking_guard_names,
        shadowed_names=shadowed_names,
    )
    for statement in statements:
        collector.visit(statement)
    runtime_names = collector.runtime_names - collector.global_or_nonlocal_names
    type_checking_only_names = (
        collector.type_checking_only_names
        - collector.global_or_nonlocal_names
    )
    return runtime_names, type_checking_only_names


class _ScopeNameCollector(ast.NodeVisitor):
    def __init__(
        self,
        *,
        typing_module_names: set[str] | None = None,
        type_checking_guard_names: set[str] | None = None,
        shadowed_names: set[str] | None = None,
    ) -> None:
        self.runtime_names: set[str] = set()
        self.type_checking_only_names: set[str] = set()
        self.global_or_nonlocal_names: set[str] = set()
        self.typing_module_names = set(typing_module_names or ())
        self.type_checking_guard_names = set(type_checking_guard_names or ())
        for name in shadowed_names or ():
            self._shadow_typing_binding(name)

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
        self._shadow_typing_binding(node.name)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self.runtime_names.add(node.name)
        self._shadow_typing_binding(node.name)

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        self.runtime_names.add(node.name)
        self._shadow_typing_binding(node.name)

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
        for alias in node.names:
            local_name = alias.asname or alias.name.split(".", 1)[0]
            self.runtime_names.add(local_name)
            self._shadow_typing_binding(local_name)
            if alias.name == "typing":
                self.typing_module_names.add(local_name)

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        for alias in node.names:
            if alias.name == "*":
                continue
            local_name = alias.asname or alias.name
            self.runtime_names.add(local_name)
            self._shadow_typing_binding(local_name)
            if node.level == 0 and node.module == "typing" and alias.name == "TYPE_CHECKING":
                self.type_checking_guard_names.add(local_name)

    def visit_Name(self, node: ast.Name) -> None:
        if isinstance(node.ctx, ast.Store):
            self.runtime_names.add(node.id)
            self._shadow_typing_binding(node.id)

    def visit_ExceptHandler(self, node: ast.ExceptHandler) -> None:
        if node.name is not None:
            self.runtime_names.add(node.name)
        self.generic_visit(node)

    def visit_Global(self, node: ast.Global) -> None:
        self.global_or_nonlocal_names.update(node.names)

    def visit_Nonlocal(self, node: ast.Nonlocal) -> None:
        self.global_or_nonlocal_names.update(node.names)

    def _shadow_typing_binding(self, name: str) -> None:
        self.typing_module_names.discard(name)
        self.type_checking_guard_names.discard(name)

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
        self.names.add(node.name)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self.names.add(node.name)

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        self.names.add(node.name)

    def visit_Name(self, node: ast.Name) -> None:
        if isinstance(node.ctx, ast.Store):
            self.names.add(node.id)

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


def _annotation_names(annotation: ast.expr) -> set[str]:
    return _annotation_names_impl(annotation, set(), 0)


def _annotation_names_impl(
    annotation: ast.expr,
    seen_forward_refs: set[str],
    depth: int,
) -> set[str]:
    if depth > 20:
        return set()
    if isinstance(annotation, ast.Constant) and isinstance(annotation.value, str):
        return _parsed_forward_ref_names(annotation.value, seen_forward_refs, depth)

    names = {child.id for child in ast.walk(annotation) if isinstance(child, ast.Name)}
    names.update(_nested_forward_ref_names(annotation, seen_forward_refs, depth))
    return names


def _parsed_forward_ref_names(
    value: str,
    seen_forward_refs: set[str],
    depth: int,
) -> set[str]:
    if value in seen_forward_refs:
        return set()
    seen_forward_refs.add(value)
    try:
        parsed = ast.parse(value, mode="eval").body
    except SyntaxError:
        return set()
    return _annotation_names_impl(parsed, seen_forward_refs, depth + 1)


def _nested_forward_ref_names(
    expression: ast.expr,
    seen_forward_refs: set[str],
    depth: int,
) -> set[str]:
    if isinstance(expression, ast.Constant):
        return set()
    if isinstance(expression, ast.Subscript):
        dotted = _dotted_name(expression.value)
        elements = (
            list(expression.slice.elts)
            if isinstance(expression.slice, (ast.List, ast.Tuple))
            else [expression.slice]
        )
        if dotted in {"Literal", "typing.Literal", "typing_extensions.Literal"}:
            return set()
        if dotted in {"Annotated", "typing.Annotated", "typing_extensions.Annotated"}:
            elements = elements[:1]
        return set().union(
            *(
                _type_position_forward_ref_names(element, seen_forward_refs, depth)
                for element in elements
            )
        )
    if isinstance(expression, ast.Call) and _dotted_name(expression.func) in {
        "ForwardRef",
        "typing.ForwardRef",
        "typing_extensions.ForwardRef",
    }:
        if expression.args:
            return _type_position_forward_ref_names(
                expression.args[0],
                seen_forward_refs,
                depth,
            )
        return set()
    if isinstance(expression, ast.BinOp):
        return _type_position_forward_ref_names(
            expression.left,
            seen_forward_refs,
            depth,
        ) | _type_position_forward_ref_names(
            expression.right,
            seen_forward_refs,
            depth,
        )
    if isinstance(expression, (ast.List, ast.Tuple)):
        return set().union(
            *(
                _type_position_forward_ref_names(element, seen_forward_refs, depth)
                for element in expression.elts
            )
        )
    if isinstance(expression, ast.Starred):
        return _type_position_forward_ref_names(expression.value, seen_forward_refs, depth)
    return set()


def _type_position_forward_ref_names(
    expression: ast.expr,
    seen_forward_refs: set[str],
    depth: int,
) -> set[str]:
    if isinstance(expression, ast.Constant) and isinstance(expression.value, str):
        return _parsed_forward_ref_names(expression.value, seen_forward_refs, depth)
    return _nested_forward_ref_names(expression, seen_forward_refs, depth)


def _match_pattern_names(pattern: ast.AST) -> set[str]:
    return {
        name
        for child in ast.walk(pattern)
        if type(child).__name__ in {"MatchAs", "MatchStar"}
        for name in [getattr(child, "name", None)]
        if name is not None
    }


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
