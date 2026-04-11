from __future__ import annotations

import ast
import json
import sys


def literal_string_sequence(node: ast.AST | None) -> list[str] | None:
    if not isinstance(node, (ast.List, ast.Tuple)):
        return None
    if not all(
        isinstance(element, ast.Constant) and isinstance(element.value, str)
        for element in node.elts
    ):
        return None
    return [element.value for element in node.elts]


def resolve_function_return(
    functions: dict[str, ast.FunctionDef | ast.AsyncFunctionDef],
    name: str,
) -> list[str] | None:
    function = functions.get(name)
    if function is None:
        return None
    if function.decorator_list:
        return None
    if function.args.posonlyargs or function.args.args or function.args.kwonlyargs:
        return None
    if function.args.vararg is not None or function.args.kwarg is not None:
        return None
    if function.args.defaults or function.args.kw_defaults:
        return None
    if len(function.body) != 1 or not isinstance(function.body[0], ast.Return):
        return None
    return literal_string_sequence(function.body[0].value)


def resolve_all_names(
    value: ast.AST | None,
    functions: dict[str, ast.FunctionDef | ast.AsyncFunctionDef],
) -> list[str] | None:
    names = literal_string_sequence(value)
    if names is not None:
        return names
    if (
        isinstance(value, ast.Call)
        and isinstance(value.func, ast.Name)
        and not value.args
        and not value.keywords
    ):
        return resolve_function_return(functions, value.func.id)
    return None


with open(sys.argv[1], "r", encoding="utf-8") as handle:
    tree = ast.parse(handle.read(), sys.argv[1])

functions = {
    node.name: node
    for node in tree.body
    if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
}

names = None
for node in tree.body:
    if isinstance(node, ast.Assign):
        targets = node.targets
        value = node.value
    elif isinstance(node, ast.AnnAssign):
        targets = [node.target]
        value = node.value
    else:
        continue

    if any(
        isinstance(target, ast.Name) and target.id == "__all__" for target in targets
    ):
        names = resolve_all_names(value, functions)
        break

print(json.dumps(names))
