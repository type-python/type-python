from typing import Any
from typing_extensions import Self

__all__ = ["Tensor", "tensor"]

class Tensor:
    def to(self, device: Any) -> Self: ...

def tensor(value: Any) -> Tensor: ...
