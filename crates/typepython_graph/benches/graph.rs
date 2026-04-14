use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use typepython_binding::{
    BindingTable, Declaration, DeclarationKind, DeclarationOwnerKind, ModuleSurfaceFacts,
};
use typepython_graph::build;
use typepython_syntax::SourceKind;

fn make_binding(index: usize, package: &str) -> BindingTable {
    BindingTable {
        module_path: PathBuf::from(format!("src/{package}/mod_{index}.tpy")),
        module_key: format!("{package}.mod_{index}"),
        module_kind: SourceKind::TypePython,
        surface_facts: ModuleSurfaceFacts::default(),
        declarations: vec![
            Declaration {
                metadata: Default::default(),
                name: format!("Func{index}"),
                kind: DeclarationKind::Function,
                detail: String::from("(x: int) -> int"),
                value_type: None,
                value_type_expr: None,
                method_kind: None,
                class_kind: None,
                owner: None,
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            },
            Declaration {
                metadata: Default::default(),
                name: format!("Model{index}"),
                kind: DeclarationKind::Class,
                detail: String::new(),
                value_type: None,
                value_type_expr: None,
                method_kind: None,
                class_kind: Some(DeclarationOwnerKind::Class),
                owner: None,
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            },
        ],
        calls: Vec::new(),
        method_calls: Vec::new(),
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
    }
}

fn bench_build_10_modules(c: &mut Criterion) {
    let bindings: Vec<_> = (0..10).map(|i| make_binding(i, "pkg")).collect();
    c.bench_function("build_10_module_graph", |b| b.iter(|| build(&bindings)));
}

fn bench_build_50_modules(c: &mut Criterion) {
    let bindings: Vec<_> = (0..50).map(|i| make_binding(i, "pkg")).collect();
    c.bench_function("build_50_module_graph", |b| b.iter(|| build(&bindings)));
}

fn bench_build_nested_packages(c: &mut Criterion) {
    let bindings: Vec<_> = (0..20)
        .map(|i| {
            let pkg = format!("root.sub{}.deep{}", i / 5, i);
            make_binding(i, &pkg)
        })
        .collect();
    c.bench_function("build_nested_package_graph", |b| b.iter(|| build(&bindings)));
}

criterion_group!(
    benches,
    bench_build_10_modules,
    bench_build_50_modules,
    bench_build_nested_packages
);
criterion_main!(benches);
