from typing import Any

__all__ = ["Response", "get"]

class Response:
    def json(self) -> Any: ...

def get(url: Any) -> Response: ...
