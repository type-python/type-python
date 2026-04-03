use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub(crate) type GenericTypeParamSubstitutions = GenericSolution;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum GenericSolveFailure {
    UnsupportedStarredTypeVarTupleInference { param_name: String },
    ParamSpecInferenceFailed { annotation: SemanticType, actual: SemanticType },
    TypeBindingInferenceFailed { annotation: SemanticType, actual: SemanticType },
    ParamSpecBindingConflict { param_name: String },
    TypeVarTupleBindingConflict { param_name: String },
    ConstraintViolation { param_name: String, actual: SemanticType, requirement: String },
}

impl GenericSolveFailure {
    pub(crate) fn diagnostic_reason(&self) -> String {
        match self {
            Self::UnsupportedStarredTypeVarTupleInference { param_name } => format!(
                "reason: `*{}` cannot be inferred from starred iterable arguments with unknown fixed length",
                param_name
            ),
            Self::ParamSpecInferenceFailed { annotation, actual } => format!(
                "reason: could not infer callable parameter list from expected `{}` against actual `{}`",
                diagnostic_type_text(annotation),
                diagnostic_type_text(actual)
            ),
            Self::TypeBindingInferenceFailed { annotation, actual } => format!(
                "reason: could not infer generic bindings from expected `{}` against actual `{}`",
                diagnostic_type_text(annotation),
                diagnostic_type_text(actual)
            ),
            Self::ParamSpecBindingConflict { param_name } => format!(
                "reason: inferred callable parameter list for `{}` was inconsistent across this call",
                param_name
            ),
            Self::TypeVarTupleBindingConflict { param_name } => format!(
                "reason: inferred tuple pack for `{}` was inconsistent across this call",
                param_name
            ),
            Self::ConstraintViolation { param_name, actual, requirement } => format!(
                "reason: inferred `{}` as `{}` but it does not satisfy {}",
                param_name,
                diagnostic_type_text(actual),
                requirement
            ),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct GenericSolution {
    pub(crate) types: BTreeMap<String, SemanticType>,
    pub(crate) param_lists: BTreeMap<String, ParamListBinding>,
    pub(crate) type_packs: BTreeMap<String, TypePackBinding>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ParamListBinding {
    pub(crate) params: Vec<typepython_syntax::DirectFunctionParamSite>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct TypePackBinding {
    pub(crate) types: Vec<SemanticType>,
}

#[derive(Debug, Clone)]
struct GenericSolverParam {
    kind: typepython_binding::GenericTypeParamKind,
    name: String,
    bound: Option<String>,
    constraints: Vec<String>,
    default: Option<String>,
}

#[derive(Debug, Clone)]
struct GenericSolverMetadata {
    params: Vec<GenericSolverParam>,
    type_names: BTreeSet<String>,
    param_spec_names: BTreeSet<String>,
    type_pack_names: BTreeSet<String>,
}

impl GenericSolverMetadata {
    fn from_function(function: &Declaration) -> Self {
        Self {
            params: function
                .type_params
                .iter()
                .map(|type_param| GenericSolverParam {
                    kind: type_param.kind.clone(),
                    name: type_param.name.clone(),
                    bound: type_param.bound.clone(),
                    constraints: type_param.constraints.clone(),
                    default: type_param.default.clone(),
                })
                .collect(),
            type_names: function
                .type_params
                .iter()
                .filter(|type_param| {
                    type_param.kind == typepython_binding::GenericTypeParamKind::TypeVar
                })
                .map(|type_param| type_param.name.clone())
                .collect(),
            param_spec_names: function
                .type_params
                .iter()
                .filter(|type_param| {
                    type_param.kind == typepython_binding::GenericTypeParamKind::ParamSpec
                })
                .map(|type_param| type_param.name.clone())
                .collect(),
            type_pack_names: function
                .type_params
                .iter()
                .filter(|type_param| {
                    type_param.kind == typepython_binding::GenericTypeParamKind::TypeVarTuple
                })
                .map(|type_param| type_param.name.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct TypeVarConstraint {
    name: String,
    candidate: SemanticType,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ParamSpecConstraint {
    name: String,
    binding: ParamListBinding,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct TypeVarTupleConstraint {
    name: String,
    binding: TypePackBinding,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum GenericConstraint {
    TypeVar(TypeVarConstraint),
    ParamSpec(ParamSpecConstraint),
    TypeVarTuple(TypeVarTupleConstraint),
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
struct GenericConstraintSet {
    constraints: Vec<GenericConstraint>,
}

impl GenericConstraintSet {
    fn extend_bindings(&mut self, bindings: GenericSolution) {
        for (name, candidate) in bindings.types {
            self.constraints
                .push(GenericConstraint::TypeVar(TypeVarConstraint { name, candidate }));
        }
        for (name, binding) in bindings.param_lists {
            self.constraints
                .push(GenericConstraint::ParamSpec(ParamSpecConstraint { name, binding }));
        }
        for (name, binding) in bindings.type_packs {
            self.constraints
                .push(GenericConstraint::TypeVarTuple(TypeVarTupleConstraint { name, binding }));
        }
    }

    fn push_type_pack_binding(&mut self, name: &str, binding: TypePackBinding) {
        self.constraints.push(GenericConstraint::TypeVarTuple(TypeVarTupleConstraint {
            name: name.to_owned(),
            binding,
        }));
    }
}

#[derive(Debug, Clone)]
struct GenericSolverState {
    metadata: GenericSolverMetadata,
    constraints: GenericConstraintSet,
}

impl GenericSolverState {
    fn new(function: &Declaration) -> Self {
        Self {
            metadata: GenericSolverMetadata::from_function(function),
            constraints: GenericConstraintSet::default(),
        }
    }

    fn current_bindings_detailed(&self) -> Result<GenericSolution, GenericSolveFailure> {
        solve_collected_generic_constraints_detailed(&self.constraints)
    }

    fn record_bindings(&mut self, bindings: GenericSolution) {
        self.constraints.extend_bindings(bindings);
    }

    fn record_type_pack_binding(&mut self, name: &str, binding: TypePackBinding) {
        self.constraints.push_type_pack_binding(name, binding);
    }

    fn finish_detailed(
        &self,
        node: &typepython_graph::ModuleNode,
        nodes: &[typepython_graph::ModuleNode],
    ) -> Result<GenericSolution, GenericSolveFailure> {
        finalize_generic_solution_detailed(node, nodes, &self.metadata, &self.constraints)
    }
}

fn solve_collected_generic_constraints_detailed(
    constraints: &GenericConstraintSet,
) -> Result<GenericSolution, GenericSolveFailure> {
    let mut solution = GenericSolution::default();
    for constraint in &constraints.constraints {
        match constraint {
            GenericConstraint::TypeVar(constraint) => match solution.types.get(&constraint.name) {
                Some(existing) if existing != &constraint.candidate => {
                    solution.types.insert(
                        constraint.name.clone(),
                        merge_generic_type_candidates(existing, &constraint.candidate),
                    );
                }
                Some(_) => {}
                None => {
                    solution.types.insert(constraint.name.clone(), constraint.candidate.clone());
                }
            },
            GenericConstraint::ParamSpec(constraint) => {
                insert_param_spec_binding(
                    &mut solution,
                    &constraint.name,
                    constraint.binding.clone(),
                )
                .ok_or_else(|| GenericSolveFailure::ParamSpecBindingConflict {
                    param_name: constraint.name.clone(),
                })?;
            }
            GenericConstraint::TypeVarTuple(constraint) => {
                insert_type_pack_binding(
                    &mut solution,
                    &constraint.name,
                    constraint.binding.clone(),
                )
                .ok_or_else(|| {
                    GenericSolveFailure::TypeVarTupleBindingConflict {
                        param_name: constraint.name.clone(),
                    }
                })?;
            }
        }
    }
    Ok(solution)
}

fn generic_type_param_requirement(type_param: &GenericSolverParam) -> String {
    if let Some(bound) = &type_param.bound {
        format!("bound `{}`", normalize_type_text(bound))
    } else if !type_param.constraints.is_empty() {
        format!(
            "constraint list `{}`",
            type_param
                .constraints
                .iter()
                .map(|constraint| normalize_type_text(constraint))
                .collect::<Vec<_>>()
                .join(" | ")
        )
    } else {
        String::from("its declared constraints")
    }
}

fn finalize_generic_solution_detailed(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    metadata: &GenericSolverMetadata,
    constraints: &GenericConstraintSet,
) -> Result<GenericSolution, GenericSolveFailure> {
    let mut substitutions = solve_collected_generic_constraints_detailed(constraints)?;

    for type_param in &metadata.params {
        match type_param.kind {
            typepython_binding::GenericTypeParamKind::TypeVar => {
                if !substitutions.types.contains_key(&type_param.name)
                    && let Some(default) = &type_param.default
                {
                    substitutions
                        .types
                        .insert(type_param.name.clone(), lower_type_text_or_name(default));
                }
                let Some(actual) = substitutions.types.get(&type_param.name) else {
                    continue;
                };
                if !generic_type_param_accepts_actual(
                    node,
                    nodes,
                    &typepython_binding::GenericTypeParam {
                        kind: type_param.kind.clone(),
                        name: type_param.name.clone(),
                        bound: type_param.bound.clone(),
                        constraints: type_param.constraints.clone(),
                        default: type_param.default.clone(),
                    },
                    actual,
                ) {
                    return Err(GenericSolveFailure::ConstraintViolation {
                        param_name: type_param.name.clone(),
                        actual: actual.clone(),
                        requirement: generic_type_param_requirement(type_param),
                    });
                }
            }
            typepython_binding::GenericTypeParamKind::ParamSpec => {
                if substitutions.param_lists.contains_key(&type_param.name) {
                    continue;
                }
                let Some(default) = &type_param.default else {
                    continue;
                };
                substitutions.param_lists.insert(
                    type_param.name.clone(),
                    param_list_binding_from_default(default).ok_or_else(|| {
                        GenericSolveFailure::ParamSpecInferenceFailed {
                            annotation: SemanticType::Name(type_param.name.clone()),
                            actual: lower_type_text_or_name(default),
                        }
                    })?,
                );
            }
            typepython_binding::GenericTypeParamKind::TypeVarTuple => {
                if substitutions.type_packs.contains_key(&type_param.name) {
                    continue;
                }
                let Some(default) = &type_param.default else {
                    continue;
                };
                substitutions.type_packs.insert(
                    type_param.name.clone(),
                    type_pack_binding_from_default(default).ok_or_else(|| {
                        GenericSolveFailure::TypeVarTupleBindingConflict {
                            param_name: type_param.name.clone(),
                        }
                    })?,
                );
            }
        }
    }

    Ok(substitutions)
}

pub(crate) fn finalize_generic_type_param_substitutions_detailed(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    substitutions: GenericTypeParamSubstitutions,
) -> Result<GenericTypeParamSubstitutions, GenericSolveFailure> {
    let metadata = GenericSolverMetadata::from_function(function);
    let mut constraints = GenericConstraintSet::default();
    constraints.extend_bindings(substitutions);
    finalize_generic_solution_detailed(node, nodes, &metadata, &constraints)
}

#[allow(dead_code)]
pub(crate) fn infer_generic_type_param_substitutions(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    signature: &[typepython_syntax::DirectFunctionParamSite],
    call: &typepython_binding::CallSite,
) -> Option<GenericTypeParamSubstitutions> {
    infer_generic_type_param_substitutions_detailed(node, nodes, function, signature, call).ok()
}

pub(crate) fn infer_generic_type_param_substitutions_detailed(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    signature: &[typepython_syntax::DirectFunctionParamSite],
    call: &typepython_binding::CallSite,
) -> Result<GenericTypeParamSubstitutions, GenericSolveFailure> {
    let mut solver = GenericSolverState::new(function);
    let expected_positional_arg_types =
        expected_positional_arg_types_from_signature_sites(signature, call.arg_count);
    let (positional_types, variadic_starred_types) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    let mut positional_index = 0;

    for param in signature.iter().filter(|param| !param.keyword_only && !param.keyword_variadic) {
        let Some(annotation_text) = param.annotation.as_deref() else {
            if param.variadic {
                positional_index = positional_types.len();
            } else if positional_index < positional_types.len() {
                positional_index += 1;
            }
            continue;
        };
        let annotation = lower_type_text_or_name(annotation_text);
        if param.variadic {
            if extract_param_spec_args_name_from_semantic(&annotation).is_some() {
                positional_index = positional_types.len();
                continue;
            }
            if let Some(type_pack_name) = type_pack_name_from_unpack_semantic_annotation(
                &annotation,
                &solver.metadata.type_pack_names,
            ) {
                if !variadic_starred_types.is_empty() {
                    return Err(GenericSolveFailure::UnsupportedStarredTypeVarTupleInference {
                        param_name: type_pack_name,
                    });
                }
                solver.record_type_pack_binding(
                    &type_pack_name,
                    TypePackBinding {
                        types: positional_types[positional_index..]
                            .iter()
                            .map(|ty| lower_type_text_or_name(ty))
                            .collect(),
                    },
                );
                positional_index = positional_types.len();
                continue;
            }
            let existing = solver.current_bindings_detailed()?;
            for actual in positional_types.iter().skip(positional_index) {
                let actual_type = lower_type_text_or_name(actual);
                let bindings = infer_generic_type_param_bindings(
                    node,
                    nodes,
                    &annotation,
                    &actual_type,
                    &solver.metadata.type_names,
                    &existing,
                    &solver.metadata.type_pack_names,
                )
                .ok_or_else(|| {
                    GenericSolveFailure::TypeBindingInferenceFailed {
                        annotation: annotation.clone(),
                        actual: actual_type,
                    }
                })?;
                solver.record_bindings(bindings);
            }
            positional_index = positional_types.len();
            continue;
        }
        let Some(actual) = positional_types.get(positional_index) else {
            continue;
        };
        let annotation_mentions_param_spec =
            annotation.callable_parts().is_some_and(|(params, _)| {
                callable_param_expr_mentions_param_spec_semantic(
                    params,
                    &solver.metadata.param_spec_names,
                )
            });
        let existing = solver.current_bindings_detailed()?;
        let actual_type = lower_type_text_or_name(actual);
        let bindings = infer_callable_param_spec_bindings(
            node,
            nodes,
            &annotation,
            &actual_type,
            call.arg_values.get(positional_index),
            &solver.metadata.type_names,
            &solver.metadata.param_spec_names,
            &existing,
        )
        .ok_or_else(|| GenericSolveFailure::ParamSpecInferenceFailed {
            annotation: annotation.clone(),
            actual: actual_type.clone(),
        })?;
        solver.record_bindings(bindings);
        if annotation_mentions_param_spec {
            positional_index += 1;
            continue;
        }
        let existing = solver.current_bindings_detailed()?;
        let bindings = infer_generic_type_param_bindings(
            node,
            nodes,
            &annotation,
            &actual_type,
            &solver.metadata.type_names,
            &existing,
            &solver.metadata.type_pack_names,
        )
        .ok_or_else(|| GenericSolveFailure::TypeBindingInferenceFailed {
            annotation: annotation.clone(),
            actual: actual_type,
        })?;
        solver.record_bindings(bindings);
        positional_index += 1;
    }

    for (index, (keyword, actual)) in
        call.keyword_names.iter().zip(&call.keyword_arg_types).enumerate()
    {
        let Some(param) = signature.iter().find(|param| param.name == *keyword) else {
            continue;
        };
        let Some(annotation_text) = param.annotation.as_deref() else {
            continue;
        };
        let annotation = lower_type_text_or_name(annotation_text);
        let annotation_mentions_param_spec =
            annotation.callable_parts().is_some_and(|(params, _)| {
                callable_param_expr_mentions_param_spec_semantic(
                    params,
                    &solver.metadata.param_spec_names,
                )
            });
        let existing = solver.current_bindings_detailed()?;
        let actual_type = lower_type_text_or_name(actual);
        let bindings = infer_callable_param_spec_bindings(
            node,
            nodes,
            &annotation,
            &actual_type,
            call.keyword_arg_values.get(index),
            &solver.metadata.type_names,
            &solver.metadata.param_spec_names,
            &existing,
        )
        .ok_or_else(|| GenericSolveFailure::ParamSpecInferenceFailed {
            annotation: annotation.clone(),
            actual: actual_type.clone(),
        })?;
        solver.record_bindings(bindings);
        if annotation_mentions_param_spec {
            continue;
        }
        let existing = solver.current_bindings_detailed()?;
        let bindings = infer_generic_type_param_bindings(
            node,
            nodes,
            &annotation,
            &actual_type,
            &solver.metadata.type_names,
            &existing,
            &solver.metadata.type_pack_names,
        )
        .ok_or_else(|| GenericSolveFailure::TypeBindingInferenceFailed {
            annotation: annotation.clone(),
            actual: actual_type,
        })?;
        solver.record_bindings(bindings);
    }

    solver.finish_detailed(node, nodes)
}

pub(crate) fn infer_generic_type_param_substitutions_from_semantic_params_detailed(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    function: &Declaration,
    params: &[SemanticCallableParam],
    call: &typepython_binding::CallSite,
) -> Result<GenericTypeParamSubstitutions, GenericSolveFailure> {
    let mut solver = GenericSolverState::new(function);
    let expected_positional_arg_types =
        expected_positional_arg_types_from_semantic_params(params, call.arg_count);
    let (positional_types, variadic_starred_types) =
        expanded_positional_arg_types(node, nodes, call, &expected_positional_arg_types);
    let mut positional_index = 0;

    for param in params.iter().filter(|param| !param.keyword_only && !param.keyword_variadic) {
        let Some(annotation) = param.annotation.clone() else {
            if param.variadic {
                positional_index = positional_types.len();
            } else if positional_index < positional_types.len() {
                positional_index += 1;
            }
            continue;
        };
        if param.variadic {
            if extract_param_spec_args_name_from_semantic(&annotation).is_some() {
                positional_index = positional_types.len();
                continue;
            }
            if let Some(type_pack_name) = type_pack_name_from_unpack_semantic_annotation(
                &annotation,
                &solver.metadata.type_pack_names,
            ) {
                if !variadic_starred_types.is_empty() {
                    return Err(GenericSolveFailure::UnsupportedStarredTypeVarTupleInference {
                        param_name: type_pack_name,
                    });
                }
                solver.record_type_pack_binding(
                    &type_pack_name,
                    TypePackBinding {
                        types: positional_types[positional_index..]
                            .iter()
                            .map(|ty| lower_type_text_or_name(ty))
                            .collect(),
                    },
                );
                positional_index = positional_types.len();
                continue;
            }
            let existing = solver.current_bindings_detailed()?;
            for actual in positional_types.iter().skip(positional_index) {
                let actual_type = lower_type_text_or_name(actual);
                let bindings = infer_generic_type_param_bindings(
                    node,
                    nodes,
                    &annotation,
                    &actual_type,
                    &solver.metadata.type_names,
                    &existing,
                    &solver.metadata.type_pack_names,
                )
                .ok_or_else(|| {
                    GenericSolveFailure::TypeBindingInferenceFailed {
                        annotation: annotation.clone(),
                        actual: actual_type,
                    }
                })?;
                solver.record_bindings(bindings);
            }
            positional_index = positional_types.len();
            continue;
        }
        let Some(actual) = positional_types.get(positional_index) else {
            continue;
        };
        let annotation_mentions_param_spec =
            annotation.callable_parts().is_some_and(|(params, _)| {
                callable_param_expr_mentions_param_spec_semantic(
                    params,
                    &solver.metadata.param_spec_names,
                )
            });
        let existing = solver.current_bindings_detailed()?;
        let actual_type = lower_type_text_or_name(actual);
        let bindings = infer_callable_param_spec_bindings(
            node,
            nodes,
            &annotation,
            &actual_type,
            call.arg_values.get(positional_index),
            &solver.metadata.type_names,
            &solver.metadata.param_spec_names,
            &existing,
        )
        .ok_or_else(|| GenericSolveFailure::ParamSpecInferenceFailed {
            annotation: annotation.clone(),
            actual: actual_type.clone(),
        })?;
        solver.record_bindings(bindings);
        if annotation_mentions_param_spec {
            positional_index += 1;
            continue;
        }
        let existing = solver.current_bindings_detailed()?;
        let bindings = infer_generic_type_param_bindings(
            node,
            nodes,
            &annotation,
            &actual_type,
            &solver.metadata.type_names,
            &existing,
            &solver.metadata.type_pack_names,
        )
        .ok_or_else(|| GenericSolveFailure::TypeBindingInferenceFailed {
            annotation: annotation.clone(),
            actual: actual_type,
        })?;
        solver.record_bindings(bindings);
        positional_index += 1;
    }

    for (index, (keyword, actual)) in
        call.keyword_names.iter().zip(&call.keyword_arg_types).enumerate()
    {
        let Some(param) = params.iter().find(|param| param.name == *keyword) else {
            continue;
        };
        let Some(annotation) = param.annotation.clone() else {
            continue;
        };
        let annotation_mentions_param_spec =
            annotation.callable_parts().is_some_and(|(params, _)| {
                callable_param_expr_mentions_param_spec_semantic(
                    params,
                    &solver.metadata.param_spec_names,
                )
            });
        let existing = solver.current_bindings_detailed()?;
        let actual_type = lower_type_text_or_name(actual);
        let bindings = infer_callable_param_spec_bindings(
            node,
            nodes,
            &annotation,
            &actual_type,
            call.keyword_arg_values.get(index),
            &solver.metadata.type_names,
            &solver.metadata.param_spec_names,
            &existing,
        )
        .ok_or_else(|| GenericSolveFailure::ParamSpecInferenceFailed {
            annotation: annotation.clone(),
            actual: actual_type.clone(),
        })?;
        solver.record_bindings(bindings);
        if annotation_mentions_param_spec {
            continue;
        }
        let existing = solver.current_bindings_detailed()?;
        let bindings = infer_generic_type_param_bindings(
            node,
            nodes,
            &annotation,
            &actual_type,
            &solver.metadata.type_names,
            &existing,
            &solver.metadata.type_pack_names,
        )
        .ok_or_else(|| GenericSolveFailure::TypeBindingInferenceFailed {
            annotation: annotation.clone(),
            actual: actual_type,
        })?;
        solver.record_bindings(bindings);
    }

    solver.finish_detailed(node, nodes)
}

pub(crate) fn instantiate_direct_function_signature(
    signature: &[typepython_syntax::DirectFunctionParamSite],
    substitutions: &GenericTypeParamSubstitutions,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    let mut instantiated = Vec::new();
    let mut expanded_param_specs = BTreeSet::new();

    for param in signature {
        let param_spec_name = param.annotation.as_deref().and_then(|annotation| {
            if param.variadic {
                extract_param_spec_args_name(annotation)
            } else if param.keyword_variadic {
                extract_param_spec_kwargs_name(annotation)
            } else {
                None
            }
        });
        if let Some(param_spec_name) = param_spec_name {
            let binding = substitutions.param_lists.get(param_spec_name)?;
            if expanded_param_specs.insert(param_spec_name.to_owned()) {
                instantiated.extend(
                    binding
                        .params
                        .iter()
                        .cloned()
                        .map(|param| instantiate_direct_function_param(param, substitutions)),
                );
            }
            continue;
        }
        if param.variadic
            && let Some(annotation) = param.annotation.as_deref()
            && let Some(type_pack_name) = unpack_inner(annotation)
            && let Some(binding) = substitutions.type_packs.get(type_pack_name.trim())
        {
            instantiated.extend(binding.types.iter().enumerate().map(|(index, element_type)| {
                typepython_syntax::DirectFunctionParamSite {
                    name: format!("{}{}", param.name, index),
                    annotation: Some(render_semantic_type(element_type)),
                    has_default: false,
                    positional_only: true,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                }
            }));
            continue;
        }
        instantiated.push(instantiate_direct_function_param(param.clone(), substitutions));
    }

    Some(instantiated)
}

pub(crate) fn instantiate_direct_function_param(
    mut param: typepython_syntax::DirectFunctionParamSite,
    substitutions: &GenericTypeParamSubstitutions,
) -> typepython_syntax::DirectFunctionParamSite {
    param.annotation = instantiate_direct_function_param_annotation(&param, substitutions)
        .map(|annotation| render_semantic_type(&annotation));
    param
}

pub(crate) fn instantiate_direct_function_param_annotation(
    param: &typepython_syntax::DirectFunctionParamSite,
    substitutions: &GenericTypeParamSubstitutions,
) -> Option<SemanticType> {
    param
        .annotation
        .as_deref()
        .map(|annotation| instantiate_semantic_annotation(annotation, substitutions))
}

pub(crate) fn instantiate_semantic_annotation(
    annotation: &str,
    substitutions: &GenericTypeParamSubstitutions,
) -> SemanticType {
    substitute_semantic_type_params(&lower_type_text_or_name(annotation), substitutions)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn infer_callable_param_spec_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &SemanticType,
    actual: &SemanticType,
    actual_value: Option<&typepython_syntax::DirectExprMetadata>,
    type_names: &BTreeSet<String>,
    param_spec_names: &BTreeSet<String>,
    existing: &GenericTypeParamSubstitutions,
) -> Option<GenericTypeParamSubstitutions> {
    if param_spec_names.is_empty() {
        return Some(GenericTypeParamSubstitutions::default());
    }

    let Some((expected_params_expr, expected_return)) = annotation.callable_parts() else {
        return Some(GenericTypeParamSubstitutions::default());
    };
    if !callable_param_expr_mentions_param_spec_semantic(expected_params_expr, param_spec_names) {
        return Some(GenericTypeParamSubstitutions::default());
    }

    let (actual_binding, actual_return) =
        resolve_callable_shape_from_actual(node, nodes, actual, actual_value)?;
    let mut bindings = infer_callable_param_expr_bindings(
        node,
        nodes,
        expected_params_expr,
        &actual_binding,
        type_names,
        param_spec_names,
        existing,
    )?;
    let combined = combine_generic_substitutions(existing, &bindings);
    merge_nested_generic_bindings(
        &mut bindings,
        infer_generic_type_param_bindings(
            node,
            nodes,
            expected_return,
            &actual_return,
            type_names,
            &combined,
            &BTreeSet::new(),
        )?,
    )?;
    Some(bindings)
}

pub(crate) fn callable_param_expr_mentions_param_spec_semantic(
    params_expr: &SemanticCallableParams,
    param_spec_names: &BTreeSet<String>,
) -> bool {
    match params_expr {
        SemanticCallableParams::Single(expr) => {
            matches!(expr.as_ref(), SemanticType::Name(name) if param_spec_names.contains(name.trim()))
        }
        SemanticCallableParams::Concatenate(parts) => parts
            .last()
            .is_some_and(|tail| matches!(tail, SemanticType::Name(name) if param_spec_names.contains(name.trim()))),
        _ => false,
    }
}

pub(crate) fn infer_callable_param_expr_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_params_expr: &SemanticCallableParams,
    actual_binding: &ParamListBinding,
    type_names: &BTreeSet<String>,
    param_spec_names: &BTreeSet<String>,
    existing: &GenericTypeParamSubstitutions,
) -> Option<GenericTypeParamSubstitutions> {
    match expected_params_expr {
        SemanticCallableParams::Single(expr) => {
            if let SemanticType::Name(name) = expr.as_ref()
                && param_spec_names.contains(name.trim())
            {
                let mut bindings = GenericTypeParamSubstitutions::default();
                insert_param_spec_binding(&mut bindings, name.trim(), actual_binding.clone())?;
                return Some(bindings);
            }
        }
        SemanticCallableParams::Concatenate(parts) => {
            let (tail, prefixes) = parts.split_last()?;
            let SemanticType::Name(tail) = tail else {
                return None;
            };
            if !param_spec_names.contains(tail.trim())
                || actual_binding.params.len() < prefixes.len()
            {
                return None;
            }
            let mut bindings = GenericTypeParamSubstitutions::default();
            for (expected_prefix, actual_param) in prefixes.iter().zip(actual_binding.params.iter())
            {
                let combined = combine_generic_substitutions(existing, &bindings);
                merge_nested_generic_bindings(
                    &mut bindings,
                    infer_generic_type_param_bindings(
                        node,
                        nodes,
                        expected_prefix,
                        &param_annotation_semantic_type(actual_param),
                        type_names,
                        &combined,
                        &BTreeSet::new(),
                    )?,
                )?;
            }
            insert_param_spec_binding(
                &mut bindings,
                tail.trim(),
                ParamListBinding { params: actual_binding.params[prefixes.len()..].to_vec() },
            )?;
            return Some(bindings);
        }
        _ => {}
    }

    Some(GenericTypeParamSubstitutions::default())
}

pub(crate) fn insert_param_spec_binding(
    substitutions: &mut GenericTypeParamSubstitutions,
    name: &str,
    binding: ParamListBinding,
) -> Option<()> {
    match substitutions.param_lists.get(name) {
        Some(existing) if existing != &binding => None,
        Some(_) => Some(()),
        None => {
            substitutions.param_lists.insert(name.to_owned(), binding);
            Some(())
        }
    }
}

pub(crate) fn resolve_callable_shape_from_actual(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    actual: &SemanticType,
    actual_value: Option<&typepython_syntax::DirectExprMetadata>,
) -> Option<(ParamListBinding, SemanticType)> {
    if let Some(actual_value) = actual_value
        && let Some(shape) = resolve_callable_shape_from_metadata(node, nodes, actual_value, actual)
    {
        return Some(shape);
    }

    let (params, return_type) = actual.callable_parts()?;
    let SemanticCallableParams::ParamList(param_types) = params else {
        return None;
    };
    Some((
        ParamListBinding { params: synthesize_semantic_param_list_binding(param_types.clone()) },
        return_type.clone(),
    ))
}

pub(crate) fn resolve_callable_shape_from_metadata(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    actual_value: &typepython_syntax::DirectExprMetadata,
    actual: &SemanticType,
) -> Option<(ParamListBinding, SemanticType)> {
    if let Some(lambda) = actual_value.value_lambda.as_deref() {
        let (params, return_type) = actual.callable_parts()?;
        let SemanticCallableParams::ParamList(param_types) = params else {
            return None;
        };
        if param_types.len() != lambda.params.len() {
            return None;
        }
        let params = lambda
            .params
            .iter()
            .zip(param_types.iter())
            .map(|(param, annotation)| typepython_syntax::DirectFunctionParamSite {
                name: param.name.clone(),
                annotation: Some(render_semantic_type(annotation)),
                has_default: param.has_default,
                positional_only: param.positional_only,
                keyword_only: param.keyword_only,
                variadic: param.variadic,
                keyword_variadic: param.keyword_variadic,
            })
            .collect();
        return Some((ParamListBinding { params }, return_type.clone()));
    }

    let function_name = actual_value.value_name.as_deref()?;
    if let Some(callable_type) = resolve_decorated_function_callable_semantic_type_with_context(
        &CheckerContext::new(nodes, ImportFallback::Unknown, None),
        node,
        nodes,
        function_name,
    ) {
        let signature = direct_function_signature_sites_from_semantic_callable(&callable_type)?;
        let return_type =
            decorated_function_return_semantic_type_from_semantic_callable(&callable_type)?;
        return Some((ParamListBinding { params: signature }, return_type));
    }
    let function = resolve_direct_function(node, nodes, function_name)?;
    Some((
        ParamListBinding { params: declaration_signature_sites(function) },
        declaration_signature_return_semantic_type(function)?,
    ))
}

pub(crate) fn synthesize_param_list_binding(
    param_types: Vec<String>,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    param_types
        .into_iter()
        .enumerate()
        .map(|(index, annotation)| typepython_syntax::DirectFunctionParamSite {
            name: format!("arg{index}"),
            annotation: Some(annotation),
            has_default: false,
            positional_only: false,
            keyword_only: false,
            variadic: false,
            keyword_variadic: false,
        })
        .collect()
}

pub(crate) fn synthesize_semantic_param_list_binding(
    param_types: Vec<SemanticType>,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    param_types
        .into_iter()
        .enumerate()
        .map(|(index, annotation)| typepython_syntax::DirectFunctionParamSite {
            name: format!("arg{index}"),
            annotation: Some(render_semantic_type(&annotation)),
            has_default: false,
            positional_only: false,
            keyword_only: false,
            variadic: false,
            keyword_variadic: false,
        })
        .collect()
}

pub(crate) fn param_list_binding_from_default(default: &str) -> Option<ParamListBinding> {
    let default = normalize_callable_param_expr(default);
    if default == "..." {
        return Some(ParamListBinding { params: Vec::new() });
    }
    if let Some(inner) = default.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')) {
        let params = if inner.trim().is_empty() {
            Vec::new()
        } else {
            synthesize_param_list_binding(
                split_top_level_type_args(inner).into_iter().map(normalize_type_text).collect(),
            )
        };
        return Some(ParamListBinding { params });
    }
    None
}

pub(crate) fn extract_param_spec_args_name(annotation: &str) -> Option<&str> {
    annotation.strip_suffix(".args").map(str::trim).filter(|name| !name.is_empty())
}

pub(crate) fn extract_param_spec_kwargs_name(annotation: &str) -> Option<&str> {
    annotation.strip_suffix(".kwargs").map(str::trim).filter(|name| !name.is_empty())
}

pub(crate) fn extract_param_spec_args_name_from_semantic(
    annotation: &SemanticType,
) -> Option<&str> {
    match annotation.strip_annotated() {
        SemanticType::Name(name) => extract_param_spec_args_name(name),
        _ => None,
    }
}

pub(crate) fn generic_type_param_accepts_actual(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_param: &typepython_binding::GenericTypeParam,
    actual: &SemanticType,
) -> bool {
    if let Some(bound) = &type_param.bound {
        return semantic_type_is_assignable(node, nodes, &lower_type_text_or_name(bound), actual);
    }
    if !type_param.constraints.is_empty() {
        return type_param
            .constraints
            .iter()
            .map(|constraint| lower_type_text_or_name(constraint))
            .any(|constraint| semantic_type_is_assignable(node, nodes, &constraint, actual));
    }
    true
}

pub(crate) fn infer_generic_type_param_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &SemanticType,
    actual: &SemanticType,
    generic_names: &BTreeSet<String>,
    substitutions: &GenericTypeParamSubstitutions,
    type_pack_names: &BTreeSet<String>,
) -> Option<GenericTypeParamSubstitutions> {
    infer_generic_type_param_bindings_full(
        node,
        nodes,
        annotation,
        actual,
        generic_names,
        substitutions,
        type_pack_names,
    )
}

fn infer_generic_type_param_bindings_full(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &SemanticType,
    actual: &SemanticType,
    generic_names: &BTreeSet<String>,
    substitutions: &GenericTypeParamSubstitutions,
    type_pack_names: &BTreeSet<String>,
) -> Option<GenericTypeParamSubstitutions> {
    if let SemanticType::Name(name) = &actual
        && name.is_empty()
    {
        return Some(GenericTypeParamSubstitutions::default());
    }

    if let SemanticType::Name(name) = &annotation
        && generic_names.contains(name)
    {
        let candidate = substitutions
            .types
            .get(name)
            .map(|existing| merge_generic_type_candidates(existing, actual))
            .unwrap_or_else(|| actual.clone());
        let mut inferred = GenericTypeParamSubstitutions::default();
        inferred.types.insert(name.clone(), candidate);
        return Some(inferred);
    }

    if let Some(branches) = semantic_union_branches(actual)
        && branches.len() > 1
    {
        let mut candidates = Vec::new();
        for branch in branches {
            let candidate = infer_generic_type_param_bindings_full(
                node,
                nodes,
                annotation,
                &branch,
                generic_names,
                substitutions,
                type_pack_names,
            )?;
            let combined = combine_generic_substitutions(substitutions, &candidate);
            let substituted_annotation = substitute_semantic_type_params(annotation, &combined);
            if !semantic_type_is_assignable(node, nodes, &substituted_annotation, &branch) {
                return None;
            }
            candidates.push(candidate);
        }
        return merge_union_branch_bindings(candidates);
    }

    if let Some(branches) = semantic_union_branches(annotation)
        && branches.len() > 1
    {
        let candidates = branches
            .into_iter()
            .filter_map(|branch| {
                let candidate = infer_generic_type_param_bindings_full(
                    node,
                    nodes,
                    &branch,
                    actual,
                    generic_names,
                    substitutions,
                    type_pack_names,
                )?;
                let combined = combine_generic_substitutions(substitutions, &candidate);
                let substituted_branch = substitute_semantic_type_params(&branch, &combined);
                semantic_type_is_assignable(node, nodes, &substituted_branch, actual)
                    .then_some(candidate)
            })
            .collect::<Vec<_>>();
        return select_best_union_branch_binding(candidates);
    }

    infer_generic_type_param_bindings_semantic(
        node,
        nodes,
        annotation,
        actual,
        generic_names,
        substitutions,
        type_pack_names,
    )
}

pub(crate) fn infer_generic_type_param_bindings_semantic(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    annotation: &SemanticType,
    actual: &SemanticType,
    generic_names: &BTreeSet<String>,
    substitutions: &GenericTypeParamSubstitutions,
    type_pack_names: &BTreeSet<String>,
) -> Option<GenericTypeParamSubstitutions> {
    match (annotation.generic_parts(), actual.generic_parts()) {
        (Some((expected_head, expected_args)), Some((actual_head, actual_args)))
            if expected_head == actual_head =>
        {
            infer_generic_type_arg_bindings(
                node,
                nodes,
                expected_args,
                actual_args,
                generic_names,
                substitutions,
                type_pack_names,
            )
        }
        _ => semantic_type_is_assignable(node, nodes, annotation, actual)
            .then_some(GenericTypeParamSubstitutions::default()),
    }
}

pub(crate) fn combine_generic_substitutions(
    existing: &GenericTypeParamSubstitutions,
    inferred: &GenericTypeParamSubstitutions,
) -> GenericTypeParamSubstitutions {
    let mut combined = existing.clone();
    combined.types.extend(inferred.types.clone());
    combined.param_lists.extend(inferred.param_lists.clone());
    combined.type_packs.extend(inferred.type_packs.clone());
    combined
}

pub(crate) fn select_best_union_branch_binding(
    candidates: Vec<GenericTypeParamSubstitutions>,
) -> Option<GenericTypeParamSubstitutions> {
    let min_len = candidates.iter().map(generic_binding_count).min()?;
    let mut filtered =
        candidates.into_iter().filter(|candidate| generic_binding_count(candidate) == min_len);
    let first = filtered.next()?;
    if filtered.all(|candidate| candidate == first) { Some(first) } else { None }
}

pub(crate) fn merge_union_branch_bindings(
    candidates: Vec<GenericTypeParamSubstitutions>,
) -> Option<GenericTypeParamSubstitutions> {
    let mut merged = GenericTypeParamSubstitutions::default();
    for candidate in candidates {
        for (name, actual_type) in candidate.types {
            match merged.types.get(&name) {
                Some(existing) if existing == &actual_type => {}
                Some(existing) => {
                    merged.types.insert(
                        name,
                        join_semantic_type_candidates(vec![existing.clone(), actual_type]),
                    );
                }
                None => {
                    merged.types.insert(name, actual_type);
                }
            }
        }
        for (name, binding) in candidate.type_packs {
            insert_type_pack_binding(&mut merged, &name, binding)?;
        }
    }
    Some(merged)
}

pub(crate) fn merge_generic_type_candidates(
    existing: &SemanticType,
    actual: &SemanticType,
) -> SemanticType {
    if existing == actual {
        existing.clone()
    } else {
        join_semantic_type_candidates(vec![existing.clone(), actual.clone()])
    }
}

pub(crate) fn substitute_semantic_type_params(
    annotation: &SemanticType,
    substitutions: &GenericTypeParamSubstitutions,
) -> SemanticType {
    match annotation {
        SemanticType::Name(name) => substitutions
            .types
            .get(name)
            .cloned()
            .unwrap_or_else(|| SemanticType::Name(name.clone())),
        SemanticType::Generic { head, args } => SemanticType::Generic {
            head: head.clone(),
            args: expand_substituted_semantic_generic_args(args, substitutions),
        },
        SemanticType::Callable { params, return_type } => SemanticType::Callable {
            params: substitute_semantic_callable_param_expr(params, substitutions),
            return_type: Box::new(substitute_semantic_type_params(return_type, substitutions)),
        },
        SemanticType::Union(branches) => join_semantic_type_candidates(
            branches
                .iter()
                .map(|branch| substitute_semantic_type_params(branch, substitutions))
                .collect(),
        ),
        SemanticType::Annotated { value, metadata } => SemanticType::Annotated {
            value: Box::new(substitute_semantic_type_params(value, substitutions)),
            metadata: metadata.clone(),
        },
        SemanticType::Unpack(inner) => {
            SemanticType::Unpack(Box::new(substitute_semantic_type_params(inner, substitutions)))
        }
    }
}

pub(crate) fn substitute_semantic_callable_param_expr(
    params: &SemanticCallableParams,
    substitutions: &GenericTypeParamSubstitutions,
) -> SemanticCallableParams {
    match params {
        SemanticCallableParams::Ellipsis => SemanticCallableParams::Ellipsis,
        SemanticCallableParams::ParamList(types) => SemanticCallableParams::ParamList(
            expand_substituted_semantic_generic_args(types, substitutions),
        ),
        SemanticCallableParams::Concatenate(types) => {
            if let Some((tail, prefixes)) = types.split_last()
                && let SemanticType::Name(name) = tail
                && let Some(binding) = substitutions.param_lists.get(name.trim())
            {
                let mut rendered = prefixes
                    .iter()
                    .map(|part| substitute_semantic_type_params(part, substitutions))
                    .collect::<Vec<_>>();
                rendered.extend(binding.params.iter().map(param_annotation_semantic_type));
                SemanticCallableParams::ParamList(rendered)
            } else {
                SemanticCallableParams::Concatenate(
                    types
                        .iter()
                        .map(|part| substitute_semantic_type_params(part, substitutions))
                        .collect(),
                )
            }
        }
        SemanticCallableParams::Single(expr) => {
            if let SemanticType::Name(name) = expr.as_ref()
                && let Some(binding) = substitutions.param_lists.get(name.trim())
            {
                return SemanticCallableParams::ParamList(
                    binding.params.iter().map(param_annotation_semantic_type).collect(),
                );
            }
            SemanticCallableParams::Single(Box::new(substitute_semantic_type_params(
                expr,
                substitutions,
            )))
        }
    }
}

pub(crate) fn param_annotation_semantic_type(
    param: &typepython_syntax::DirectFunctionParamSite,
) -> SemanticType {
    param
        .annotation
        .as_deref()
        .map(|annotation| {
            lower_param_annotation_text(annotation, param.variadic, param.keyword_variadic)
        })
        .unwrap_or_else(|| SemanticType::Name(String::from("dynamic")))
}

pub(crate) fn expand_substituted_semantic_generic_args(
    args: &[SemanticType],
    substitutions: &GenericTypeParamSubstitutions,
) -> Vec<SemanticType> {
    let mut rendered = Vec::new();
    for arg in args {
        if let Some(inner) = arg.unpacked_inner() {
            if let SemanticType::Name(name) = inner
                && let Some(binding) = substitutions.type_packs.get(name.trim())
            {
                rendered.extend(binding.types.iter().cloned());
                continue;
            }
            if let Some(elements) = unpacked_fixed_tuple_semantic_elements(inner) {
                rendered.extend(elements);
                continue;
            }
        }
        rendered.push(substitute_semantic_type_params(arg, substitutions));
    }
    rendered
}

pub(crate) fn unpacked_fixed_tuple_elements(text: &str) -> Option<Vec<String>> {
    unpacked_fixed_tuple_semantic_elements(&lower_type_text_or_name(text)).map(|elements| {
        elements.into_iter().map(|element| render_semantic_type(&element)).collect()
    })
}

pub(crate) fn insert_type_pack_binding(
    substitutions: &mut GenericTypeParamSubstitutions,
    name: &str,
    binding: TypePackBinding,
) -> Option<()> {
    match substitutions.type_packs.get(name) {
        Some(existing) if existing == &binding => Some(()),
        Some(existing) => {
            let merged = merge_type_pack_candidates(existing, &binding)?;
            substitutions.type_packs.insert(name.to_owned(), merged);
            Some(())
        }
        None => {
            substitutions.type_packs.insert(name.to_owned(), binding);
            Some(())
        }
    }
}

pub(crate) fn merge_type_pack_candidates(
    existing: &TypePackBinding,
    actual: &TypePackBinding,
) -> Option<TypePackBinding> {
    if existing.types.len() != actual.types.len() {
        return None;
    }
    Some(TypePackBinding {
        types: existing
            .types
            .iter()
            .zip(&actual.types)
            .map(|(left, right)| merge_generic_type_candidates(left, right))
            .collect(),
    })
}

pub(crate) fn type_pack_name_from_unpack_semantic_annotation(
    annotation: &SemanticType,
    type_pack_names: &BTreeSet<String>,
) -> Option<String> {
    let inner = annotation.unpacked_inner()?;
    let SemanticType::Name(inner) = inner else {
        return None;
    };
    let inner = inner.trim();
    type_pack_names.contains(inner).then(|| inner.to_owned())
}

pub(crate) fn type_pack_binding_from_default(default: &str) -> Option<TypePackBinding> {
    let normalized = normalize_type_text(default);
    if normalized == "tuple[()]" {
        return Some(TypePackBinding::default());
    }
    if let Some(elements) = unpacked_fixed_tuple_elements(&normalized) {
        return Some(TypePackBinding {
            types: elements.into_iter().map(|element| lower_type_text_or_name(&element)).collect(),
        });
    }
    None
}

pub(crate) fn generic_binding_count(solution: &GenericTypeParamSubstitutions) -> usize {
    solution.types.len() + solution.param_lists.len() + solution.type_packs.len()
}

pub(crate) fn infer_generic_type_arg_bindings(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    expected_args: &[SemanticType],
    actual_args: &[SemanticType],
    generic_names: &BTreeSet<String>,
    substitutions: &GenericTypeParamSubstitutions,
    type_pack_names: &BTreeSet<String>,
) -> Option<GenericTypeParamSubstitutions> {
    let expected_args = expand_inferred_generic_args(expected_args, type_pack_names);
    let actual_args = expand_inferred_generic_args(actual_args, type_pack_names);
    let mut inferred = GenericTypeParamSubstitutions::default();

    if let Some((pack_index, pack_name)) = expected_type_pack_index(&expected_args, type_pack_names)
    {
        let suffix_len = expected_args.len().saturating_sub(pack_index + 1);
        if actual_args.len() < pack_index + suffix_len {
            return None;
        }
        for (expected_arg, actual_arg) in
            expected_args[..pack_index].iter().zip(actual_args[..pack_index].iter())
        {
            merge_nested_generic_bindings(
                &mut inferred,
                infer_generic_type_param_bindings_full(
                    node,
                    nodes,
                    expected_arg,
                    actual_arg,
                    generic_names,
                    substitutions,
                    type_pack_names,
                )?,
            )?;
        }
        let actual_pack_end = actual_args.len() - suffix_len;
        insert_type_pack_binding(
            &mut inferred,
            &pack_name,
            TypePackBinding { types: actual_args[pack_index..actual_pack_end].to_vec() },
        )?;
        for (expected_arg, actual_arg) in
            expected_args[pack_index + 1..].iter().zip(actual_args[actual_pack_end..].iter())
        {
            merge_nested_generic_bindings(
                &mut inferred,
                infer_generic_type_param_bindings_full(
                    node,
                    nodes,
                    expected_arg,
                    actual_arg,
                    generic_names,
                    substitutions,
                    type_pack_names,
                )?,
            )?;
        }
        return Some(inferred);
    }

    if expected_args.len() != actual_args.len() {
        return None;
    }
    for (expected_arg, actual_arg) in expected_args.iter().zip(actual_args.iter()) {
        merge_nested_generic_bindings(
            &mut inferred,
            infer_generic_type_param_bindings_full(
                node,
                nodes,
                expected_arg,
                actual_arg,
                generic_names,
                substitutions,
                type_pack_names,
            )?,
        )?;
    }
    Some(inferred)
}

pub(crate) fn merge_nested_generic_bindings(
    inferred: &mut GenericTypeParamSubstitutions,
    nested: GenericTypeParamSubstitutions,
) -> Option<()> {
    for (name, actual_type) in nested.types {
        match inferred.types.get(&name) {
            Some(existing) if existing != &actual_type => {
                let merged = merge_generic_type_candidates(existing, &actual_type);
                inferred.types.insert(name, merged);
            }
            Some(_) => {}
            None => {
                inferred.types.insert(name, actual_type);
            }
        }
    }
    for (name, binding) in nested.type_packs {
        insert_type_pack_binding(inferred, &name, binding)?;
    }
    Some(())
}

pub(crate) fn expand_inferred_generic_args(
    args: &[SemanticType],
    type_pack_names: &BTreeSet<String>,
) -> Vec<SemanticType> {
    let mut expanded = Vec::new();
    for arg in args {
        if let Some(inner) = arg.unpacked_inner() {
            if !matches!(inner, SemanticType::Name(name) if type_pack_names.contains(name.trim()))
                && let Some(elements) = unpacked_fixed_tuple_semantic_elements(inner)
            {
                expanded.extend(elements);
                continue;
            }
        }
        expanded.push(arg.clone());
    }
    expanded
}

pub(crate) fn expected_type_pack_index(
    args: &[SemanticType],
    type_pack_names: &BTreeSet<String>,
) -> Option<(usize, String)> {
    let matches = args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| match arg.unpacked_inner() {
            Some(SemanticType::Name(name)) if type_pack_names.contains(name.trim()) => {
                Some((index, name.trim().to_owned()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [(index, name)] => Some((*index, name.clone())),
        _ => None,
    }
}
