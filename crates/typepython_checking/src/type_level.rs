use super::*;

const DEFAULT_TYPE_LEVEL_STEP_BUDGET: usize = 64;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum TypeLevelValue {
    Type(SemanticType),
    Bool(bool),
    KeySet(Vec<String>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum TypeLevelEvalError {
    StepBudgetExceeded,
    UnsupportedForm(String),
    InvalidArity { form: String, expected: usize, actual: usize },
    NonBooleanCondition(String),
    UnknownShape(String),
}

#[derive(Debug, Clone)]
pub(crate) struct TypeLevelEvaluator<'a> {
    context: &'a CheckerContext<'a>,
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    remaining_steps: usize,
}

impl<'a> TypeLevelEvaluator<'a> {
    pub(crate) fn new(
        context: &'a CheckerContext<'a>,
        node: &'a typepython_graph::ModuleNode,
        nodes: &'a [typepython_graph::ModuleNode],
    ) -> Self {
        Self { context, node, nodes, remaining_steps: DEFAULT_TYPE_LEVEL_STEP_BUDGET }
    }

    pub(crate) fn evaluate_type(
        &mut self,
        ty: &SemanticType,
    ) -> Result<TypeLevelValue, TypeLevelEvalError> {
        self.consume_step()?;
        let ty = ty.strip_annotated();
        match ty {
            SemanticType::Generic { head, args } if head == "TypeIf" => self.evaluate_type_if(args),
            SemanticType::Generic { head, args } if head == "IsSubtype" => {
                self.evaluate_is_subtype(args)
            }
            SemanticType::Generic { head, args } if head == "KeyOf" => self.evaluate_key_of(args),
            SemanticType::Generic { head, args } if head == "Pick" => {
                self.evaluate_pick_omit(args, true)
            }
            SemanticType::Generic { head, args } if head == "Omit" => {
                self.evaluate_pick_omit(args, false)
            }
            SemanticType::Generic { head, args } if head == "RequiredKeys" => {
                self.evaluate_key_filter(args, true)
            }
            SemanticType::Generic { head, args } if head == "OptionalKeys" => {
                self.evaluate_key_filter(args, false)
            }
            SemanticType::Generic { head, args } if head == "MapValues" => {
                self.evaluate_map_values(args)
            }
            _ => Ok(TypeLevelValue::Type(ty.clone())),
        }
    }

    fn evaluate_type_if(
        &mut self,
        args: &[SemanticType],
    ) -> Result<TypeLevelValue, TypeLevelEvalError> {
        if args.len() != 3 {
            return Err(TypeLevelEvalError::InvalidArity {
                form: String::from("TypeIf"),
                expected: 3,
                actual: args.len(),
            });
        }
        let condition = self.evaluate_type(&args[0])?;
        let TypeLevelValue::Bool(condition) = condition else {
            return Err(TypeLevelEvalError::NonBooleanCondition(render_semantic_type(&args[0])));
        };
        self.evaluate_type(if condition { &args[1] } else { &args[2] })
    }

    fn evaluate_is_subtype(
        &mut self,
        args: &[SemanticType],
    ) -> Result<TypeLevelValue, TypeLevelEvalError> {
        if args.len() != 2 {
            return Err(TypeLevelEvalError::InvalidArity {
                form: String::from("IsSubtype"),
                expected: 2,
                actual: args.len(),
            });
        }
        let subtype = self.expect_type(&args[0])?;
        let supertype = self.expect_type(&args[1])?;
        Ok(TypeLevelValue::Bool(
            self.context.semantic_type_is_assignable(self.node, &supertype, &subtype),
        ))
    }

    fn evaluate_key_of(
        &mut self,
        args: &[SemanticType],
    ) -> Result<TypeLevelValue, TypeLevelEvalError> {
        if args.len() != 1 {
            return Err(TypeLevelEvalError::InvalidArity {
                form: String::from("KeyOf"),
                expected: 1,
                actual: args.len(),
            });
        }
        let TypeLevelValue::Type(shape_like) = self.evaluate_type(&args[0])? else {
            return Err(TypeLevelEvalError::UnsupportedForm(render_semantic_type(&args[0])));
        };
        let Some(owner) = semantic_nominal_owner_name(&shape_like) else {
            return Err(TypeLevelEvalError::UnknownShape(render_semantic_type(&shape_like)));
        };
        let Some(shape) =
            resolve_known_shape_from_type_with_context(self.context, self.node, self.nodes, &owner)
        else {
            return Err(TypeLevelEvalError::UnknownShape(owner));
        };
        Ok(TypeLevelValue::KeySet(
            shape.fields.into_iter().map(|field| field.public_alias).collect(),
        ))
    }

    fn evaluate_pick_omit(
        &mut self,
        args: &[SemanticType],
        pick: bool,
    ) -> Result<TypeLevelValue, TypeLevelEvalError> {
        if args.len() != 2 {
            return Err(TypeLevelEvalError::InvalidArity {
                form: if pick { String::from("Pick") } else { String::from("Omit") },
                expected: 2,
                actual: args.len(),
            });
        }
        let TypeLevelValue::Type(shape_like) = self.evaluate_type(&args[0])? else {
            return Err(TypeLevelEvalError::UnsupportedForm(render_semantic_type(&args[0])));
        };
        let Some(owner) = semantic_nominal_owner_name(&shape_like) else {
            return Err(TypeLevelEvalError::UnknownShape(render_semantic_type(&shape_like)));
        };
        let Some(shape) =
            resolve_known_shape_from_type_with_context(self.context, self.node, self.nodes, &owner)
        else {
            return Err(TypeLevelEvalError::UnknownShape(owner));
        };
        let keys = key_literals(&args[1])?;
        let key_refs = keys.iter().map(String::as_str).collect::<Vec<_>>();
        let projected = if pick { shape.pick(&key_refs) } else { shape.omit(&key_refs) };
        Ok(TypeLevelValue::KeySet(
            projected.fields.into_iter().map(|field| field.public_alias).collect(),
        ))
    }

    fn evaluate_key_filter(
        &mut self,
        args: &[SemanticType],
        required: bool,
    ) -> Result<TypeLevelValue, TypeLevelEvalError> {
        if args.len() != 1 {
            return Err(TypeLevelEvalError::InvalidArity {
                form: if required {
                    String::from("RequiredKeys")
                } else {
                    String::from("OptionalKeys")
                },
                expected: 1,
                actual: args.len(),
            });
        }
        let TypeLevelValue::Type(shape_like) = self.evaluate_type(&args[0])? else {
            return Err(TypeLevelEvalError::UnsupportedForm(render_semantic_type(&args[0])));
        };
        let Some(owner) = semantic_nominal_owner_name(&shape_like) else {
            return Err(TypeLevelEvalError::UnknownShape(render_semantic_type(&shape_like)));
        };
        let Some(shape) =
            resolve_known_shape_from_type_with_context(self.context, self.node, self.nodes, &owner)
        else {
            return Err(TypeLevelEvalError::UnknownShape(owner));
        };
        Ok(TypeLevelValue::KeySet(
            shape
                .fields
                .into_iter()
                .filter(|field| field.required == required)
                .map(|field| field.public_alias)
                .collect(),
        ))
    }

    fn evaluate_map_values(
        &mut self,
        args: &[SemanticType],
    ) -> Result<TypeLevelValue, TypeLevelEvalError> {
        if args.len() != 2 {
            return Err(TypeLevelEvalError::InvalidArity {
                form: String::from("MapValues"),
                expected: 2,
                actual: args.len(),
            });
        }
        let wrapper = render_semantic_type(&args[1]);
        if !matches!(wrapper.as_str(), "Optional" | "Readonly") {
            return Err(TypeLevelEvalError::UnsupportedForm(format!("MapValues[{wrapper}]")));
        }
        let TypeLevelValue::Type(shape_like) = self.evaluate_type(&args[0])? else {
            return Err(TypeLevelEvalError::UnsupportedForm(render_semantic_type(&args[0])));
        };
        let Some(owner) = semantic_nominal_owner_name(&shape_like) else {
            return Err(TypeLevelEvalError::UnknownShape(render_semantic_type(&shape_like)));
        };
        let Some(shape) =
            resolve_known_shape_from_type_with_context(self.context, self.node, self.nodes, &owner)
        else {
            return Err(TypeLevelEvalError::UnknownShape(owner));
        };
        Ok(TypeLevelValue::KeySet(
            shape.fields.into_iter().map(|field| field.public_alias).collect(),
        ))
    }

    fn expect_type(&mut self, ty: &SemanticType) -> Result<SemanticType, TypeLevelEvalError> {
        match self.evaluate_type(ty)? {
            TypeLevelValue::Type(ty) => Ok(ty),
            other => Err(TypeLevelEvalError::UnsupportedForm(format!("{other:?}"))),
        }
    }

    fn consume_step(&mut self) -> Result<(), TypeLevelEvalError> {
        if self.remaining_steps == 0 {
            return Err(TypeLevelEvalError::StepBudgetExceeded);
        }
        self.remaining_steps -= 1;
        Ok(())
    }
}

fn key_literals(ty: &SemanticType) -> Result<Vec<String>, TypeLevelEvalError> {
    match ty.strip_annotated() {
        SemanticType::Generic { head, args } if head == "Literal" => args
            .iter()
            .map(|arg| {
                let rendered = render_semantic_type(arg);
                rendered
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
                    .map(str::to_owned)
                    .ok_or(TypeLevelEvalError::UnsupportedForm(format!("key literal `{rendered}`")))
            })
            .collect(),
        SemanticType::Union(branches) => {
            branches.iter().try_fold(Vec::new(), |mut keys, branch| {
                keys.extend(key_literals(branch)?);
                Ok(keys)
            })
        }
        other => Err(TypeLevelEvalError::UnsupportedForm(format!(
            "key set `{}`",
            render_semantic_type(other)
        ))),
    }
}

pub(crate) fn evaluate_restricted_type_level_aliases(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<(String, Result<TypeLevelValue, TypeLevelEvalError>)> {
    node.declarations
        .iter()
        .filter(|declaration| declaration.kind == DeclarationKind::TypeAlias)
        .filter_map(|declaration| {
            let alias_value = declaration.type_alias_value()?;
            let semantic = lower_type_expr(alias_value.expr.clone());
            contains_restricted_type_level_form(&semantic).then(|| {
                let mut evaluator = TypeLevelEvaluator::new(context, node, nodes);
                (declaration.name.clone(), evaluator.evaluate_type(&semantic))
            })
        })
        .collect()
}

pub(crate) fn restricted_type_level_alias_diagnostics(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
) -> Vec<Diagnostic> {
    evaluate_restricted_type_level_aliases(context, node, nodes)
        .into_iter()
        .filter_map(|(alias, result)| {
            result.err().map(|error| {
                Diagnostic::error(
                    "TPY4027",
                    format!(
                        "type-level alias `{alias}` could not be evaluated: {}",
                        type_level_error_message(&error),
                    ),
                )
                .with_span(Span::new(node.module_path.display().to_string(), 1, 1, 1, 1))
                .with_note("restricted type-level forms must fully reduce to standard Python typing before emit")
            })
        })
        .collect()
}

fn type_level_error_message(error: &TypeLevelEvalError) -> String {
    match error {
        TypeLevelEvalError::StepBudgetExceeded => String::from("evaluation step budget exceeded"),
        TypeLevelEvalError::UnsupportedForm(form) => {
            format!("unsupported form `{form}` in the current evaluator slice")
        }
        TypeLevelEvalError::InvalidArity { form, expected, actual } => {
            format!("form `{form}` expects {expected} argument(s), but received {actual}")
        }
        TypeLevelEvalError::NonBooleanCondition(condition) => {
            format!("condition `{condition}` did not evaluate to a boolean")
        }
        TypeLevelEvalError::UnknownShape(shape) => {
            format!("shape source `{shape}` is not known to the checker")
        }
    }
}

fn contains_restricted_type_level_form(ty: &SemanticType) -> bool {
    match ty.strip_annotated() {
        SemanticType::Generic { head, args } => {
            matches!(
                head.as_str(),
                "TypeIf"
                    | "IsSubtype"
                    | "KeyOf"
                    | "Pick"
                    | "Omit"
                    | "RequiredKeys"
                    | "OptionalKeys"
                    | "MapValues"
            ) || args.iter().any(contains_restricted_type_level_form)
        }
        SemanticType::Callable { params, return_type } => {
            contains_restricted_callable_param_form(params)
                || contains_restricted_type_level_form(return_type)
        }
        SemanticType::Union(branches) => branches.iter().any(contains_restricted_type_level_form),
        SemanticType::Annotated { value, .. } | SemanticType::Unpack(value) => {
            contains_restricted_type_level_form(value)
        }
        SemanticType::Name(_) => false,
    }
}

fn contains_restricted_callable_param_form(params: &SemanticCallableParams) -> bool {
    match params {
        SemanticCallableParams::Ellipsis => false,
        SemanticCallableParams::ParamList(params) | SemanticCallableParams::Concatenate(params) => {
            params.iter().any(contains_restricted_type_level_form)
        }
        SemanticCallableParams::Single(param) => contains_restricted_type_level_form(param),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn empty_node() -> typepython_graph::ModuleNode {
        typepython_graph::ModuleNode {
            module_path: PathBuf::from("type-level.tpy"),
            module_key: String::from("type_level"),
            module_kind: SourceKind::TypePython,
            declarations: Vec::new(),
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
            summary_fingerprint: 0,
        }
    }

    #[test]
    fn type_if_evaluates_decidable_is_subtype_condition() {
        let node = empty_node();
        let nodes = vec![node.clone()];
        let context = CheckerContext::new(&nodes, ImportFallback::Unknown, None);
        let ty = lower_type_text_or_name("TypeIf[IsSubtype[int, object], str, bytes]");
        let mut evaluator = TypeLevelEvaluator::new(&context, &node, &nodes);

        let result = evaluator.evaluate_type(&ty);

        assert_eq!(result, Ok(TypeLevelValue::Type(SemanticType::Name(String::from("str")))));
    }

    #[test]
    fn type_if_is_subtype_honors_strict_nulls_option() {
        let node = empty_node();
        let nodes = vec![node.clone()];
        let context = CheckerContext::new_with_bound_surface_facts_and_options(
            &nodes,
            None,
            None,
            CheckerOptions { strict_nulls: false, ..CheckerOptions::default() },
        );
        let ty = lower_type_text_or_name("TypeIf[IsSubtype[None, int], str, bytes]");
        let mut evaluator = TypeLevelEvaluator::new(&context, &node, &nodes);

        let result = evaluator.evaluate_type(&ty);

        assert_eq!(result, Ok(TypeLevelValue::Type(SemanticType::Name(String::from("str")))));
    }

    #[test]
    fn type_if_is_subtype_honors_taint_option() {
        let node = empty_node();
        let nodes = vec![node.clone()];
        let context = CheckerContext::new_with_bound_surface_facts_and_options(
            &nodes,
            None,
            None,
            CheckerOptions { experimental_taint: true, ..CheckerOptions::default() },
        );
        let ty =
            lower_type_text_or_name("TypeIf[IsSubtype[Tainted[str, \"html\"], str], bytes, str]");
        let mut evaluator = TypeLevelEvaluator::new(&context, &node, &nodes);

        let result = evaluator.evaluate_type(&ty);

        assert_eq!(result, Ok(TypeLevelValue::Type(SemanticType::Name(String::from("str")))));
    }

    #[test]
    fn type_if_rejects_non_boolean_condition() {
        let node = empty_node();
        let nodes = vec![node.clone()];
        let context = CheckerContext::new(&nodes, ImportFallback::Unknown, None);
        let ty = lower_type_text_or_name("TypeIf[int, str, bytes]");
        let mut evaluator = TypeLevelEvaluator::new(&context, &node, &nodes);

        let result = evaluator.evaluate_type(&ty);

        assert_eq!(result, Err(TypeLevelEvalError::NonBooleanCondition(String::from("int"))));
    }
}
