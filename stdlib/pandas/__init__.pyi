from typing import Any
from typing_extensions import Self

__all__ = ["DataFrame", "Series", "read_csv"]

class Series:
    def head(self, n: int = ...) -> Self: ...

class DataFrame:
    def head(self, n: int = ...) -> Self: ...

def read_csv(path: Any) -> DataFrame: ...
