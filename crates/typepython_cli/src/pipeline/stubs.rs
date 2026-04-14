use super::*;

pub(super) fn build_typepython_stub_contexts(
    syntax_trees: &[typepython_syntax::SyntaxTree],
    _lowered_modules: &[LoweredModule],
    graph: &typepython_graph::ModuleGraph,
) -> BTreeMap<PathBuf, TypePythonStubContext> {
    let mut contexts = syntax_trees
        .iter()
        .filter(|tree| tree.source.kind == SourceKind::TypePython)
        .map(|tree| {
            let mut context = TypePythonStubContext::default();
            collect_value_stub_overrides(&tree.statements, &mut context.value_overrides);
            collect_sealed_stub_metadata(&tree.statements, &mut context.sealed_classes);
            context.guarded_declaration_lines = collect_guarded_declaration_lines(&tree.statements);
            (tree.source.path.clone(), context)
        })
        .collect::<BTreeMap<_, _>>();
    let module_paths = syntax_trees
        .iter()
        .map(|tree| (tree.source.logical_module.clone(), tree.source.path.clone()))
        .collect::<BTreeMap<_, _>>();

    for override_signature in collect_effective_callable_stub_overrides(graph) {
        let Some(path) = module_paths.get(&override_signature.module_key) else {
            continue;
        };
        let Some(context) = contexts.get_mut(path) else {
            continue;
        };
        context.callable_overrides.push(StubCallableOverride {
            line: override_signature.line,
            params: override_signature.params,
            returns: Some(override_signature.returns),
            use_async_syntax: false,
            drop_non_builtin_decorators: true,
        });
    }

    for synthetic_method in collect_synthetic_method_stubs(graph) {
        let Some(path) = module_paths.get(&synthetic_method.module_key) else {
            continue;
        };
        let Some(context) = contexts.get_mut(path) else {
            continue;
        };
        context.synthetic_methods.push(StubSyntheticMethod {
            class_line: synthetic_method.class_line,
            name: synthetic_method.name,
            method_kind: synthetic_method.method_kind,
            params: synthetic_method.params,
            returns: synthetic_method.returns,
        });
    }

    contexts
}

fn collect_value_stub_overrides(
    statements: &[typepython_syntax::SyntaxStatement],
    overrides: &mut Vec<StubValueOverride>,
) {
    for statement in statements {
        match statement {
            typepython_syntax::SyntaxStatement::Value(statement)
                if statement.annotation.is_none()
                    && statement.owner_name.is_none()
                    && statement
                        .rendered_value_type()
                        .as_deref()
                        .is_some_and(|value| !value.is_empty()) =>
            {
                overrides.push(StubValueOverride {
                    line: statement.line,
                    annotation: statement.rendered_value_type().unwrap_or_default(),
                });
            }
            typepython_syntax::SyntaxStatement::Interface(statement)
            | typepython_syntax::SyntaxStatement::DataClass(statement)
            | typepython_syntax::SyntaxStatement::SealedClass(statement)
            | typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                collect_class_member_value_stub_overrides(&statement.members, overrides);
            }
            _ => {}
        }
    }
}

fn collect_class_member_value_stub_overrides(
    members: &[typepython_syntax::ClassMember],
    overrides: &mut Vec<StubValueOverride>,
) {
    for member in members {
        if member.kind == typepython_syntax::ClassMemberKind::Field
            && member.annotation.is_none()
            && member.rendered_value_type().as_deref().is_some_and(|value| !value.is_empty())
        {
            overrides.push(StubValueOverride {
                line: member.line,
                annotation: member.rendered_value_type().unwrap_or_default(),
            });
        }
    }
}

fn collect_sealed_stub_metadata(
    statements: &[typepython_syntax::SyntaxStatement],
    sealed_classes: &mut Vec<StubSealedClass>,
) {
    let class_like = statements
        .iter()
        .filter_map(|statement| match statement {
            typepython_syntax::SyntaxStatement::Interface(statement)
            | typepython_syntax::SyntaxStatement::DataClass(statement)
            | typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                Some((statement.name.clone(), statement.bases.clone()))
            }
            typepython_syntax::SyntaxStatement::SealedClass(statement) => {
                Some((statement.name.clone(), statement.bases.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    for statement in statements.iter().filter_map(|statement| match statement {
        typepython_syntax::SyntaxStatement::SealedClass(statement) => Some(statement),
        _ => None,
    }) {
        let mut members = class_like
            .iter()
            .filter(|(candidate_name, bases)| {
                candidate_name != &statement.name
                    && bases.iter().any(|base| base == &statement.name)
            })
            .map(|(candidate_name, _)| candidate_name.clone())
            .collect::<Vec<_>>();
        members.sort();
        sealed_classes.push(StubSealedClass {
            line: statement.line,
            name: statement.name.clone(),
            members,
        });
    }
}

fn collect_guarded_declaration_lines(
    statements: &[typepython_syntax::SyntaxStatement],
) -> BTreeSet<usize> {
    let guard_ranges = statements
        .iter()
        .filter_map(|statement| match statement {
            typepython_syntax::SyntaxStatement::If(statement) => Some((
                statement.true_start_line,
                statement.true_end_line,
                statement.false_start_line,
                statement.false_end_line,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();

    statements
        .iter()
        .filter_map(|statement| match statement {
            typepython_syntax::SyntaxStatement::TypeAlias(statement) => Some(statement.line),
            typepython_syntax::SyntaxStatement::Interface(statement)
            | typepython_syntax::SyntaxStatement::DataClass(statement)
            | typepython_syntax::SyntaxStatement::SealedClass(statement)
            | typepython_syntax::SyntaxStatement::ClassDef(statement) => Some(statement.line),
            typepython_syntax::SyntaxStatement::FunctionDef(statement)
            | typepython_syntax::SyntaxStatement::OverloadDef(statement) => Some(statement.line),
            _ => None,
        })
        .filter(|line| {
            guard_ranges.iter().any(|(true_start, true_end, false_start, false_end)| {
                (*line >= *true_start && *line <= *true_end)
                    || false_start
                        .zip(*false_end)
                        .is_some_and(|(start, end)| *line >= start && *line <= end)
            })
        })
        .collect()
}
