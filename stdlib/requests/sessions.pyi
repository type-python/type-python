from typing import Any

from requests import Response

__all__ = ["Session"]

class Session:
    def get(self, url: Any) -> Response: ...
