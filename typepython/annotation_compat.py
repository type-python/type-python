from __future__ import annotations

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


def supported_formats() -> AnnotationSupport:
    if HAS_ANNOTATIONLIB:
        return AnnotationSupport(value=True, forwardref=True, string=True)
    return AnnotationSupport(value=True, forwardref=False, string=False)


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
