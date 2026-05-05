from ._runner import main
from .annotation_compat import (
    AnnotationAudit,
    AnnotationAuditFinding,
    AnnotationConsumer,
    AnnotationFormat,
    AnnotationSupport,
    audit_source,
    get_annotations,
    supported_formats,
)

__all__ = [
    "__version__",
    "AnnotationFormat",
    "AnnotationAudit",
    "AnnotationAuditFinding",
    "AnnotationConsumer",
    "AnnotationSupport",
    "audit_source",
    "get_annotations",
    "main",
    "supported_formats",
]

__version__ = "0.4.0"
