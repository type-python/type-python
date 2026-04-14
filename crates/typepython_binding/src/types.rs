use super::*;

/// Bound representation of one module's symbol and flow surface.
#[derive(Debug, Clone)]
pub struct BindingTable {
    pub module_path: PathBuf,
    pub module_key: String,
    pub module_kind: SourceKind,
    pub surface_facts: ModuleSurfaceFacts,
    pub declarations: Vec<Declaration>,
    pub calls: Vec<CallSite>,
    pub method_calls: Vec<MethodCallSite>,
    pub member_accesses: Vec<MemberAccessSite>,
    pub returns: Vec<ReturnSite>,
    pub yields: Vec<YieldSite>,
    pub if_guards: Vec<IfGuardSite>,
    pub asserts: Vec<AssertGuardSite>,
    pub invalidations: Vec<InvalidationSite>,
    pub matches: Vec<MatchSite>,
    pub for_loops: Vec<ForSite>,
    pub with_statements: Vec<WithSite>,
    pub except_handlers: Vec<ExceptHandlerSite>,
    pub assignments: Vec<AssignmentSite>,
}

impl Default for BindingTable {
    fn default() -> Self {
        Self {
            module_path: PathBuf::new(),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            surface_facts: ModuleSurfaceFacts::default(),
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
        }
    }
}

/// Source-derived module facts reused by later phases without reopening files.
#[derive(Debug, Clone, Default)]
pub struct ModuleSurfaceFacts {
    pub typed_dict_class_metadata: BTreeMap<String, typepython_syntax::TypedDictClassMetadata>,
    pub direct_function_signatures:
        BTreeMap<String, Vec<typepython_syntax::DirectFunctionParamSite>>,
    pub direct_method_signatures:
        BTreeMap<(String, String), Vec<typepython_syntax::DirectFunctionParamSite>>,
    pub decorator_transform_module_info: typepython_syntax::DecoratorTransformModuleInfo,
    pub dataclass_transform_module_info: typepython_syntax::DataclassTransformModuleInfo,
}

/// Wrapper around a parsed type expression captured from bound source text.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BoundTypeExpr {
    pub expr: typepython_syntax::TypeExpr,
}

impl BoundTypeExpr {
    /// Parses text into a bound type expression, falling back to a name node when parsing fails.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            expr: typepython_syntax::TypeExpr::parse(&text)
                .unwrap_or(typepython_syntax::TypeExpr::Name(text)),
        }
    }

    /// Wraps an already parsed type expression.
    #[must_use]
    pub fn from_expr(expr: typepython_syntax::TypeExpr) -> Self {
        Self { expr }
    }

    /// Renders the bound expression back to canonical text form.
    #[must_use]
    pub fn render(&self) -> String {
        self.expr.render()
    }
}

/// Resolved symbol portion of an import target such as `pkg.mod.Name`.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BoundImportSymbolTarget {
    pub module_key: String,
    pub symbol_name: String,
}

/// Structured import target extracted from a declaration.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BoundImportTarget {
    pub raw_target: String,
    pub module_target: String,
    pub symbol_target: Option<BoundImportSymbolTarget>,
}

impl BoundImportTarget {
    /// Builds a structured import target from the source-authored raw import string.
    #[must_use]
    pub fn new(raw_target: impl Into<String>) -> Self {
        let raw_target = raw_target.into();
        let symbol_target =
            raw_target.rsplit_once('.').map(|(module_key, symbol_name)| BoundImportSymbolTarget {
                module_key: module_key.to_owned(),
                symbol_name: symbol_name.to_owned(),
            });
        Self { raw_target: raw_target.clone(), module_target: raw_target, symbol_target }
    }
}

/// Bound callable signature used by later graph and checker phases.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BoundCallableSignature {
    pub params: Vec<typepython_syntax::DirectFunctionParamSite>,
    pub returns: Option<BoundTypeExpr>,
}

impl BoundCallableSignature {
    #[must_use]
    pub fn from_function_parts(
        params: &[typepython_syntax::FunctionParam],
        returns: Option<&str>,
    ) -> Self {
        Self::from_function_parts_with_expr(params, returns, None)
    }

    #[must_use]
    pub fn from_function_parts_with_expr(
        params: &[typepython_syntax::FunctionParam],
        returns: Option<&str>,
        returns_expr: Option<&typepython_syntax::TypeExpr>,
    ) -> Self {
        Self {
            params: params.iter().map(direct_param_site_from_function_param).collect(),
            returns: returns_expr.cloned().map(BoundTypeExpr::from_expr).or_else(|| {
                returns.map(str::trim).filter(|returns| !returns.is_empty()).map(BoundTypeExpr::new)
            }),
        }
    }

    #[must_use]
    pub fn rendered(&self) -> String {
        render_signature_from_direct_params(
            &self.params,
            self.returns.as_ref().map(|expr| expr.render()).as_deref(),
        )
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Default)]
pub enum DeclarationMetadata {
    #[default]
    None,
    TypeAlias {
        value: BoundTypeExpr,
    },
    Callable {
        signature: BoundCallableSignature,
    },
    Import {
        target: BoundImportTarget,
    },
    Value {
        annotation: Option<BoundTypeExpr>,
    },
    Class {
        bases: Vec<String>,
    },
}

/// Direct function call captured from a bound module.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CallSite {
    pub callee: String,
    pub arg_count: usize,
    pub arg_types: Vec<String>,
    pub arg_values: Vec<typepython_syntax::DirectExprMetadata>,
    pub starred_arg_types: Vec<String>,
    pub starred_arg_values: Vec<typepython_syntax::DirectExprMetadata>,
    pub keyword_names: Vec<String>,
    pub keyword_arg_types: Vec<String>,
    pub keyword_arg_values: Vec<typepython_syntax::DirectExprMetadata>,
    pub keyword_expansion_types: Vec<String>,
    pub keyword_expansion_values: Vec<typepython_syntax::DirectExprMetadata>,
    pub line: usize,
}

impl CallSite {
    #[must_use]
    pub fn positional_arg_type_texts(&self) -> Vec<String> {
        rendered_direct_expr_type_texts(&self.arg_values, &self.arg_types)
    }

    #[must_use]
    pub fn starred_arg_type_texts(&self) -> Vec<String> {
        rendered_direct_expr_type_texts(&self.starred_arg_values, &self.starred_arg_types)
    }

    #[must_use]
    pub fn keyword_arg_type_texts(&self) -> Vec<String> {
        rendered_direct_expr_type_texts(&self.keyword_arg_values, &self.keyword_arg_types)
    }

    #[must_use]
    pub fn keyword_expansion_type_texts(&self) -> Vec<String> {
        rendered_direct_expr_type_texts(
            &self.keyword_expansion_values,
            &self.keyword_expansion_types,
        )
    }
}

/// Direct member access captured from a bound module.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MemberAccessSite {
    pub current_owner_name: Option<String>,
    pub current_owner_type_name: Option<String>,
    pub owner_name: String,
    pub member: String,
    pub through_instance: bool,
    pub line: usize,
}

/// Method call with a resolved receiver name and argument metadata.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MethodCallSite {
    pub current_owner_name: Option<String>,
    pub current_owner_type_name: Option<String>,
    pub owner_name: String,
    pub method: String,
    pub through_instance: bool,
    pub arg_count: usize,
    pub arg_types: Vec<String>,
    pub arg_values: Vec<typepython_syntax::DirectExprMetadata>,
    pub starred_arg_types: Vec<String>,
    pub starred_arg_values: Vec<typepython_syntax::DirectExprMetadata>,
    pub keyword_names: Vec<String>,
    pub keyword_arg_types: Vec<String>,
    pub keyword_arg_values: Vec<typepython_syntax::DirectExprMetadata>,
    pub keyword_expansion_types: Vec<String>,
    pub keyword_expansion_values: Vec<typepython_syntax::DirectExprMetadata>,
    pub line: usize,
}

impl MethodCallSite {
    #[must_use]
    pub fn positional_arg_type_texts(&self) -> Vec<String> {
        rendered_direct_expr_type_texts(&self.arg_values, &self.arg_types)
    }

    #[must_use]
    pub fn starred_arg_type_texts(&self) -> Vec<String> {
        rendered_direct_expr_type_texts(&self.starred_arg_values, &self.starred_arg_types)
    }

    #[must_use]
    pub fn keyword_arg_type_texts(&self) -> Vec<String> {
        rendered_direct_expr_type_texts(&self.keyword_arg_values, &self.keyword_arg_types)
    }

    #[must_use]
    pub fn keyword_expansion_type_texts(&self) -> Vec<String> {
        rendered_direct_expr_type_texts(
            &self.keyword_expansion_values,
            &self.keyword_expansion_types,
        )
    }
}

fn rendered_direct_expr_type_texts(
    metadata: &[typepython_syntax::DirectExprMetadata],
    fallback: &[String],
) -> Vec<String> {
    if metadata.is_empty() {
        return fallback.to_vec();
    }
    metadata
        .iter()
        .enumerate()
        .map(|(index, metadata)| {
            metadata
                .rendered_value_type()
                .or_else(|| fallback.get(index).cloned())
                .unwrap_or_default()
        })
        .collect()
}

/// Return expression metadata extracted from a function body.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ReturnSite {
    pub owner_name: String,
    pub owner_type_name: Option<String>,
    pub value: Option<typepython_syntax::DirectExprMetadata>,
    pub value_type: Option<String>,
    pub is_awaited: bool,
    pub value_callee: Option<String>,
    pub value_name: Option<String>,
    pub value_member_owner_name: Option<String>,
    pub value_member_name: Option<String>,
    pub value_member_through_instance: bool,
    pub value_method_owner_name: Option<String>,
    pub value_method_name: Option<String>,
    pub value_method_through_instance: bool,
    pub value_subscript_target: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_subscript_string_key: Option<String>,
    pub value_subscript_index: Option<String>,
    pub value_if_true: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_if_false: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_if_guard: Option<GuardConditionSite>,
    pub value_bool_left: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_bool_right: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_left: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_right: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_operator: Option<String>,
    pub value_lambda: Option<Box<typepython_syntax::LambdaMetadata>>,
    pub value_list_elements: Option<Vec<typepython_syntax::DirectExprMetadata>>,
    pub value_set_elements: Option<Vec<typepython_syntax::DirectExprMetadata>>,
    pub value_dict_entries: Option<Vec<typepython_syntax::TypedDictLiteralEntry>>,
    pub line: usize,
}

/// Yield or `yield from` expression metadata extracted from a function body.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct YieldSite {
    pub owner_name: String,
    pub owner_type_name: Option<String>,
    pub value: Option<typepython_syntax::DirectExprMetadata>,
    pub value_type: Option<String>,
    pub value_callee: Option<String>,
    pub value_name: Option<String>,
    pub value_member_owner_name: Option<String>,
    pub value_member_name: Option<String>,
    pub value_member_through_instance: bool,
    pub value_method_owner_name: Option<String>,
    pub value_method_name: Option<String>,
    pub value_method_through_instance: bool,
    pub value_subscript_target: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_subscript_string_key: Option<String>,
    pub value_subscript_index: Option<String>,
    pub value_if_true: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_if_false: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_if_guard: Option<GuardConditionSite>,
    pub value_bool_left: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_bool_right: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_left: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_right: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_operator: Option<String>,
    pub value_lambda: Option<Box<typepython_syntax::LambdaMetadata>>,
    pub value_list_elements: Option<Vec<typepython_syntax::DirectExprMetadata>>,
    pub value_set_elements: Option<Vec<typepython_syntax::DirectExprMetadata>>,
    pub value_dict_entries: Option<Vec<typepython_syntax::TypedDictLiteralEntry>>,
    pub is_yield_from: bool,
    pub line: usize,
}

/// Control-flow guard range introduced by an `if` statement.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct IfGuardSite {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub guard: Option<GuardConditionSite>,
    pub line: usize,
    pub true_start_line: usize,
    pub true_end_line: usize,
    pub false_start_line: Option<usize>,
    pub false_end_line: Option<usize>,
}

/// Guard introduced by an `assert` statement.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AssertGuardSite {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub guard: Option<GuardConditionSite>,
    pub line: usize,
}

/// Reason why previously known name information must be invalidated.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum InvalidationKind {
    RebindLike,
    Delete,
    ScopeChange,
}

/// Explicit invalidation of one or more tracked names.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct InvalidationSite {
    pub kind: InvalidationKind,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub names: Vec<String>,
    pub line: usize,
}

/// Bound guard condition used for narrowing and flow-sensitive checks.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum GuardConditionSite {
    IsNone { name: String, negated: bool },
    IsInstance { name: String, types: Vec<String> },
    PredicateCall { name: String, callee: String },
    TruthyName { name: String },
    Not(Box<GuardConditionSite>),
    And(Vec<GuardConditionSite>),
    Or(Vec<GuardConditionSite>),
}

/// Match statement subject and case metadata.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MatchSite {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub subject: Option<typepython_syntax::DirectExprMetadata>,
    pub subject_type: Option<String>,
    pub subject_is_awaited: bool,
    pub subject_callee: Option<String>,
    pub subject_name: Option<String>,
    pub subject_member_owner_name: Option<String>,
    pub subject_member_name: Option<String>,
    pub subject_member_through_instance: bool,
    pub subject_method_owner_name: Option<String>,
    pub subject_method_name: Option<String>,
    pub subject_method_through_instance: bool,
    pub cases: Vec<MatchCaseSite>,
    pub line: usize,
}

/// One `case` arm inside a bound match statement.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MatchCaseSite {
    pub patterns: Vec<MatchPatternSite>,
    pub has_guard: bool,
    pub line: usize,
}

/// Pattern category extracted from a `match` case.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum MatchPatternSite {
    Wildcard,
    Literal(String),
    Class(String),
    Unsupported,
}

/// Bound `for` loop target and iterable metadata.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ForSite {
    pub target_name: String,
    pub target_names: Vec<String>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub iter: Option<typepython_syntax::DirectExprMetadata>,
    pub iter_type: Option<String>,
    pub iter_is_awaited: bool,
    pub iter_callee: Option<String>,
    pub iter_name: Option<String>,
    pub iter_member_owner_name: Option<String>,
    pub iter_member_name: Option<String>,
    pub iter_member_through_instance: bool,
    pub iter_method_owner_name: Option<String>,
    pub iter_method_name: Option<String>,
    pub iter_method_through_instance: bool,
    pub line: usize,
}

/// Bound `with` statement context-manager metadata.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WithSite {
    pub target_name: Option<String>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub context: Option<typepython_syntax::DirectExprMetadata>,
    pub context_type: Option<String>,
    pub context_is_awaited: bool,
    pub context_callee: Option<String>,
    pub context_name: Option<String>,
    pub context_member_owner_name: Option<String>,
    pub context_member_name: Option<String>,
    pub context_member_through_instance: bool,
    pub context_method_owner_name: Option<String>,
    pub context_method_name: Option<String>,
    pub context_method_through_instance: bool,
    pub line: usize,
}

/// Exception handler binding and covered source range.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ExceptHandlerSite {
    pub exception_type: String,
    pub binding_name: Option<String>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
    pub end_line: usize,
}

/// Assignment target annotated with the expression shape on the right-hand side.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AssignmentSite {
    pub name: String,
    pub destructuring_target_names: Option<Vec<String>>,
    pub destructuring_index: Option<usize>,
    pub annotation: Option<String>,
    pub annotation_expr: Option<BoundTypeExpr>,
    pub value: Option<typepython_syntax::DirectExprMetadata>,
    pub value_type: Option<String>,
    pub is_awaited: bool,
    pub value_callee: Option<String>,
    pub value_name: Option<String>,
    pub value_member_owner_name: Option<String>,
    pub value_member_name: Option<String>,
    pub value_member_through_instance: bool,
    pub value_method_owner_name: Option<String>,
    pub value_method_name: Option<String>,
    pub value_method_through_instance: bool,
    pub value_subscript_target: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_subscript_string_key: Option<String>,
    pub value_subscript_index: Option<String>,
    pub value_if_true: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_if_false: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_if_guard: Option<GuardConditionSite>,
    pub value_bool_left: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_bool_right: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_left: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_right: Option<Box<typepython_syntax::DirectExprMetadata>>,
    pub value_binop_operator: Option<String>,
    pub value_lambda: Option<Box<typepython_syntax::LambdaMetadata>>,
    pub value_list_comprehension: Option<Box<typepython_syntax::ComprehensionMetadata>>,
    pub value_generator_comprehension: Option<Box<typepython_syntax::ComprehensionMetadata>>,
    pub value_list_elements: Option<Vec<typepython_syntax::DirectExprMetadata>>,
    pub value_set_elements: Option<Vec<typepython_syntax::DirectExprMetadata>>,
    pub value_dict_entries: Option<Vec<typepython_syntax::TypedDictLiteralEntry>>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
}

impl ReturnSite {
    #[must_use]
    pub fn value_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.value.clone().or_else(|| {
            direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
                value_type_expr: None,
                value_type: self.value_type.clone(),
                is_awaited: self.is_awaited,
                value_callee: self.value_callee.clone(),
                value_name: self.value_name.clone(),
                value_member_owner_name: self.value_member_owner_name.clone(),
                value_member_name: self.value_member_name.clone(),
                value_member_through_instance: self.value_member_through_instance,
                value_method_owner_name: self.value_method_owner_name.clone(),
                value_method_name: self.value_method_name.clone(),
                value_method_through_instance: self.value_method_through_instance,
                value_subscript_target: self.value_subscript_target.clone(),
                value_subscript_string_key: self.value_subscript_string_key.clone(),
                value_subscript_index: self.value_subscript_index.clone(),
                value_if_true: self.value_if_true.clone(),
                value_if_false: self.value_if_false.clone(),
                value_if_guard: self.value_if_guard.as_ref().map(guard_condition_site_to_guard),
                value_bool_left: self.value_bool_left.clone(),
                value_bool_right: self.value_bool_right.clone(),
                value_binop_left: self.value_binop_left.clone(),
                value_binop_right: self.value_binop_right.clone(),
                value_binop_operator: self.value_binop_operator.clone(),
                value_lambda: self.value_lambda.clone(),
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: self.value_list_elements.clone(),
                value_set_elements: self.value_set_elements.clone(),
                value_dict_entries: self.value_dict_entries.clone(),
            })
        })
    }
}

impl YieldSite {
    #[must_use]
    pub fn value_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.value.clone().or_else(|| {
            direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
                value_type_expr: None,
                value_type: self.value_type.clone(),
                is_awaited: false,
                value_callee: self.value_callee.clone(),
                value_name: self.value_name.clone(),
                value_member_owner_name: self.value_member_owner_name.clone(),
                value_member_name: self.value_member_name.clone(),
                value_member_through_instance: self.value_member_through_instance,
                value_method_owner_name: self.value_method_owner_name.clone(),
                value_method_name: self.value_method_name.clone(),
                value_method_through_instance: self.value_method_through_instance,
                value_subscript_target: self.value_subscript_target.clone(),
                value_subscript_string_key: self.value_subscript_string_key.clone(),
                value_subscript_index: self.value_subscript_index.clone(),
                value_if_true: self.value_if_true.clone(),
                value_if_false: self.value_if_false.clone(),
                value_if_guard: self.value_if_guard.as_ref().map(guard_condition_site_to_guard),
                value_bool_left: self.value_bool_left.clone(),
                value_bool_right: self.value_bool_right.clone(),
                value_binop_left: self.value_binop_left.clone(),
                value_binop_right: self.value_binop_right.clone(),
                value_binop_operator: self.value_binop_operator.clone(),
                value_lambda: self.value_lambda.clone(),
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: self.value_list_elements.clone(),
                value_set_elements: self.value_set_elements.clone(),
                value_dict_entries: self.value_dict_entries.clone(),
            })
        })
    }
}

impl MatchSite {
    #[must_use]
    pub fn subject_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.subject.clone().or_else(|| {
            direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
                value_type_expr: None,
                value_type: self.subject_type.clone(),
                is_awaited: self.subject_is_awaited,
                value_callee: self.subject_callee.clone(),
                value_name: self.subject_name.clone(),
                value_member_owner_name: self.subject_member_owner_name.clone(),
                value_member_name: self.subject_member_name.clone(),
                value_member_through_instance: self.subject_member_through_instance,
                value_method_owner_name: self.subject_method_owner_name.clone(),
                value_method_name: self.subject_method_name.clone(),
                value_method_through_instance: self.subject_method_through_instance,
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
            })
        })
    }
}

impl ForSite {
    #[must_use]
    pub fn iter_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.iter.clone().or_else(|| {
            direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
                value_type_expr: None,
                value_type: self.iter_type.clone(),
                is_awaited: self.iter_is_awaited,
                value_callee: self.iter_callee.clone(),
                value_name: self.iter_name.clone(),
                value_member_owner_name: self.iter_member_owner_name.clone(),
                value_member_name: self.iter_member_name.clone(),
                value_member_through_instance: self.iter_member_through_instance,
                value_method_owner_name: self.iter_method_owner_name.clone(),
                value_method_name: self.iter_method_name.clone(),
                value_method_through_instance: self.iter_method_through_instance,
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
            })
        })
    }
}

impl WithSite {
    #[must_use]
    pub fn context_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.context.clone().or_else(|| {
            direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
                value_type_expr: None,
                value_type: self.context_type.clone(),
                is_awaited: self.context_is_awaited,
                value_callee: self.context_callee.clone(),
                value_name: self.context_name.clone(),
                value_member_owner_name: self.context_member_owner_name.clone(),
                value_member_name: self.context_member_name.clone(),
                value_member_through_instance: self.context_member_through_instance,
                value_method_owner_name: self.context_method_owner_name.clone(),
                value_method_name: self.context_method_name.clone(),
                value_method_through_instance: self.context_method_through_instance,
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
            })
        })
    }
}

impl AssignmentSite {
    #[must_use]
    pub fn annotation_text(&self) -> Option<&str> {
        self.annotation.as_deref()
    }

    #[must_use]
    pub fn value_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.value.clone().or_else(|| {
            direct_expr_metadata_from_parts(typepython_syntax::DirectExprMetadata {
                value_type_expr: None,
                value_type: self.value_type.clone(),
                is_awaited: self.is_awaited,
                value_callee: self.value_callee.clone(),
                value_name: self.value_name.clone(),
                value_member_owner_name: self.value_member_owner_name.clone(),
                value_member_name: self.value_member_name.clone(),
                value_member_through_instance: self.value_member_through_instance,
                value_method_owner_name: self.value_method_owner_name.clone(),
                value_method_name: self.value_method_name.clone(),
                value_method_through_instance: self.value_method_through_instance,
                value_subscript_target: self.value_subscript_target.clone(),
                value_subscript_string_key: self.value_subscript_string_key.clone(),
                value_subscript_index: self.value_subscript_index.clone(),
                value_if_true: self.value_if_true.clone(),
                value_if_false: self.value_if_false.clone(),
                value_if_guard: self.value_if_guard.as_ref().map(guard_condition_site_to_guard),
                value_bool_left: self.value_bool_left.clone(),
                value_bool_right: self.value_bool_right.clone(),
                value_binop_left: self.value_binop_left.clone(),
                value_binop_right: self.value_binop_right.clone(),
                value_binop_operator: self.value_binop_operator.clone(),
                value_lambda: self.value_lambda.clone(),
                value_list_comprehension: self.value_list_comprehension.clone(),
                value_generator_comprehension: self.value_generator_comprehension.clone(),
                value_list_elements: self.value_list_elements.clone(),
                value_set_elements: self.value_set_elements.clone(),
                value_dict_entries: self.value_dict_entries.clone(),
            })
        })
    }
}

/// Bound declaration exported by a module or owned by a type block.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Declaration {
    pub name: String,
    pub kind: DeclarationKind,
    pub detail: String,
    pub metadata: DeclarationMetadata,
    pub value_type: Option<String>,
    pub method_kind: Option<MethodKind>,
    pub class_kind: Option<DeclarationOwnerKind>,
    pub owner: Option<DeclarationOwner>,
    pub is_async: bool,
    pub is_override: bool,
    pub is_abstract_method: bool,
    pub is_final_decorator: bool,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub is_final: bool,
    pub is_class_var: bool,
    pub bases: Vec<String>,
    pub type_params: Vec<GenericTypeParam>,
}

impl Declaration {
    #[must_use]
    pub fn rendered_detail(&self) -> String {
        match &self.metadata {
            DeclarationMetadata::None => self.detail.clone(),
            DeclarationMetadata::TypeAlias { value } => value.render(),
            DeclarationMetadata::Callable { signature } => signature.rendered(),
            DeclarationMetadata::Import { target } => target.raw_target.clone(),
            DeclarationMetadata::Value { annotation } => annotation
                .as_ref()
                .map(BoundTypeExpr::render)
                .unwrap_or_else(|| self.detail.clone()),
            DeclarationMetadata::Class { bases } => bases.join(","),
        }
    }

    #[must_use]
    pub fn callable_signature(&self) -> Option<&BoundCallableSignature> {
        match &self.metadata {
            DeclarationMetadata::Callable { signature } => Some(signature),
            _ => None,
        }
    }

    #[must_use]
    pub fn type_alias_value(&self) -> Option<&BoundTypeExpr> {
        match &self.metadata {
            DeclarationMetadata::TypeAlias { value } => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn import_target(&self) -> Option<&BoundImportTarget> {
        match &self.metadata {
            DeclarationMetadata::Import { target } => Some(target),
            _ => None,
        }
    }

    #[must_use]
    pub fn value_annotation(&self) -> Option<&BoundTypeExpr> {
        match &self.metadata {
            DeclarationMetadata::Value { annotation } => annotation.as_ref(),
            _ => None,
        }
    }

    #[must_use]
    pub fn class_bases(&self) -> Option<&[String]> {
        match &self.metadata {
            DeclarationMetadata::Class { bases } => Some(bases),
            _ => None,
        }
    }
}

/// Generic parameter category preserved by binding.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum GenericTypeParamKind {
    TypeVar,
    ParamSpec,
    TypeVarTuple,
}

/// Bound generic parameter metadata for declarations and aliases.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct GenericTypeParam {
    pub kind: GenericTypeParamKind,
    pub name: String,
    pub bound: Option<String>,
    pub bound_expr: Option<BoundTypeExpr>,
    pub constraints: Vec<String>,
    pub constraint_exprs: Vec<BoundTypeExpr>,
    pub default: Option<String>,
    pub default_expr: Option<BoundTypeExpr>,
}

impl GenericTypeParam {
    #[must_use]
    pub fn rendered_bound(&self) -> Option<String> {
        self.bound_expr.as_ref().map(BoundTypeExpr::render).or_else(|| self.bound.clone())
    }

    #[must_use]
    pub fn rendered_constraints(&self) -> Vec<String> {
        if !self.constraint_exprs.is_empty() {
            self.constraint_exprs.iter().map(BoundTypeExpr::render).collect()
        } else {
            self.constraints.clone()
        }
    }

    #[must_use]
    pub fn rendered_default(&self) -> Option<String> {
        self.default_expr.as_ref().map(BoundTypeExpr::render).or_else(|| self.default.clone())
    }
}

/// Owning type declaration for a bound member.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DeclarationOwner {
    pub name: String,
    pub kind: DeclarationOwnerKind,
}

/// Type block kinds that can own bound members.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum DeclarationOwnerKind {
    Class,
    Interface,
    DataClass,
    SealedClass,
}

/// Top-level declaration categories produced by binding.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum DeclarationKind {
    TypeAlias,
    Class,
    Function,
    Overload,
    Value,
    Import,
}
