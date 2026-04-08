import importlib
import json
import sys

sys.path.insert(0, sys.argv[1])
module_name = sys.argv[2]

try:
    importlib.import_module(module_name)
except BaseException as error:
    print(
        json.dumps({"importable": False, "error": f"{type(error).__name__}: {error}"})
    )
else:
    print(json.dumps({"importable": True}))
