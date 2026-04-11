import importlib
import json
import sys

sys.path.insert(0, sys.argv[1])
module_name = sys.argv[2]

try:
    module = importlib.import_module(module_name)
    public_names = getattr(module, "__all__", None)
    if public_names is None:
        public_names = [name for name in vars(module) if not name.startswith("_")]
    elif not all(isinstance(name, str) for name in public_names):
        raise TypeError("__all__ must contain only strings")
except BaseException as error:
    print(
        json.dumps({"importable": False, "error": f"{type(error).__name__}: {error}"})
    )
else:
    print(json.dumps({"importable": True, "public_names": public_names}))
