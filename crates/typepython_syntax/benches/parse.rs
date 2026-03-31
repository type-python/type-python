use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use typepython_syntax::{SourceFile, SourceKind, parse};

fn bench_parse_small(c: &mut Criterion) {
    let source = "\
def hello(name: str) -> str:
    return f\"Hello, {name}\"

class User:
    name: str
    age: int

    def greet(self) -> str:
        return f\"Hi, I am {self.name}\"
";
    c.bench_function("parse_small_module", |b| {
        b.iter(|| {
            parse(SourceFile {
                path: PathBuf::from("bench.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::from("bench"),
                text: source.to_string(),
            })
        })
    });
}

fn bench_parse_medium(c: &mut Criterion) {
    let mut source = String::with_capacity(8192);
    for i in 0..50 {
        source.push_str(&format!(
            "\
def func_{i}(arg: int, flag: bool = False) -> int:
    if flag:
        return arg + {i}
    return arg

"
        ));
    }
    for i in 0..10 {
        source.push_str(&format!(
            "\
class Model{i}:
    name: str
    value: int

    def method(self) -> int:
        return self.value + {i}

"
        ));
    }
    c.bench_function("parse_medium_module", |b| {
        b.iter(|| {
            parse(SourceFile {
                path: PathBuf::from("bench.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::from("bench"),
                text: source.clone(),
            })
        })
    });
}

fn bench_parse_typepython_extensions(c: &mut Criterion) {
    let source = "\
typealias UserId = int
typealias Pair[T] = tuple[T, T]

interface Closable:
    def close(self) -> None: ...

interface Serializable[T]:
    def serialize(self) -> T: ...

data class Point:
    x: float
    y: float

data class Point3D(Point):
    z: float

sealed class Expr:
    ...

class Add(Expr):
    left: Expr
    right: Expr

class Num(Expr):
    value: int

overload def parse(x: str) -> int: ...
overload def parse(x: bytes) -> int: ...

def helper() -> int:
    unsafe:
        return eval('1')
";
    c.bench_function("parse_typepython_extensions", |b| {
        b.iter(|| {
            parse(SourceFile {
                path: PathBuf::from("bench.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::from("bench"),
                text: source.to_string(),
            })
        })
    });
}

criterion_group!(benches, bench_parse_small, bench_parse_medium, bench_parse_typepython_extensions);
criterion_main!(benches);
