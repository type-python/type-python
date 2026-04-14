use std::path::PathBuf;

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use typepython_binding::{
    AssignmentSite, BindingTable, BoundCallableSignature, BoundImportTarget, CallSite, Declaration,
    DeclarationKind, DeclarationMetadata, GenericTypeParam, GenericTypeParamKind,
    ModuleSurfaceFacts,
};
use typepython_checking::{check, semantic_incremental_state_with_binding_metadata};
use typepython_config::ImportFallback;
use typepython_graph::{ModuleGraph, ModuleNode};
use typepython_incremental::SnapshotMetadata;
use typepython_syntax::{FunctionParam, SourceKind};

fn param(name: &str, annotation: &str) -> FunctionParam {
    FunctionParam {
        name: name.to_owned(),
        annotation: Some(annotation.to_owned()),
        annotation_expr: typepython_syntax::TypeExpr::parse(annotation),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    }
}

fn variadic_param(name: &str, annotation: &str) -> FunctionParam {
    FunctionParam { variadic: true, ..param(name, annotation) }
}

fn function_declaration(
    name: &str,
    kind: DeclarationKind,
    params: Vec<FunctionParam>,
    returns: &str,
    type_params: Vec<GenericTypeParam>,
) -> Declaration {
    let signature = BoundCallableSignature::from_function_parts(&params, Some(returns));
    Declaration {
        metadata: DeclarationMetadata::Callable { signature: signature.clone() },
        name: name.to_owned(),
        kind,
        detail: signature.rendered(),
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
        type_params,
    }
}

fn import_declaration(name: &str, target: &str) -> Declaration {
    Declaration {
        metadata: DeclarationMetadata::Import { target: BoundImportTarget::new(target) },
        name: name.to_owned(),
        kind: DeclarationKind::Import,
        detail: target.to_owned(),
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
    }
}

fn type_var(name: &str) -> GenericTypeParam {
    GenericTypeParam {
        kind: GenericTypeParamKind::TypeVar,
        name: name.to_owned(),
        bound: None,
        bound_expr: None,
        constraints: Vec::new(),
        constraint_exprs: Vec::new(),
        default: None,
        default_expr: None,
    }
}

fn type_var_tuple(name: &str) -> GenericTypeParam {
    GenericTypeParam {
        kind: GenericTypeParamKind::TypeVarTuple,
        name: name.to_owned(),
        bound: None,
        bound_expr: None,
        constraints: Vec::new(),
        constraint_exprs: Vec::new(),
        default: None,
        default_expr: None,
    }
}

fn call_site(callee: &str, arg_types: Vec<String>, line: usize) -> CallSite {
    CallSite {
        callee: callee.to_owned(),
        arg_count: arg_types.len(),
        arg_values: typepython_syntax::direct_expr_metadata_vec_from_type_texts(arg_types),
        starred_arg_values: Vec::new(),
        keyword_names: Vec::new(),
        keyword_arg_values: Vec::new(),
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
        annotation_expr: None,
        value: None,
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

fn bindings_from_graph(graph: &ModuleGraph) -> Vec<BindingTable> {
    graph
        .nodes
        .iter()
        .map(|node| BindingTable {
            module_path: node.module_path.clone(),
            module_key: node.module_key.clone(),
            module_kind: node.module_kind,
            surface_facts: ModuleSurfaceFacts::default(),
            declarations: node.declarations.clone(),
            calls: node.calls.clone(),
            method_calls: node.method_calls.clone(),
            member_accesses: node.member_accesses.clone(),
            returns: node.returns.clone(),
            yields: node.yields.clone(),
            if_guards: node.if_guards.clone(),
            asserts: node.asserts.clone(),
            invalidations: node.invalidations.clone(),
            matches: node.matches.clone(),
            for_loops: node.for_loops.clone(),
            with_statements: node.with_statements.clone(),
            except_handlers: node.except_handlers.clone(),
            assignments: node.assignments.clone(),
        })
        .collect()
}

fn make_checker_bench_graph(repetitions: usize) -> ModuleGraph {
    let helper_module = ModuleNode {
        module_path: PathBuf::from("<bench/helpers.pyi>"),
        module_key: String::from("bench.helpers"),
        module_kind: SourceKind::Stub,
        declarations: vec![
            function_declaration(
                "box_value",
                DeclarationKind::Function,
                vec![param("value", "T")],
                "list[T]",
                vec![type_var("T")],
            ),
            function_declaration(
                "collect",
                DeclarationKind::Function,
                vec![variadic_param("args", "Unpack[Ts]")],
                "tuple[Unpack[Ts]]",
                vec![type_var_tuple("Ts")],
            ),
            function_declaration(
                "wrap",
                DeclarationKind::Overload,
                vec![param("value", "T")],
                "tuple[T]",
                vec![type_var("T")],
            ),
            function_declaration(
                "wrap",
                DeclarationKind::Overload,
                vec![param("value", "object")],
                "tuple[object]",
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

fn bench_semantic_summary_medium(c: &mut Criterion) {
    let graph = make_checker_bench_graph(64);
    let bindings = bindings_from_graph(&graph);
    c.bench_function("check_semantic_incremental_summary_medium", |b| {
        b.iter(|| {
            black_box(semantic_incremental_state_with_binding_metadata(
                black_box(&graph),
                black_box(&bindings),
                ImportFallback::Unknown,
                None,
                None,
                SnapshotMetadata::default(),
            ))
        })
    });
}

criterion_group!(benches, bench_checker_small, bench_checker_medium, bench_semantic_summary_medium);
criterion_main!(benches);
