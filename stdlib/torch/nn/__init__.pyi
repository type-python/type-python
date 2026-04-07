from typing import Any

from torch import Tensor

__all__ = ["Module", "Linear"]

class Module:
    def __call__(self, value: Any) -> Tensor: ...

class Linear(Module):
    def __init__(self, in_features: int, out_features: int) -> None: ...
