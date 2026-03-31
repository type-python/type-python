import ast
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    tree = ast.parse(handle.read(), sys.argv[1])

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

    if any(isinstance(target, ast.Name) and target.id == "__all__" for target in targets):
        if isinstance(value, (ast.List, ast.Tuple)) and all(
            isinstance(element, ast.Constant) and isinstance(element.value, str)
            for element in value.elts
        ):
            names = [element.value for element in value.elts]
        break

print(json.dumps(names))
