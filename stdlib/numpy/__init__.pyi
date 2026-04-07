from typing import Any
from typing_extensions import Self

__all__ = ["array", "ndarray"]

class ndarray:
    def reshape(self, shape: Any) -> Self: ...

def array(value: Any) -> ndarray: ...
