use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use typepython_lowering::lower;
use typepython_syntax::{SourceFile, SourceKind, parse};

fn bench_lower_small(c: &mut Criterion) {
    let source = "\
typealias UserId = int

def hello(name: str) -> str:
    return f\"Hello, {name}\"
";
    let tree = parse(SourceFile {
        path: PathBuf::from("bench.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::from("bench"),
        text: source.to_string(),
    });
    c.bench_function("lower_small_module", |b| b.iter(|| lower(&tree)));
}

fn bench_lower_medium(c: &mut Criterion) {
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

overload def parse(x: str) -> int: ...
overload def parse(x: bytes) -> int: ...

def helper[T](value: T) -> T:
    return value

unsafe:
    x = eval('1')
";
    let tree = parse(SourceFile {
        path: PathBuf::from("bench.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::from("bench"),
        text: source.to_string(),
    });
    c.bench_function("lower_medium_module", |b| b.iter(|| lower(&tree)));
}

fn bench_lower_python_passthrough(c: &mut Criterion) {
    let mut source = String::with_capacity(4096);
    for i in 0..30 {
        source.push_str(&format!(
            "\
def func_{i}(arg: int) -> int:
    return arg + {i}

"
        ));
    }
    let tree = parse(SourceFile {
        path: PathBuf::from("bench.py"),
        kind: SourceKind::Python,
        logical_module: String::from("bench"),
        text: source,
    });
    c.bench_function("lower_python_passthrough", |b| b.iter(|| lower(&tree)));
}

criterion_group!(benches, bench_lower_small, bench_lower_medium, bench_lower_python_passthrough);
criterion_main!(benches);
