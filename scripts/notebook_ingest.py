from __future__ import annotations

import ast
import builtins
import dataclasses
import json
import pathlib
import sys
from collections.abc import Iterable, Sequence


@dataclasses.dataclass(frozen=True)
class CodeCell:
    index: int
    source: str
    defines: tuple[str, ...]
    uses_previous: tuple[str, ...]
    public_symbols: tuple[str, ...]
    implicit_globals: tuple[str, ...]
    untyped_functions: tuple[str, ...]
    dict_like_records: tuple[str, ...]
    dataframe_boundaries: tuple[str, ...]
    side_effects: tuple[str, ...]

    def to_json(self) -> dict[str, object]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class NotebookReport:
    path: str
    cells: tuple[CodeCell, ...]
    candidate_tpy: str
    pyi_preview: str

    def to_json(self) -> dict[str, object]:
        return {
            "path": self.path,
            "cells": [cell.to_json() for cell in self.cells],
            "candidate_tpy": self.candidate_tpy,
            "pyi_preview": self.pyi_preview,
        }


def load_notebook(path: pathlib.Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def source_text(source: object) -> str:
    if isinstance(source, str):
        return source
    if isinstance(source, list):
        return "".join(str(part) for part in source)
    return ""


def iter_code_sources(notebook: dict[str, object]) -> Iterable[str]:
    cells = notebook.get("cells", [])
    if not isinstance(cells, list):
        return
    for cell in cells:
        if not isinstance(cell, dict):
            continue
        if cell.get("cell_type") != "code":
            continue
        yield source_text(cell.get("source", ""))


def assigned_names(target: ast.AST) -> set[str]:
    if isinstance(target, ast.Name):
        return {target.id}
    if isinstance(target, (ast.Tuple, ast.List)):
        names: set[str] = set()
        for element in target.elts:
            names.update(assigned_names(element))
        return names
    return set()


class CellFacts(ast.NodeVisitor):
    def __init__(self) -> None:
        self.defines: set[str] = set()
        self.loads: set[str] = set()
        self.public_symbols: set[str] = set()
        self.untyped_functions: set[str] = set()
        self.dict_like_records: set[str] = set()
        self.dataframe_boundaries: set[str] = set()
        self.side_effects: set[str] = set()

    def visit_Name(self, node: ast.Name) -> None:
        if isinstance(node.ctx, ast.Load):
            self.loads.add(node.id)

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        self.defines.add(node.name)
        if not node.name.startswith("_"):
            self.public_symbols.add(node.name)
        args = [*node.args.posonlyargs, *node.args.args, *node.args.kwonlyargs]
        has_untyped_arg = any(arg.annotation is None for arg in args)
        if node.returns is None or has_untyped_arg:
            self.untyped_functions.add(node.name)

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self.visit_FunctionDef(node)  # type: ignore[arg-type]

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        self.defines.add(node.name)
        if not node.name.startswith("_"):
            self.public_symbols.add(node.name)
        self.generic_visit(node)

    def visit_Assign(self, node: ast.Assign) -> None:
        names: set[str] = set()
        for target in node.targets:
            names.update(assigned_names(target))
        self.defines.update(names)
        self.public_symbols.update(name for name in names if not name.startswith("_"))
        if isinstance(node.value, ast.Dict):
            self.dict_like_records.update(names)
        if is_dataframe_boundary(node.value):
            self.dataframe_boundaries.update(names)
        self.generic_visit(node)

    def visit_AnnAssign(self, node: ast.AnnAssign) -> None:
        names = assigned_names(node.target)
        self.defines.update(names)
        self.public_symbols.update(name for name in names if not name.startswith("_"))
        if node.value is not None and is_dataframe_boundary(node.value):
            self.dataframe_boundaries.update(names)
        self.generic_visit(node)

    def visit_Expr(self, node: ast.Expr) -> None:
        if isinstance(node.value, ast.Call):
            name = call_name(node.value)
            if name in {"print", "display", "open"} or name.endswith(".to_csv"):
                self.side_effects.add(name)
        self.generic_visit(node)


def call_name(node: ast.Call) -> str:
    if isinstance(node.func, ast.Name):
        return node.func.id
    if isinstance(node.func, ast.Attribute) and isinstance(node.func.value, ast.Name):
        return f"{node.func.value.id}.{node.func.attr}"
    return "<call>"


def is_dataframe_boundary(node: ast.AST) -> bool:
    if not isinstance(node, ast.Call):
        return False
    name = call_name(node)
    return name in {"pd.DataFrame", "pandas.DataFrame", "pd.read_csv", "pandas.read_csv"}


def analyze_source(index: int, source: str, previous_defs: set[str]) -> CodeCell:
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return CodeCell(index, source, (), (), (), (), (), (), ("syntax-error",))
    facts = CellFacts()
    facts.visit(tree)
    uses_previous = facts.loads & previous_defs
    builtin_names = set(dir(builtins)) | {"pd", "pandas"}
    implicit_globals = facts.loads - previous_defs - facts.defines - builtin_names
    return CodeCell(
        index=index,
        source=source,
        defines=tuple(sorted(facts.defines)),
        uses_previous=tuple(sorted(uses_previous)),
        public_symbols=tuple(sorted(facts.public_symbols)),
        implicit_globals=tuple(sorted(implicit_globals)),
        untyped_functions=tuple(sorted(facts.untyped_functions)),
        dict_like_records=tuple(sorted(facts.dict_like_records)),
        dataframe_boundaries=tuple(sorted(facts.dataframe_boundaries)),
        side_effects=tuple(sorted(facts.side_effects)),
    )


def build_candidate_tpy(cells: Sequence[CodeCell]) -> str:
    parts: list[str] = []
    for cell in cells:
        if cell.source.strip():
            parts.append(f"# %% notebook cell {cell.index}\n{cell.source.rstrip()}\n")
    return "\n".join(parts)


def build_pyi_preview(cells: Sequence[CodeCell]) -> str:
    lines: list[str] = []
    for cell in cells:
        try:
            tree = ast.parse(cell.source)
        except SyntaxError:
            continue
        for node in tree.body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                prefix = "async " if isinstance(node, ast.AsyncFunctionDef) else ""
                lines.append(f"{prefix}def {node.name}(...): ...")
            elif isinstance(node, ast.ClassDef):
                lines.append(f"class {node.name}: ...")
            elif isinstance(node, (ast.Assign, ast.AnnAssign)):
                targets = node.targets if isinstance(node, ast.Assign) else [node.target]
                for target in targets:
                    for name in sorted(assigned_names(target)):
                        if not name.startswith("_"):
                            lines.append(f"{name}: object")
    return "\n".join(lines) + ("\n" if lines else "")


def analyze_notebook(path: pathlib.Path) -> NotebookReport:
    previous_defs: set[str] = set()
    cells: list[CodeCell] = []
    for index, source in enumerate(iter_code_sources(load_notebook(path)), start=1):
        cell = analyze_source(index, source, previous_defs)
        cells.append(cell)
        previous_defs.update(cell.defines)
    return NotebookReport(
        path=str(path),
        cells=tuple(cells),
        candidate_tpy=build_candidate_tpy(cells),
        pyi_preview=build_pyi_preview(cells),
    )


def main(argv: Sequence[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if len(args) != 1:
        print("usage: notebook_ingest.py NOTEBOOK.ipynb", file=sys.stderr)
        return 2
    report = analyze_notebook(pathlib.Path(args[0]))
    print(json.dumps(report.to_json(), indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
