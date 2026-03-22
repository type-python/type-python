class Any:
    pass

class Optional:
    pass

class Union:
    pass

class TypeAlias:
    pass

class ClassVar:
    pass

class Final:
    pass

class Callable:
    def __class_getitem__(cls, item) -> Any: ...

class property:
    pass

class classmethod:
    pass

class staticmethod:
    pass

class Literal:
    def __class_getitem__(cls, item) -> Any: ...

class Annotated:
    pass

class Required:
    pass

class NotRequired:
    pass

class ReadOnly:
    pass

class Unpack:
    pass

class Concatenate:
    pass

class TypeGuard:
    pass

class TypeIs:
    pass

class TypedDict:
    pass

class Protocol:
    pass

class Iterator:
    def __next__(self) -> Any: ...

class Awaitable(Protocol):
    def __await__(self) -> Iterator: ...

class ContextManager(Protocol):
    def __enter__(self) -> Any: ...
    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> bool: ...

class AbstractContextManager(ContextManager):
    pass

class AsyncIterable(Protocol):
    def __aiter__(self) -> AsyncIterator: ...

class AsyncIterator(AsyncIterable):
    def __anext__(self) -> Awaitable: ...

class AsyncGenerator(AsyncIterator):
    def asend(self, value: Any) -> Awaitable: ...
    def athrow(self, typ: Any, val: Any, tb: Any) -> Awaitable: ...
    def aclose(self) -> Awaitable: ...

class Coroutine(Awaitable):
    def send(self, value: Any) -> Any: ...
    def throw(self, typ: Any, val: Any, tb: Any) -> Any: ...
    def close(self) -> None: ...

class Generator(Iterator):
    def send(self, value: Any) -> Any: ...
    def throw(self, typ: Any, val: Any, tb: Any) -> Any: ...
    def close(self) -> None: ...

def cast(t, value) -> Any: ...
def dataclass_transform(func) -> Any: ...
def override(func) -> Any: ...
def NewType(name: str, typ) -> Any: ...
def TypeVar(name: str) -> Any: ...
def ParamSpec(name: str) -> Any: ...
def TypeVarTuple(name: str) -> Any: ...
