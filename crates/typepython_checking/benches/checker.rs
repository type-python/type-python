use std::path::PathBuf;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use typepython_binding::{
    AssignmentSite, CallSite, Declaration, DeclarationKind, GenericTypeParam, GenericTypeParamKind,
};
use typepython_checking::check;
use typepython_graph::{ModuleGraph, ModuleNode};
use typepython_syntax::SourceKind;

fn declaration(
    name: &str,
    kind: DeclarationKind,
    detail: &str,
    type_params: Vec<GenericTypeParam>,
) -> Declaration {
    Declaration {
        name: name.to_owned(),
        kind,
        detail: detail.to_owned(),
        value_type: None,
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
        type_params,
    }
}

fn import_declaration(name: &str, target: &str) -> Declaration {
    declaration(name, DeclarationKind::Import, target, Vec::new())
}

fn type_var(name: &str) -> GenericTypeParam {
    GenericTypeParam {
        kind: GenericTypeParamKind::TypeVar,
        name: name.to_owned(),
        bound: None,
        constraints: Vec::new(),
        default: None,
    }
}

fn type_var_tuple(name: &str) -> GenericTypeParam {
    GenericTypeParam {
        kind: GenericTypeParamKind::TypeVarTuple,
        name: name.to_owned(),
        bound: None,
        constraints: Vec::new(),
        default: None,
    }
}

fn call_site(callee: &str, arg_types: Vec<String>, line: usize) -> CallSite {
    CallSite {
        callee: callee.to_owned(),
        arg_count: arg_types.len(),
        arg_types,
        arg_values: Vec::new(),
        starred_arg_types: Vec::new(),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_types: Vec::new(),
        keyword_arg_values: Vec::new(),
        keyword_expansion_types: Vec::new(),
        keyword_expansion_values: Vec::new(),
        line,
    }
}

fn assignment_from_call(
    name: String,
    annotation: &str,
    callee: &str,
    line: usize,
) -> AssignmentSite {
    AssignmentSite {
        name,
        destructuring_target_names: None,
        destructuring_index: None,
        annotation: Some(annotation.to_owned()),
        value_type: Some(String::new()),
        is_awaited: false,
        value_callee: Some(callee.to_owned()),
        value_name: None,
        value_member_owner_name: None,
        value_member_name: None,
        value_member_through_instance: false,
        value_method_owner_name: None,
        value_method_name: None,
        value_method_through_instance: false,
        value_subscript_target: None,
        value_subscript_string_key: None,
        value_subscript_index: None,
        value_if_true: None,
        value_if_false: None,
        value_if_guard: None,
        value_bool_left: None,
        value_bool_right: None,
        value_binop_left: None,
        value_binop_right: None,
        value_binop_operator: None,
        value_lambda: None,
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: None,
        value_set_elements: None,
        value_dict_entries: None,
        owner_name: None,
        owner_type_name: None,
        line,
    }
}

fn make_checker_bench_graph(repetitions: usize) -> ModuleGraph {
    let helper_module = ModuleNode {
        module_path: PathBuf::from("<bench/helpers.pyi>"),
        module_key: String::from("bench.helpers"),
        module_kind: SourceKind::Stub,
        declarations: vec![
            declaration(
                "box_value",
                DeclarationKind::Function,
                "(value:T)->list[T]",
                vec![type_var("T")],
            ),
            declaration(
                "collect",
                DeclarationKind::Function,
                "(*args:Unpack[Ts])->tuple[Unpack[Ts]]",
                vec![type_var_tuple("Ts")],
            ),
            declaration(
                "wrap",
                DeclarationKind::Overload,
                "(value:T)->tuple[T]",
                vec![type_var("T")],
            ),
            declaration(
                "wrap",
                DeclarationKind::Overload,
                "(value:object)->tuple[object]",
                Vec::new(),
            ),
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
        summary_fingerprint: 1,
    };

    let mut calls = Vec::with_capacity(repetitions * 3);
    let mut assignments = Vec::with_capacity(repetitions * 3);
    for index in 0..repetitions {
        let base_line = index * 3 + 1;
        calls.push(call_site("box_value", vec![String::from("int")], base_line));
        assignments.push(assignment_from_call(
            format!("boxed_{index}"),
            "list[int]",
            "box_value",
            base_line,
        ));

        calls.push(call_site(
            "collect",
            vec![String::from("int"), String::from("str")],
            base_line + 1,
        ));
        assignments.push(assignment_from_call(
            format!("collected_{index}"),
            "tuple[int, str]",
            "collect",
            base_line + 1,
        ));

        let (arg_type, expected) = if index % 2 == 0 {
            (String::from("int"), "tuple[int]")
        } else {
            (String::from("str"), "tuple[str]")
        };
        calls.push(call_site("wrap", vec![arg_type], base_line + 2));
        assignments.push(assignment_from_call(
            format!("wrapped_{index}"),
            expected,
            "wrap",
            base_line + 2,
        ));
    }

    let app_module = ModuleNode {
        module_path: PathBuf::from("<bench/app.tpy>"),
        module_key: String::from("bench.app"),
        module_kind: SourceKind::TypePython,
        declarations: vec![
            import_declaration("box_value", "bench.helpers.box_value"),
            import_declaration("collect", "bench.helpers.collect"),
            import_declaration("wrap", "bench.helpers.wrap"),
        ],
        calls,
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
        assignments,
        summary_fingerprint: 2,
    };

    ModuleGraph { nodes: vec![helper_module, app_module] }
}

fn bench_checker_small(c: &mut Criterion) {
    let graph = make_checker_bench_graph(8);
    let result = check(&graph);
    assert!(
        !result.diagnostics.has_errors(),
        "benchmark graph should type-check:\n{}",
        result.diagnostics.as_text()
    );
    c.bench_function("check_solver_direct_calls_small", |b| {
        b.iter(|| black_box(check(black_box(&graph)).diagnostics.has_errors()))
    });
}

fn bench_checker_medium(c: &mut Criterion) {
    let graph = make_checker_bench_graph(64);
    let result = check(&graph);
    assert!(
        !result.diagnostics.has_errors(),
        "benchmark graph should type-check:\n{}",
        result.diagnostics.as_text()
    );
    c.bench_function("check_solver_direct_calls_medium", |b| {
        b.iter(|| black_box(check(black_box(&graph)).diagnostics.has_errors()))
    });
}

criterion_group!(benches, bench_checker_small, bench_checker_medium);
criterion_main!(benches);
