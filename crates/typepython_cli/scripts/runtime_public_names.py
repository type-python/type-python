import importlib
import json
import sys

sys.path.insert(0, sys.argv[1])
module_name = sys.argv[2]

try:
    module = importlib.import_module(module_name)
except Exception:
    print(json.dumps({"importable": False}))
else:
    exported = getattr(module, "__all__", None)
    if isinstance(exported, (list, tuple)) and all(isinstance(name, str) for name in exported):
        names = sorted(dict.fromkeys(exported))
    else:
        names = sorted(name for name in dir(module) if not name.startswith("_"))
    print(json.dumps({"importable": True, "names": names}))
