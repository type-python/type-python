//! Symbol binding boundary for TypePython.

use std::{collections::BTreeMap, path::PathBuf};

use typepython_syntax::{MethodKind, SourceKind, SyntaxStatement, SyntaxTree};

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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BoundTypeExpr {
    pub text: String,
}

impl BoundTypeExpr {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BoundImportSymbolTarget {
    pub module_key: String,
    pub symbol_name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BoundImportTarget {
    pub raw_target: String,
    pub module_target: String,
    pub symbol_target: Option<BoundImportSymbolTarget>,
}

impl BoundImportTarget {
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
        Self {
            params: params.iter().map(direct_param_site_from_function_param).collect(),
            returns: returns
                .map(str::trim)
                .filter(|returns| !returns.is_empty())
                .map(BoundTypeExpr::new),
        }
    }

    #[must_use]
    pub fn rendered(&self) -> String {
        render_signature_from_direct_params(
            &self.params,
            self.returns.as_ref().map(|expr| expr.text.as_str()),
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
            direct_expr_metadata_from_flat_fields(
                self.value_type.clone(),
                self.is_awaited,
                self.value_callee.clone(),
                self.value_name.clone(),
                self.value_member_owner_name.clone(),
                self.value_member_name.clone(),
                self.value_member_through_instance,
                self.value_method_owner_name.clone(),
                self.value_method_name.clone(),
                self.value_method_through_instance,
                self.value_subscript_target.clone(),
                self.value_subscript_string_key.clone(),
                self.value_subscript_index.clone(),
                self.value_if_true.clone(),
                self.value_if_false.clone(),
                self.value_if_guard.as_ref().map(guard_condition_site_to_guard),
                self.value_bool_left.clone(),
                self.value_bool_right.clone(),
                self.value_binop_left.clone(),
                self.value_binop_right.clone(),
                self.value_binop_operator.clone(),
                self.value_lambda.clone(),
                None,
                None,
                self.value_list_elements.clone(),
                self.value_set_elements.clone(),
                self.value_dict_entries.clone(),
            )
        })
    }
}

impl YieldSite {
    #[must_use]
    pub fn value_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.value.clone().or_else(|| {
            direct_expr_metadata_from_flat_fields(
                self.value_type.clone(),
                false,
                self.value_callee.clone(),
                self.value_name.clone(),
                self.value_member_owner_name.clone(),
                self.value_member_name.clone(),
                self.value_member_through_instance,
                self.value_method_owner_name.clone(),
                self.value_method_name.clone(),
                self.value_method_through_instance,
                self.value_subscript_target.clone(),
                self.value_subscript_string_key.clone(),
                self.value_subscript_index.clone(),
                self.value_if_true.clone(),
                self.value_if_false.clone(),
                self.value_if_guard.as_ref().map(guard_condition_site_to_guard),
                self.value_bool_left.clone(),
                self.value_bool_right.clone(),
                self.value_binop_left.clone(),
                self.value_binop_right.clone(),
                self.value_binop_operator.clone(),
                self.value_lambda.clone(),
                None,
                None,
                self.value_list_elements.clone(),
                self.value_set_elements.clone(),
                self.value_dict_entries.clone(),
            )
        })
    }
}

impl MatchSite {
    #[must_use]
    pub fn subject_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.subject.clone().or_else(|| {
            direct_expr_metadata_from_flat_fields(
                self.subject_type.clone(),
                self.subject_is_awaited,
                self.subject_callee.clone(),
                self.subject_name.clone(),
                self.subject_member_owner_name.clone(),
                self.subject_member_name.clone(),
                self.subject_member_through_instance,
                self.subject_method_owner_name.clone(),
                self.subject_method_name.clone(),
                self.subject_method_through_instance,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
        })
    }
}

impl ForSite {
    #[must_use]
    pub fn iter_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.iter.clone().or_else(|| {
            direct_expr_metadata_from_flat_fields(
                self.iter_type.clone(),
                self.iter_is_awaited,
                self.iter_callee.clone(),
                self.iter_name.clone(),
                self.iter_member_owner_name.clone(),
                self.iter_member_name.clone(),
                self.iter_member_through_instance,
                self.iter_method_owner_name.clone(),
                self.iter_method_name.clone(),
                self.iter_method_through_instance,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
        })
    }
}

impl WithSite {
    #[must_use]
    pub fn context_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.context.clone().or_else(|| {
            direct_expr_metadata_from_flat_fields(
                self.context_type.clone(),
                self.context_is_awaited,
                self.context_callee.clone(),
                self.context_name.clone(),
                self.context_member_owner_name.clone(),
                self.context_member_name.clone(),
                self.context_member_through_instance,
                self.context_method_owner_name.clone(),
                self.context_method_name.clone(),
                self.context_method_through_instance,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
        })
    }
}

impl AssignmentSite {
    #[must_use]
    pub fn annotation_text(&self) -> Option<&str> {
        self.annotation_expr
            .as_ref()
            .map(|annotation| annotation.text.as_str())
            .or(self.annotation.as_deref())
    }

    #[must_use]
    pub fn value_metadata(&self) -> Option<typepython_syntax::DirectExprMetadata> {
        self.value.clone().or_else(|| {
            direct_expr_metadata_from_flat_fields(
                self.value_type.clone(),
                self.is_awaited,
                self.value_callee.clone(),
                self.value_name.clone(),
                self.value_member_owner_name.clone(),
                self.value_member_name.clone(),
                self.value_member_through_instance,
                self.value_method_owner_name.clone(),
                self.value_method_name.clone(),
                self.value_method_through_instance,
                self.value_subscript_target.clone(),
                self.value_subscript_string_key.clone(),
                self.value_subscript_index.clone(),
                self.value_if_true.clone(),
                self.value_if_false.clone(),
                self.value_if_guard.as_ref().map(guard_condition_site_to_guard),
                self.value_bool_left.clone(),
                self.value_bool_right.clone(),
                self.value_binop_left.clone(),
                self.value_binop_right.clone(),
                self.value_binop_operator.clone(),
                self.value_lambda.clone(),
                self.value_list_comprehension.clone(),
                self.value_generator_comprehension.clone(),
                self.value_list_elements.clone(),
                self.value_set_elements.clone(),
                self.value_dict_entries.clone(),
            )
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
            DeclarationMetadata::TypeAlias { value } => value.text.clone(),
            DeclarationMetadata::Callable { signature } => signature.rendered(),
            DeclarationMetadata::Import { target } => target.raw_target.clone(),
            DeclarationMetadata::Value { annotation } => annotation
                .as_ref()
                .map(|annotation| annotation.text.clone())
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
    pub constraints: Vec<String>,
    pub default: Option<String>,
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

/// Transforms a parsed syntax tree into its bound module surface.
#[must_use]
pub fn bind(tree: &SyntaxTree) -> BindingTable {
    BindingTable {
        module_path: tree.source.path.clone(),
        module_key: tree.source.logical_module.clone(),
        module_kind: tree.source.kind,
        surface_facts: bind_module_surface_facts(tree),
        declarations: tree.statements.iter().flat_map(bind_statement).collect(),
        calls: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Call(statement) => Some(CallSite {
                    callee: statement.callee.clone(),
                    arg_count: statement.arg_count,
                    arg_types: statement.arg_types.clone(),
                    arg_values: statement.arg_values.clone(),
                    starred_arg_types: statement.starred_arg_types.clone(),
                    starred_arg_values: statement.starred_arg_values.clone(),
                    keyword_names: statement.keyword_names.clone(),
                    keyword_arg_types: statement.keyword_arg_types.clone(),
                    keyword_arg_values: statement.keyword_arg_values.clone(),
                    keyword_expansion_types: statement.keyword_expansion_types.clone(),
                    keyword_expansion_values: statement.keyword_expansion_values.clone(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        method_calls: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::MethodCall(statement) => Some(MethodCallSite {
                    owner_name: statement.owner_name.clone(),
                    method: statement.method.clone(),
                    through_instance: statement.through_instance,
                    arg_count: statement.arg_count,
                    arg_types: statement.arg_types.clone(),
                    arg_values: statement.arg_values.clone(),
                    starred_arg_types: statement.starred_arg_types.clone(),
                    starred_arg_values: statement.starred_arg_values.clone(),
                    keyword_names: statement.keyword_names.clone(),
                    keyword_arg_types: statement.keyword_arg_types.clone(),
                    keyword_arg_values: statement.keyword_arg_values.clone(),
                    keyword_expansion_types: statement.keyword_expansion_types.clone(),
                    keyword_expansion_values: statement.keyword_expansion_values.clone(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        member_accesses: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::MemberAccess(statement) => Some(MemberAccessSite {
                    current_owner_name: statement.current_owner_name.clone(),
                    current_owner_type_name: statement.current_owner_type_name.clone(),
                    owner_name: statement.owner_name.clone(),
                    member: statement.member.clone(),
                    through_instance: statement.through_instance,
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        returns: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Return(statement) => Some(ReturnSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    value: direct_expr_metadata_from_return_statement(statement),
                    value_type: statement.value_type.clone(),
                    is_awaited: statement.is_awaited,
                    value_callee: statement.value_callee.clone(),
                    value_name: statement.value_name.clone(),
                    value_member_owner_name: statement.value_member_owner_name.clone(),
                    value_member_name: statement.value_member_name.clone(),
                    value_member_through_instance: statement.value_member_through_instance,
                    value_method_owner_name: statement.value_method_owner_name.clone(),
                    value_method_name: statement.value_method_name.clone(),
                    value_method_through_instance: statement.value_method_through_instance,
                    value_subscript_target: statement.value_subscript_target.clone(),
                    value_subscript_string_key: statement.value_subscript_string_key.clone(),
                    value_subscript_index: statement.value_subscript_index.clone(),
                    value_if_true: statement.value_if_true.clone(),
                    value_if_false: statement.value_if_false.clone(),
                    value_if_guard: statement.value_if_guard.as_ref().map(map_guard_condition),
                    value_bool_left: statement.value_bool_left.clone(),
                    value_bool_right: statement.value_bool_right.clone(),
                    value_binop_left: statement.value_binop_left.clone(),
                    value_binop_right: statement.value_binop_right.clone(),
                    value_binop_operator: statement.value_binop_operator.clone(),
                    value_lambda: statement.value_lambda.clone(),
                    value_list_elements: statement.value_list_elements.clone(),
                    value_set_elements: statement.value_set_elements.clone(),
                    value_dict_entries: statement.value_dict_entries.clone(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        yields: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Yield(statement) => Some(YieldSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    value: direct_expr_metadata_from_yield_statement(statement),
                    value_type: statement.value_type.clone(),
                    value_callee: statement.value_callee.clone(),
                    value_name: statement.value_name.clone(),
                    value_member_owner_name: statement.value_member_owner_name.clone(),
                    value_member_name: statement.value_member_name.clone(),
                    value_member_through_instance: statement.value_member_through_instance,
                    value_method_owner_name: statement.value_method_owner_name.clone(),
                    value_method_name: statement.value_method_name.clone(),
                    value_method_through_instance: statement.value_method_through_instance,
                    value_subscript_target: statement.value_subscript_target.clone(),
                    value_subscript_string_key: statement.value_subscript_string_key.clone(),
                    value_subscript_index: statement.value_subscript_index.clone(),
                    value_if_true: statement.value_if_true.clone(),
                    value_if_false: statement.value_if_false.clone(),
                    value_if_guard: statement.value_if_guard.as_ref().map(map_guard_condition),
                    value_bool_left: statement.value_bool_left.clone(),
                    value_bool_right: statement.value_bool_right.clone(),
                    value_binop_left: statement.value_binop_left.clone(),
                    value_binop_right: statement.value_binop_right.clone(),
                    value_binop_operator: statement.value_binop_operator.clone(),
                    value_lambda: statement.value_lambda.clone(),
                    value_list_elements: statement.value_list_elements.clone(),
                    value_set_elements: statement.value_set_elements.clone(),
                    value_dict_entries: statement.value_dict_entries.clone(),
                    is_yield_from: statement.is_yield_from,
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        if_guards: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::If(statement) => Some(IfGuardSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    guard: statement.guard.as_ref().map(map_guard_condition),
                    line: statement.line,
                    true_start_line: statement.true_start_line,
                    true_end_line: statement.true_end_line,
                    false_start_line: statement.false_start_line,
                    false_end_line: statement.false_end_line,
                }),
                _ => None,
            })
            .collect(),
        asserts: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Assert(statement) => Some(AssertGuardSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    guard: statement.guard.as_ref().map(map_guard_condition),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        invalidations: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Invalidate(statement) => Some(InvalidationSite {
                    kind: match statement.kind {
                        typepython_syntax::InvalidationKind::RebindLike => {
                            InvalidationKind::RebindLike
                        }
                        typepython_syntax::InvalidationKind::Delete => InvalidationKind::Delete,
                        typepython_syntax::InvalidationKind::ScopeChange => {
                            InvalidationKind::ScopeChange
                        }
                    },
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    names: statement.names.clone(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        matches: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Match(statement) => Some(MatchSite {
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    subject: direct_expr_metadata_from_match_statement(statement),
                    subject_type: statement.subject_type.clone(),
                    subject_is_awaited: statement.subject_is_awaited,
                    subject_callee: statement.subject_callee.clone(),
                    subject_name: statement.subject_name.clone(),
                    subject_member_owner_name: statement.subject_member_owner_name.clone(),
                    subject_member_name: statement.subject_member_name.clone(),
                    subject_member_through_instance: statement.subject_member_through_instance,
                    subject_method_owner_name: statement.subject_method_owner_name.clone(),
                    subject_method_name: statement.subject_method_name.clone(),
                    subject_method_through_instance: statement.subject_method_through_instance,
                    cases: statement
                        .cases
                        .iter()
                        .map(|case| MatchCaseSite {
                            patterns: case
                                .patterns
                                .iter()
                                .map(|pattern| match pattern {
                                    typepython_syntax::MatchPattern::Wildcard => {
                                        MatchPatternSite::Wildcard
                                    }
                                    typepython_syntax::MatchPattern::Literal(value) => {
                                        MatchPatternSite::Literal(value.clone())
                                    }
                                    typepython_syntax::MatchPattern::Class(value) => {
                                        MatchPatternSite::Class(value.clone())
                                    }
                                    typepython_syntax::MatchPattern::Unsupported => {
                                        MatchPatternSite::Unsupported
                                    }
                                })
                                .collect(),
                            has_guard: case.has_guard,
                            line: case.line,
                        })
                        .collect(),
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        for_loops: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::For(statement) => Some(ForSite {
                    target_name: statement.target_name.clone(),
                    target_names: statement.target_names.clone(),
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    iter: direct_expr_metadata_from_for_statement(statement),
                    iter_type: statement.iter_type.clone(),
                    iter_is_awaited: statement.iter_is_awaited,
                    iter_callee: statement.iter_callee.clone(),
                    iter_name: statement.iter_name.clone(),
                    iter_member_owner_name: statement.iter_member_owner_name.clone(),
                    iter_member_name: statement.iter_member_name.clone(),
                    iter_member_through_instance: statement.iter_member_through_instance,
                    iter_method_owner_name: statement.iter_method_owner_name.clone(),
                    iter_method_name: statement.iter_method_name.clone(),
                    iter_method_through_instance: statement.iter_method_through_instance,
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        with_statements: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::With(statement) => Some(WithSite {
                    target_name: statement.target_name.clone(),
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    context: direct_expr_metadata_from_with_statement(statement),
                    context_type: statement.context_type.clone(),
                    context_is_awaited: statement.context_is_awaited,
                    context_callee: statement.context_callee.clone(),
                    context_name: statement.context_name.clone(),
                    context_member_owner_name: statement.context_member_owner_name.clone(),
                    context_member_name: statement.context_member_name.clone(),
                    context_member_through_instance: statement.context_member_through_instance,
                    context_method_owner_name: statement.context_method_owner_name.clone(),
                    context_method_name: statement.context_method_name.clone(),
                    context_method_through_instance: statement.context_method_through_instance,
                    line: statement.line,
                }),
                _ => None,
            })
            .collect(),
        except_handlers: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::ExceptHandler(statement) => Some(ExceptHandlerSite {
                    exception_type: statement.exception_type.clone(),
                    binding_name: statement.binding_name.clone(),
                    owner_name: statement.owner_name.clone(),
                    owner_type_name: statement.owner_type_name.clone(),
                    line: statement.line,
                    end_line: statement.end_line,
                }),
                _ => None,
            })
            .collect(),
        assignments: tree
            .statements
            .iter()
            .flat_map(|statement| match statement {
                SyntaxStatement::Value(statement) => statement
                    .names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| AssignmentSite {
                        name: name.clone(),
                        destructuring_target_names: statement.destructuring_target_names.clone(),
                        destructuring_index: statement
                            .destructuring_target_names
                            .as_ref()
                            .map(|_| index),
                        annotation: statement.annotation.clone(),
                        annotation_expr: statement.annotation.clone().map(BoundTypeExpr::new),
                        value: direct_expr_metadata_from_value_statement(statement),
                        value_type: statement.value_type.clone(),
                        is_awaited: statement.is_awaited,
                        value_callee: statement.value_callee.clone(),
                        value_name: statement.value_name.clone(),
                        value_member_owner_name: statement.value_member_owner_name.clone(),
                        value_member_name: statement.value_member_name.clone(),
                        value_member_through_instance: statement.value_member_through_instance,
                        value_method_owner_name: statement.value_method_owner_name.clone(),
                        value_method_name: statement.value_method_name.clone(),
                        value_method_through_instance: statement.value_method_through_instance,
                        value_subscript_target: statement.value_subscript_target.clone(),
                        value_subscript_string_key: statement.value_subscript_string_key.clone(),
                        value_subscript_index: statement.value_subscript_index.clone(),
                        value_if_true: statement.value_if_true.clone(),
                        value_if_false: statement.value_if_false.clone(),
                        value_if_guard: statement.value_if_guard.as_ref().map(map_guard_condition),
                        value_bool_left: statement.value_bool_left.clone(),
                        value_bool_right: statement.value_bool_right.clone(),
                        value_binop_left: statement.value_binop_left.clone(),
                        value_binop_right: statement.value_binop_right.clone(),
                        value_binop_operator: statement.value_binop_operator.clone(),
                        value_lambda: statement.value_lambda.clone(),
                        value_list_comprehension: statement.value_list_comprehension.clone(),
                        value_generator_comprehension: statement
                            .value_generator_comprehension
                            .clone(),
                        value_list_elements: statement.value_list_elements.clone(),
                        value_set_elements: statement.value_set_elements.clone(),
                        value_dict_entries: statement.value_dict_entries.clone(),
                        owner_name: statement.owner_name.clone(),
                        owner_type_name: statement.owner_type_name.clone(),
                        line: statement.line,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect(),
    }
}

fn bind_module_surface_facts(tree: &SyntaxTree) -> ModuleSurfaceFacts {
    let collected = typepython_syntax::collect_module_surface_metadata(&tree.source.text);
    let typed_dict_class_metadata = collected
        .typed_dict_classes
        .into_iter()
        .map(|metadata| (metadata.name.clone(), metadata))
        .collect();
    let direct_function_signatures = collected
        .direct_function_signatures
        .into_iter()
        .map(|signature| (signature.name, signature.params))
        .collect();
    let direct_method_signatures = collected
        .direct_method_signatures
        .into_iter()
        .map(|signature| {
            let params = match signature.method_kind {
                MethodKind::Static | MethodKind::Property => signature.params,
                MethodKind::Instance | MethodKind::Class | MethodKind::PropertySetter => {
                    signature.params.into_iter().skip(1).collect()
                }
            };
            ((signature.owner_type_name, signature.name), params)
        })
        .collect();

    ModuleSurfaceFacts {
        typed_dict_class_metadata,
        direct_function_signatures,
        direct_method_signatures,
        decorator_transform_module_info: collected.decorator_transform,
        dataclass_transform_module_info: collected.dataclass_transform,
    }
}

fn bind_statement(statement: &SyntaxStatement) -> Vec<Declaration> {
    match statement {
        SyntaxStatement::TypeAlias(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::TypeAlias,
            detail: statement.value.clone(),
            metadata: DeclarationMetadata::TypeAlias {
                value: BoundTypeExpr::new(statement.value.clone()),
            },
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
            type_params: bind_type_params(&statement.type_params),
        }],
        SyntaxStatement::Interface(statement) => {
            bind_named_block(statement, DeclarationOwnerKind::Interface)
        }
        SyntaxStatement::DataClass(statement) => {
            bind_named_block(statement, DeclarationOwnerKind::DataClass)
        }
        SyntaxStatement::SealedClass(statement) => {
            bind_named_block(statement, DeclarationOwnerKind::SealedClass)
        }
        SyntaxStatement::ClassDef(statement) => {
            bind_named_block(statement, DeclarationOwnerKind::Class)
        }
        SyntaxStatement::OverloadDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Overload,
            detail: BoundCallableSignature::from_function_parts(
                &statement.params,
                statement.returns.as_deref(),
            )
            .rendered(),
            metadata: DeclarationMetadata::Callable {
                signature: BoundCallableSignature::from_function_parts(
                    &statement.params,
                    statement.returns.as_deref(),
                ),
            },
            value_type: None,
            method_kind: None,
            class_kind: None,
            owner: None,
            is_async: statement.is_async,
            is_override: false,
            is_abstract_method: false,
            is_final_decorator: false,
            is_deprecated: statement.is_deprecated,
            deprecation_message: statement.deprecation_message.clone(),
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
            type_params: bind_type_params(&statement.type_params),
        }],
        SyntaxStatement::FunctionDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Function,
            detail: BoundCallableSignature::from_function_parts(
                &statement.params,
                statement.returns.as_deref(),
            )
            .rendered(),
            metadata: DeclarationMetadata::Callable {
                signature: BoundCallableSignature::from_function_parts(
                    &statement.params,
                    statement.returns.as_deref(),
                ),
            },
            value_type: None,
            method_kind: None,
            class_kind: None,
            owner: None,
            is_async: statement.is_async,
            is_override: statement.is_override,
            is_abstract_method: false,
            is_final_decorator: false,
            is_deprecated: statement.is_deprecated,
            deprecation_message: statement.deprecation_message.clone(),
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
            type_params: bind_type_params(&statement.type_params),
        }],
        SyntaxStatement::Import(statement) => statement
            .bindings
            .iter()
            .map(|binding| Declaration {
                name: binding.local_name.clone(),
                kind: DeclarationKind::Import,
                detail: binding.source_path.clone(),
                metadata: DeclarationMetadata::Import {
                    target: BoundImportTarget::new(binding.source_path.clone()),
                },
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
                type_params: Vec::new(),
            })
            .collect(),
        SyntaxStatement::Value(statement) => (statement.owner_name.is_none()
            && !value_statement_is_rebind_like_update(statement))
        .then_some(statement)
        .into_iter()
        .flat_map(|statement| {
            statement
                .names
                .iter()
                .cloned()
                .map(|name| Declaration {
                    name,
                    kind: DeclarationKind::Value,
                    detail: statement.annotation.clone().unwrap_or_default(),
                    metadata: DeclarationMetadata::Value {
                        annotation: statement.annotation.clone().map(BoundTypeExpr::new),
                    },
                    value_type: statement.value_type.clone(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: statement.is_final,
                    is_class_var: statement.is_class_var,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect(),
        SyntaxStatement::Call(_) => Vec::new(),
        SyntaxStatement::MethodCall(_) => Vec::new(),
        SyntaxStatement::MemberAccess(_) => Vec::new(),
        SyntaxStatement::Return(_) => Vec::new(),
        SyntaxStatement::Yield(_) => Vec::new(),
        SyntaxStatement::If(_) => Vec::new(),
        SyntaxStatement::Assert(_) => Vec::new(),
        SyntaxStatement::Invalidate(_) => Vec::new(),
        SyntaxStatement::Match(_) => Vec::new(),
        SyntaxStatement::For(_) => Vec::new(),
        SyntaxStatement::With(_) => Vec::new(),
        SyntaxStatement::ExceptHandler(_) => Vec::new(),
        SyntaxStatement::Unsafe(_) => Vec::new(),
    }
}

fn value_statement_is_rebind_like_update(statement: &typepython_syntax::ValueStatement) -> bool {
    statement.annotation.is_none()
        && statement.names.len() == 1
        && statement.value_binop_operator.is_some()
        && statement.value_binop_left.as_deref().and_then(|left| left.value_name.as_deref())
            == statement.names.first().map(String::as_str)
}

fn map_guard_condition(condition: &typepython_syntax::GuardCondition) -> GuardConditionSite {
    match condition {
        typepython_syntax::GuardCondition::IsNone { name, negated } => {
            GuardConditionSite::IsNone { name: name.clone(), negated: *negated }
        }
        typepython_syntax::GuardCondition::IsInstance { name, types } => {
            GuardConditionSite::IsInstance { name: name.clone(), types: types.clone() }
        }
        typepython_syntax::GuardCondition::PredicateCall { name, callee } => {
            GuardConditionSite::PredicateCall { name: name.clone(), callee: callee.clone() }
        }
        typepython_syntax::GuardCondition::TruthyName { name } => {
            GuardConditionSite::TruthyName { name: name.clone() }
        }
        typepython_syntax::GuardCondition::Not(condition) => {
            GuardConditionSite::Not(Box::new(map_guard_condition(condition)))
        }
        typepython_syntax::GuardCondition::And(conditions) => {
            GuardConditionSite::And(conditions.iter().map(map_guard_condition).collect())
        }
        typepython_syntax::GuardCondition::Or(conditions) => {
            GuardConditionSite::Or(conditions.iter().map(map_guard_condition).collect())
        }
    }
}

fn bind_named_block(
    statement: &typepython_syntax::NamedBlockStatement,
    owner_kind: DeclarationOwnerKind,
) -> Vec<Declaration> {
    let owner = DeclarationOwner { name: statement.name.clone(), kind: owner_kind };
    let mut declarations = vec![Declaration {
        name: statement.name.clone(),
        kind: DeclarationKind::Class,
        detail: statement.bases.join(","),
        metadata: DeclarationMetadata::Class { bases: statement.bases.clone() },
        value_type: None,
        method_kind: None,
        class_kind: Some(owner_kind),
        owner: None,
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: statement.is_final_decorator,
        is_deprecated: statement.is_deprecated,
        deprecation_message: statement.deprecation_message.clone(),
        is_final: false,
        is_class_var: false,
        bases: statement.bases.clone(),
        type_params: bind_type_params(&statement.type_params),
    }];
    declarations.extend(statement.members.iter().map(|member| Declaration {
        name: member.name.clone(),
        kind: match member.kind {
            typepython_syntax::ClassMemberKind::Field => DeclarationKind::Value,
            typepython_syntax::ClassMemberKind::Method => DeclarationKind::Function,
            typepython_syntax::ClassMemberKind::Overload => DeclarationKind::Overload,
        },
        detail: match member.kind {
            typepython_syntax::ClassMemberKind::Field => {
                member.annotation.clone().unwrap_or_default()
            }
            typepython_syntax::ClassMemberKind::Method
            | typepython_syntax::ClassMemberKind::Overload => {
                format_signature(&member.params, member.returns.as_deref())
            }
        },
        metadata: match member.kind {
            typepython_syntax::ClassMemberKind::Field => DeclarationMetadata::Value {
                annotation: member.annotation.clone().map(BoundTypeExpr::new),
            },
            typepython_syntax::ClassMemberKind::Method
            | typepython_syntax::ClassMemberKind::Overload => DeclarationMetadata::Callable {
                signature: BoundCallableSignature::from_function_parts(
                    &member.params,
                    member.returns.as_deref(),
                ),
            },
        },
        value_type: member.value_type.clone(),
        method_kind: member.method_kind,
        class_kind: None,
        owner: Some(owner.clone()),
        is_async: member.is_async,
        is_override: member.is_override,
        is_abstract_method: member.is_abstract_method,
        is_final_decorator: member.is_final_decorator,
        is_deprecated: member.is_deprecated,
        deprecation_message: member.deprecation_message.clone(),
        is_final: member.is_final,
        is_class_var: member.is_class_var,
        bases: Vec::new(),
        type_params: Vec::new(),
    }));
    declarations
}

fn direct_param_site_from_function_param(
    param: &typepython_syntax::FunctionParam,
) -> typepython_syntax::DirectFunctionParamSite {
    typepython_syntax::DirectFunctionParamSite {
        name: param.name.clone(),
        annotation: param.annotation.clone(),
        has_default: param.has_default,
        positional_only: param.positional_only,
        keyword_only: param.keyword_only,
        variadic: param.variadic,
        keyword_variadic: param.keyword_variadic,
    }
}

fn format_signature(params: &[typepython_syntax::FunctionParam], returns: Option<&str>) -> String {
    render_signature_from_direct_params(
        &params.iter().map(direct_param_site_from_function_param).collect::<Vec<_>>(),
        returns,
    )
}

fn render_signature_from_direct_params(
    params: &[typepython_syntax::DirectFunctionParamSite],
    returns: Option<&str>,
) -> String {
    let mut rendered_params = Vec::new();
    let positional_only_count = params.iter().filter(|param| param.positional_only).count();
    let has_variadic = params.iter().any(|param| param.variadic);
    let keyword_only_index = params.iter().position(|param| param.keyword_only);

    for (index, param) in params.iter().enumerate() {
        if keyword_only_index == Some(index) && !has_variadic {
            rendered_params.push(String::from("*"));
        }

        let mut rendered = match &param.annotation {
            Some(annotation) => format!("{}:{}", param.name, annotation),
            None => param.name.clone(),
        };
        if param.has_default {
            rendered.push('=');
        }
        if param.keyword_variadic {
            rendered = format!("**{rendered}");
        } else if param.variadic {
            rendered = format!("*{rendered}");
        }
        rendered_params.push(rendered);

        if positional_only_count > 0 && index + 1 == positional_only_count {
            rendered_params.push(String::from("/"));
        }
    }

    format!("({})->{}", rendered_params.join(","), returns.unwrap_or(""))
}

fn direct_expr_metadata_from_value_statement(
    statement: &typepython_syntax::ValueStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_flat_fields(
        statement.value_type.clone(),
        statement.is_awaited,
        statement.value_callee.clone(),
        statement.value_name.clone(),
        statement.value_member_owner_name.clone(),
        statement.value_member_name.clone(),
        statement.value_member_through_instance,
        statement.value_method_owner_name.clone(),
        statement.value_method_name.clone(),
        statement.value_method_through_instance,
        statement.value_subscript_target.clone(),
        statement.value_subscript_string_key.clone(),
        statement.value_subscript_index.clone(),
        statement.value_if_true.clone(),
        statement.value_if_false.clone(),
        statement.value_if_guard.clone(),
        statement.value_bool_left.clone(),
        statement.value_bool_right.clone(),
        statement.value_binop_left.clone(),
        statement.value_binop_right.clone(),
        statement.value_binop_operator.clone(),
        statement.value_lambda.clone(),
        statement.value_list_comprehension.clone(),
        statement.value_generator_comprehension.clone(),
        statement.value_list_elements.clone(),
        statement.value_set_elements.clone(),
        statement.value_dict_entries.clone(),
    )
}

fn direct_expr_metadata_from_return_statement(
    statement: &typepython_syntax::ReturnStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_flat_fields(
        statement.value_type.clone(),
        statement.is_awaited,
        statement.value_callee.clone(),
        statement.value_name.clone(),
        statement.value_member_owner_name.clone(),
        statement.value_member_name.clone(),
        statement.value_member_through_instance,
        statement.value_method_owner_name.clone(),
        statement.value_method_name.clone(),
        statement.value_method_through_instance,
        statement.value_subscript_target.clone(),
        statement.value_subscript_string_key.clone(),
        statement.value_subscript_index.clone(),
        statement.value_if_true.clone(),
        statement.value_if_false.clone(),
        statement.value_if_guard.clone(),
        statement.value_bool_left.clone(),
        statement.value_bool_right.clone(),
        statement.value_binop_left.clone(),
        statement.value_binop_right.clone(),
        statement.value_binop_operator.clone(),
        statement.value_lambda.clone(),
        None,
        None,
        statement.value_list_elements.clone(),
        statement.value_set_elements.clone(),
        statement.value_dict_entries.clone(),
    )
}

fn direct_expr_metadata_from_yield_statement(
    statement: &typepython_syntax::YieldStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_flat_fields(
        statement.value_type.clone(),
        false,
        statement.value_callee.clone(),
        statement.value_name.clone(),
        statement.value_member_owner_name.clone(),
        statement.value_member_name.clone(),
        statement.value_member_through_instance,
        statement.value_method_owner_name.clone(),
        statement.value_method_name.clone(),
        statement.value_method_through_instance,
        statement.value_subscript_target.clone(),
        statement.value_subscript_string_key.clone(),
        statement.value_subscript_index.clone(),
        statement.value_if_true.clone(),
        statement.value_if_false.clone(),
        statement.value_if_guard.clone(),
        statement.value_bool_left.clone(),
        statement.value_bool_right.clone(),
        statement.value_binop_left.clone(),
        statement.value_binop_right.clone(),
        statement.value_binop_operator.clone(),
        statement.value_lambda.clone(),
        None,
        None,
        statement.value_list_elements.clone(),
        statement.value_set_elements.clone(),
        statement.value_dict_entries.clone(),
    )
}

fn direct_expr_metadata_from_match_statement(
    statement: &typepython_syntax::MatchStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_flat_fields(
        statement.subject_type.clone(),
        statement.subject_is_awaited,
        statement.subject_callee.clone(),
        statement.subject_name.clone(),
        statement.subject_member_owner_name.clone(),
        statement.subject_member_name.clone(),
        statement.subject_member_through_instance,
        statement.subject_method_owner_name.clone(),
        statement.subject_method_name.clone(),
        statement.subject_method_through_instance,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

fn direct_expr_metadata_from_for_statement(
    statement: &typepython_syntax::ForStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_flat_fields(
        statement.iter_type.clone(),
        statement.iter_is_awaited,
        statement.iter_callee.clone(),
        statement.iter_name.clone(),
        statement.iter_member_owner_name.clone(),
        statement.iter_member_name.clone(),
        statement.iter_member_through_instance,
        statement.iter_method_owner_name.clone(),
        statement.iter_method_name.clone(),
        statement.iter_method_through_instance,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

fn direct_expr_metadata_from_with_statement(
    statement: &typepython_syntax::WithStatement,
) -> Option<typepython_syntax::DirectExprMetadata> {
    direct_expr_metadata_from_flat_fields(
        statement.context_type.clone(),
        statement.context_is_awaited,
        statement.context_callee.clone(),
        statement.context_name.clone(),
        statement.context_member_owner_name.clone(),
        statement.context_member_name.clone(),
        statement.context_member_through_instance,
        statement.context_method_owner_name.clone(),
        statement.context_method_name.clone(),
        statement.context_method_through_instance,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

fn direct_expr_metadata_from_flat_fields(
    value_type: Option<String>,
    is_awaited: bool,
    value_callee: Option<String>,
    value_name: Option<String>,
    value_member_owner_name: Option<String>,
    value_member_name: Option<String>,
    value_member_through_instance: bool,
    value_method_owner_name: Option<String>,
    value_method_name: Option<String>,
    value_method_through_instance: bool,
    value_subscript_target: Option<Box<typepython_syntax::DirectExprMetadata>>,
    value_subscript_string_key: Option<String>,
    value_subscript_index: Option<String>,
    value_if_true: Option<Box<typepython_syntax::DirectExprMetadata>>,
    value_if_false: Option<Box<typepython_syntax::DirectExprMetadata>>,
    value_if_guard: Option<typepython_syntax::GuardCondition>,
    value_bool_left: Option<Box<typepython_syntax::DirectExprMetadata>>,
    value_bool_right: Option<Box<typepython_syntax::DirectExprMetadata>>,
    value_binop_left: Option<Box<typepython_syntax::DirectExprMetadata>>,
    value_binop_right: Option<Box<typepython_syntax::DirectExprMetadata>>,
    value_binop_operator: Option<String>,
    value_lambda: Option<Box<typepython_syntax::LambdaMetadata>>,
    value_list_comprehension: Option<Box<typepython_syntax::ComprehensionMetadata>>,
    value_generator_comprehension: Option<Box<typepython_syntax::ComprehensionMetadata>>,
    value_list_elements: Option<Vec<typepython_syntax::DirectExprMetadata>>,
    value_set_elements: Option<Vec<typepython_syntax::DirectExprMetadata>>,
    value_dict_entries: Option<Vec<typepython_syntax::TypedDictLiteralEntry>>,
) -> Option<typepython_syntax::DirectExprMetadata> {
    let metadata = typepython_syntax::DirectExprMetadata {
        value_type,
        is_awaited,
        value_callee,
        value_name,
        value_member_owner_name,
        value_member_name,
        value_member_through_instance,
        value_method_owner_name,
        value_method_name,
        value_method_through_instance,
        value_subscript_target,
        value_subscript_string_key,
        value_subscript_index,
        value_if_true,
        value_if_false,
        value_if_guard,
        value_bool_left,
        value_bool_right,
        value_binop_left,
        value_binop_right,
        value_binop_operator,
        value_lambda,
        value_list_comprehension,
        value_generator_comprehension,
        value_list_elements,
        value_set_elements,
        value_dict_entries,
    };
    direct_expr_metadata_present(&metadata).then_some(metadata)
}

fn direct_expr_metadata_present(metadata: &typepython_syntax::DirectExprMetadata) -> bool {
    metadata.value_type.as_deref().is_some_and(|value| !value.is_empty())
        || metadata.is_awaited
        || metadata.value_callee.is_some()
        || metadata.value_name.is_some()
        || metadata.value_member_owner_name.is_some()
        || metadata.value_member_name.is_some()
        || metadata.value_member_through_instance
        || metadata.value_method_owner_name.is_some()
        || metadata.value_method_name.is_some()
        || metadata.value_method_through_instance
        || metadata.value_subscript_target.is_some()
        || metadata.value_subscript_string_key.is_some()
        || metadata.value_subscript_index.is_some()
        || metadata.value_if_true.is_some()
        || metadata.value_if_false.is_some()
        || metadata.value_if_guard.is_some()
        || metadata.value_bool_left.is_some()
        || metadata.value_bool_right.is_some()
        || metadata.value_binop_left.is_some()
        || metadata.value_binop_right.is_some()
        || metadata.value_binop_operator.is_some()
        || metadata.value_lambda.is_some()
        || metadata.value_list_comprehension.is_some()
        || metadata.value_generator_comprehension.is_some()
        || metadata.value_list_elements.is_some()
        || metadata.value_set_elements.is_some()
        || metadata.value_dict_entries.is_some()
}

fn guard_condition_site_to_guard(
    condition: &GuardConditionSite,
) -> typepython_syntax::GuardCondition {
    match condition {
        GuardConditionSite::IsNone { name, negated } => {
            typepython_syntax::GuardCondition::IsNone { name: name.clone(), negated: *negated }
        }
        GuardConditionSite::IsInstance { name, types } => {
            typepython_syntax::GuardCondition::IsInstance {
                name: name.clone(),
                types: types.clone(),
            }
        }
        GuardConditionSite::PredicateCall { name, callee } => {
            typepython_syntax::GuardCondition::PredicateCall {
                name: name.clone(),
                callee: callee.clone(),
            }
        }
        GuardConditionSite::TruthyName { name } => {
            typepython_syntax::GuardCondition::TruthyName { name: name.clone() }
        }
        GuardConditionSite::Not(inner) => {
            typepython_syntax::GuardCondition::Not(Box::new(guard_condition_site_to_guard(inner)))
        }
        GuardConditionSite::And(conditions) => typepython_syntax::GuardCondition::And(
            conditions.iter().map(guard_condition_site_to_guard).collect(),
        ),
        GuardConditionSite::Or(conditions) => typepython_syntax::GuardCondition::Or(
            conditions.iter().map(guard_condition_site_to_guard).collect(),
        ),
    }
}

fn bind_type_params(type_params: &[typepython_syntax::TypeParam]) -> Vec<GenericTypeParam> {
    type_params
        .iter()
        .map(|param| GenericTypeParam {
            kind: match param.kind {
                typepython_syntax::TypeParamKind::TypeVar => GenericTypeParamKind::TypeVar,
                typepython_syntax::TypeParamKind::ParamSpec => GenericTypeParamKind::ParamSpec,
                typepython_syntax::TypeParamKind::TypeVarTuple => {
                    GenericTypeParamKind::TypeVarTuple
                }
            },
            name: param.name.clone(),
            bound: param.bound.clone(),
            constraints: param.constraints.clone(),
            default: param.default.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        bind, AssertGuardSite, AssignmentSite, BoundCallableSignature, BoundImportTarget,
        BoundTypeExpr, Declaration, DeclarationKind, DeclarationMetadata, DeclarationOwner,
        DeclarationOwnerKind, ExceptHandlerSite, ForSite, GenericTypeParam, GenericTypeParamKind,
        GuardConditionSite, IfGuardSite, InvalidationKind, InvalidationSite, MatchCaseSite,
        MatchPatternSite, MatchSite, WithSite, YieldSite,
    };
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{
        ClassMember, ClassMemberKind, DirectExprMetadata, FunctionParam, FunctionStatement,
        ImportStatement, MethodKind, NamedBlockStatement, SourceFile, SourceKind, SyntaxStatement,
        SyntaxTree, TypeAliasStatement, TypeParam, TypeParamKind, ValueStatement,
    };

    fn metadata_type_alias(text: &str) -> DeclarationMetadata {
        DeclarationMetadata::TypeAlias { value: BoundTypeExpr::new(text) }
    }

    fn metadata_value(annotation: Option<&str>) -> DeclarationMetadata {
        DeclarationMetadata::Value { annotation: annotation.map(BoundTypeExpr::new) }
    }

    fn metadata_class(bases: &[&str]) -> DeclarationMetadata {
        DeclarationMetadata::Class { bases: bases.iter().map(|base| String::from(*base)).collect() }
    }

    fn metadata_import(target: &str) -> DeclarationMetadata {
        DeclarationMetadata::Import { target: BoundImportTarget::new(target) }
    }

    fn metadata_empty_callable() -> DeclarationMetadata {
        DeclarationMetadata::Callable {
            signature: BoundCallableSignature { params: Vec::new(), returns: None },
        }
    }

    #[test]
    fn bind_collects_top_level_aliases_classes_and_functions() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/__init__.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::from("app"),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserId"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                    }],
                    value: String::from("Box[T]"),
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                    }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("helper"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                    }],
                    params: Vec::new(),
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 3,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        println!("{} {:?}", table.module_key, table.declarations);
        assert_eq!(table.module_key, "app");
        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    metadata: metadata_type_alias("Box[T]"),
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                    detail: String::from("Box[T]"),
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
                    type_params: vec![GenericTypeParam {
                        name: String::from("T"),
                        kind: GenericTypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                    }],
                },
                Declaration {
                    metadata: metadata_class(&[]),
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                    type_params: vec![GenericTypeParam {
                        name: String::from("T"),
                        kind: GenericTypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                    }],
                },
                Declaration {
                    metadata: metadata_empty_callable(),
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
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
                    type_params: vec![GenericTypeParam {
                        name: String::from("T"),
                        kind: GenericTypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                    }],
                },
            ]
        );
    }

    #[test]
    fn bind_marks_async_functions() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/fetch.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("fetch"),
                type_params: Vec::new(),
                params: Vec::new(),
                returns: Some(String::from("int")),
                is_async: true,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert!(table.declarations[0].is_async);
        assert_eq!(table.declarations[0].detail, String::from("()->int"));
    }

    #[test]
    fn bind_marks_overload_definitions_separately() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/__init__.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                    }],
                    params: Vec::new(),
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    metadata: metadata_empty_callable(),
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                    detail: String::from("()->"),
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
                    type_params: vec![GenericTypeParam {
                        name: String::from("T"),
                        kind: GenericTypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: None,
                    }],
                },
                Declaration {
                    metadata: metadata_empty_callable(),
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
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
                    type_params: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_collects_imports_and_values_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/helpers.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![
                        typepython_syntax::ImportBinding {
                            local_name: String::from("local_foo"),
                            source_path: String::from("pkg.foo"),
                        },
                        typepython_syntax::ImportBinding {
                            local_name: String::from("bar"),
                            source_path: String::from("pkg.bar"),
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value"), String::from("count")],
                    destructuring_target_names: None,
                    annotation: None,
                    value_type: None,
                    is_awaited: false,
                    value_callee: None,
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
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    metadata: metadata_import("pkg.foo"),
                    name: String::from("local_foo"),
                    kind: DeclarationKind::Import,
                    detail: String::from("pkg.foo"),
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
                    type_params: Vec::new(),
                },
                Declaration {
                    metadata: metadata_import("pkg.bar"),
                    name: String::from("bar"),
                    kind: DeclarationKind::Import,
                    detail: String::from("pkg.bar"),
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
                    type_params: Vec::new(),
                },
                Declaration {
                    metadata: metadata_value(None),
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
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
                    type_params: Vec::new(),
                },
                Declaration {
                    metadata: metadata_value(None),
                    name: String::from("count"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
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
                    type_params: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_collects_assignment_sites_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/helpers.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    is_final: false,
                    is_class_var: false,
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("copy")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("source")),
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
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.assignments,
            vec![
                AssignmentSite {
                    annotation_expr: Some(BoundTypeExpr::new("int")),
                    value: Some(DirectExprMetadata {
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: Some(String::from("helper")),
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
                    }),
                    name: String::from("value"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    line: 1,
                },
                AssignmentSite {
                    annotation_expr: Some(BoundTypeExpr::new("str")),
                    value: Some(DirectExprMetadata {
                        value_type: Some(String::new()),
                        is_awaited: false,
                        value_callee: None,
                        value_name: Some(String::from("source")),
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
                    }),
                    name: String::from("copy"),
                    destructuring_target_names: None,
                    destructuring_index: None,
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("source")),
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
                    line: 2,
                },
            ]
        );
    }

    #[test]
    fn bind_keeps_local_assignments_out_of_declarations() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/helpers.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![typepython_syntax::FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("str")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    }],
                    returns: Some(String::from("None")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("result")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("int")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(table.declarations[0].name, "build");
        assert_eq!(
            table.assignments,
            vec![AssignmentSite {
                annotation_expr: Some(BoundTypeExpr::new("int")),
                value: Some(DirectExprMetadata {
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("value")),
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
                }),
                name: String::from("result"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("value")),
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
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 2,
            }]
        );
    }

    #[test]
    fn bind_collects_local_bare_assignments() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/helpers.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("None")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("result")],
                    destructuring_target_names: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(table.declarations[0].name, "build");
        assert_eq!(
            table.assignments,
            vec![AssignmentSite {
                annotation_expr: None,
                value: Some(DirectExprMetadata {
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("helper")),
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
                }),
                name: String::from("result"),
                destructuring_target_names: None,
                destructuring_index: None,
                annotation: None,
                value_type: Some(String::new()),
                is_awaited: false,
                value_callee: Some(String::from("helper")),
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
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 2,
            }]
        );
    }

    #[test]
    fn bind_tracks_destructuring_assignment_indexes() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/helpers.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("left"), String::from("right")],
                destructuring_target_names: Some(vec![String::from("left"), String::from("right")]),
                annotation: None,
                value_type: Some(String::from("tuple[int, str]")),
                is_awaited: false,
                value_callee: None,
                value_name: Some(String::from("pair")),
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
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                is_final: false,
                is_class_var: false,
                line: 2,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.assignments.len(), 2);
        assert_eq!(table.assignments[0].name, "left");
        assert_eq!(table.assignments[0].destructuring_index, Some(0));
        assert_eq!(
            table.assignments[0].destructuring_target_names,
            Some(vec![String::from("left"), String::from("right")])
        );
        assert_eq!(table.assignments[1].name, "right");
        assert_eq!(table.assignments[1].destructuring_index, Some(1));
    }

    #[test]
    fn bind_collects_yield_sites_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/gen.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Yield(typepython_syntax::YieldStatement {
                owner_name: String::from("produce"),
                owner_type_name: None,
                value_type: Some(String::from("int")),
                value_callee: None,
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                is_yield_from: false,
                line: 2,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.yields,
            vec![YieldSite {
                value: Some(DirectExprMetadata {
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
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
                }),
                owner_name: String::from("produce"),
                owner_type_name: None,
                value_type: Some(String::from("int")),
                value_callee: None,
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                is_yield_from: false,
                line: 2,
            }]
        );
    }

    #[test]
    fn bind_collects_for_sites_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/loop.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::For(typepython_syntax::ForStatement {
                target_name: String::from("item"),
                target_names: Vec::new(),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("values")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.for_loops,
            vec![ForSite {
                iter: Some(DirectExprMetadata {
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("values")),
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
                }),
                target_name: String::from("item"),
                target_names: Vec::new(),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                iter_type: Some(String::new()),
                iter_is_awaited: false,
                iter_callee: None,
                iter_name: Some(String::from("values")),
                iter_member_owner_name: None,
                iter_member_name: None,
                iter_member_through_instance: false,
                iter_method_owner_name: None,
                iter_method_name: None,
                iter_method_through_instance: false,
                line: 2,
            }]
        );
    }

    #[test]
    fn bind_collects_match_sites_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/match.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Match(typepython_syntax::MatchStatement {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("expr")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![typepython_syntax::MatchCaseStatement {
                    patterns: vec![typepython_syntax::MatchPattern::Class(String::from("Add"))],
                    has_guard: false,
                    line: 3,
                }],
                line: 2,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.matches,
            vec![MatchSite {
                subject: Some(DirectExprMetadata {
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("expr")),
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
                }),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("expr")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![MatchCaseSite {
                    patterns: vec![MatchPatternSite::Class(String::from("Add"))],
                    has_guard: false,
                    line: 3,
                }],
                line: 2,
            }]
        );
    }

    #[test]
    fn bind_collects_if_and_assert_guard_sites_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/guards.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::If(typepython_syntax::IfStatement {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_syntax::GuardCondition::IsNone {
                        name: String::from("value"),
                        negated: true,
                    }),
                    line: 2,
                    true_start_line: 3,
                    true_end_line: 3,
                    false_start_line: None,
                    false_end_line: None,
                }),
                SyntaxStatement::Assert(typepython_syntax::AssertStatement {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    guard: Some(typepython_syntax::GuardCondition::TruthyName {
                        name: String::from("ready"),
                    }),
                    line: 4,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.if_guards,
            vec![IfGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(GuardConditionSite::IsNone {
                    name: String::from("value"),
                    negated: true,
                }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            }]
        );
        assert_eq!(
            table.asserts,
            vec![AssertGuardSite {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(GuardConditionSite::TruthyName { name: String::from("ready") }),
                line: 4,
            }]
        );
    }

    #[test]
    fn bind_collects_invalidation_sites_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/invalidate.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Invalidate(
                typepython_syntax::InvalidationStatement {
                    kind: typepython_syntax::InvalidationKind::Delete,
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 3,
                },
            )],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.invalidations,
            vec![InvalidationSite {
                kind: InvalidationKind::Delete,
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                names: vec![String::from("value")],
                line: 3,
            }]
        );
    }

    #[test]
    fn bind_collects_with_sites_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/with.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::With(typepython_syntax::WithStatement {
                target_name: Some(String::from("value")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                context_type: Some(String::new()),
                context_is_awaited: false,
                context_callee: None,
                context_name: Some(String::from("manager")),
                context_member_owner_name: None,
                context_member_name: None,
                context_member_through_instance: false,
                context_method_owner_name: None,
                context_method_name: None,
                context_method_through_instance: false,
                line: 2,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.with_statements,
            vec![WithSite {
                context: Some(DirectExprMetadata {
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("manager")),
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
                }),
                target_name: Some(String::from("value")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                context_type: Some(String::new()),
                context_is_awaited: false,
                context_callee: None,
                context_name: Some(String::from("manager")),
                context_member_owner_name: None,
                context_member_name: None,
                context_member_through_instance: false,
                context_method_owner_name: None,
                context_method_name: None,
                context_method_through_instance: false,
                line: 2,
            }]
        );
    }

    #[test]
    fn bind_collects_except_handler_sites_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/try.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::ExceptHandler(
                typepython_syntax::ExceptionHandlerStatement {
                    exception_type: String::from("ValueError"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                },
            )],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.except_handlers,
            vec![ExceptHandlerSite {
                exception_type: String::from("ValueError"),
                binding_name: Some(String::from("e")),
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                line: 4,
                end_line: 5,
            }]
        );
    }

    #[test]
    fn bind_collects_class_like_member_declarations_with_owner() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/models.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("value"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("close"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("close"),
                        kind: ClassMemberKind::Overload,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    },
                ],
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    metadata: metadata_class(&[]),
                    name: String::from("SupportsClose"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
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
                    metadata: metadata_value(None),
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
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
                    metadata: metadata_empty_callable(),
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
                    value_type: None,
                    method_kind: Some(MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
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
                    metadata: metadata_empty_callable(),
                    name: String::from("close"),
                    kind: DeclarationKind::Overload,
                    detail: String::from("()->"),
                    value_type: None,
                    method_kind: Some(MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
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
            ]
        );
    }

    #[test]
    fn bind_marks_final_values_and_fields() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/finals.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("MAX_SIZE")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("Final")),
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
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
                    is_final: true,
                    is_class_var: false,
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("limit"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: None,
                        value_type: Some(String::from("int")),
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: true,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    metadata: metadata_value(Some("Final")),
                    name: String::from("MAX_SIZE"),
                    kind: DeclarationKind::Value,
                    detail: String::from("Final"),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: true,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    metadata: metadata_class(&[]),
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                Declaration {
                    metadata: metadata_value(None),
                    name: String::from("limit"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Box"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: true,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_marks_classvar_values_and_fields() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/classvars.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("VALUE")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("ClassVar[int]")),
                    value_type: Some(String::from("int")),
                    is_awaited: false,
                    value_callee: None,
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
                    is_final: false,
                    is_class_var: true,
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("cache"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: None,
                        value_type: Some(String::from("int")),
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: true,
                        line: 2,
                    }],
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    metadata: metadata_value(Some("ClassVar[int]")),
                    name: String::from("VALUE"),
                    kind: DeclarationKind::Value,
                    detail: String::from("ClassVar[int]"),
                    value_type: Some(String::from("int")),
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
                    is_class_var: true,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    metadata: metadata_class(&[]),
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
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
                Declaration {
                    metadata: metadata_value(None),
                    name: String::from("cache"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    value_type: Some(String::from("int")),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Box"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: true,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_marks_override_functions_and_members() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/override.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("top_level"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: None,
                    is_async: false,
                    is_override: true,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Child"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(Base)"),
                    bases: vec![String::from("Base")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("run"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: true,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    metadata: metadata_empty_callable(),
                    name: String::from("top_level"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: true,
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
                    metadata: metadata_class(&["Base"]),
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::from("Base"),
                    value_type: None,
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
                    bases: vec![String::from("Base")],
                    type_params: Vec::new(),
                },
                Declaration {
                    metadata: metadata_empty_callable(),
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
                    value_type: None,
                    method_kind: Some(MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Child"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_async: false,
                    is_override: true,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_collects_data_class_declarations_with_owner() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/models.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::DataClass(NamedBlockStatement {
                name: String::from("Point"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("x"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("float")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("y"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("float")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("distance"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: Some(String::from("float")),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    },
                ],
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 4);
        assert_eq!(table.declarations[0].name, "Point");
        assert_eq!(table.declarations[0].class_kind, Some(DeclarationOwnerKind::DataClass));
        assert_eq!(table.declarations[1].name, "x");
        assert_eq!(
            table.declarations[1].owner,
            Some(DeclarationOwner {
                name: String::from("Point"),
                kind: DeclarationOwnerKind::DataClass,
            })
        );
        assert_eq!(table.declarations[2].name, "y");
        assert_eq!(
            table.declarations[2].owner,
            Some(DeclarationOwner {
                name: String::from("Point"),
                kind: DeclarationOwnerKind::DataClass,
            })
        );
        assert_eq!(table.declarations[3].name, "distance");
        assert_eq!(table.declarations[3].kind, DeclarationKind::Function);
        assert_eq!(
            table.declarations[3].owner,
            Some(DeclarationOwner {
                name: String::from("Point"),
                kind: DeclarationOwnerKind::DataClass,
            })
        );
    }

    #[test]
    fn bind_collects_sealed_class_declarations_with_owner() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/models.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::SealedClass(NamedBlockStatement {
                name: String::from("Shape"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("sides"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("int")),
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("area"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: Some(String::from("float")),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                ],
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 3);
        assert_eq!(table.declarations[0].name, "Shape");
        assert_eq!(table.declarations[0].class_kind, Some(DeclarationOwnerKind::SealedClass));
        assert_eq!(
            table.declarations[1].owner,
            Some(DeclarationOwner {
                name: String::from("Shape"),
                kind: DeclarationOwnerKind::SealedClass,
            })
        );
    }

    #[test]
    fn bind_marks_abstract_methods() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/models.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("Readable"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![ClassMember {
                    name: String::from("read"),
                    kind: ClassMemberKind::Method,
                    method_kind: Some(MethodKind::Instance),
                    annotation: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: Some(String::from("bytes")),
                    is_async: false,
                    is_override: false,
                    is_abstract_method: true,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }],
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 2);
        assert!(table.declarations[1].is_abstract_method);
        assert_eq!(table.declarations[1].name, "read");
    }

    #[test]
    fn bind_marks_deprecated_declarations() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/deprecated.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("old_func"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: true,
                    deprecation_message: Some(String::from("use new_func")),
                    line: 1,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("old_func"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: true,
                    deprecation_message: None,
                    line: 2,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 2);
        assert!(table.declarations[0].is_deprecated);
        assert_eq!(table.declarations[0].deprecation_message, Some(String::from("use new_func")));
        assert!(table.declarations[1].is_deprecated);
        assert_eq!(table.declarations[1].deprecation_message, None);
        assert_eq!(table.declarations[1].kind, DeclarationKind::Overload);
    }

    #[test]
    fn bind_marks_final_decorator_on_class() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/finals.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Singleton"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: true,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert!(table.declarations[0].is_final_decorator);
        assert_eq!(table.declarations[0].name, "Singleton");
    }

    #[test]
    fn bind_collects_generic_type_params_with_bounds_and_constraints() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/__init__.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("Sorted"),
                type_params: vec![
                    TypeParam {
                        name: String::from("T"),
                        kind: TypeParamKind::TypeVar,
                        bound: Some(String::from("Comparable")),
                        constraints: Vec::new(),
                        default: None,
                    },
                    TypeParam {
                        name: String::from("U"),
                        kind: TypeParamKind::TypeVar,
                        bound: None,
                        constraints: vec![String::from("int"), String::from("str")],
                        default: None,
                    },
                    TypeParam {
                        name: String::from("V"),
                        kind: TypeParamKind::TypeVar,
                        bound: None,
                        constraints: Vec::new(),
                        default: Some(String::from("str")),
                    },
                ],
                value: String::from("list[T]"),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(
            table.declarations[0].type_params,
            vec![
                GenericTypeParam {
                    name: String::from("T"),
                    kind: GenericTypeParamKind::TypeVar,
                    bound: Some(String::from("Comparable")),
                    constraints: Vec::new(),
                    default: None,
                },
                GenericTypeParam {
                    name: String::from("U"),
                    kind: GenericTypeParamKind::TypeVar,
                    bound: None,
                    constraints: vec![String::from("int"), String::from("str")],
                    default: None,
                },
                GenericTypeParam {
                    name: String::from("V"),
                    kind: GenericTypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: Some(String::from("str")),
                },
            ]
        );
    }

    #[test]
    fn bind_collects_paramspec_type_params() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/__init__.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("decorator"),
                type_params: vec![TypeParam {
                    name: String::from("P"),
                    kind: TypeParamKind::ParamSpec,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                }],
                params: Vec::new(),
                returns: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(
            table.declarations[0].type_params,
            vec![GenericTypeParam {
                name: String::from("P"),
                kind: GenericTypeParamKind::ParamSpec,
                bound: None,
                constraints: Vec::new(),
                default: None,
            }]
        );
    }

    #[test]
    fn bind_collects_typevartuple_type_params() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/__init__.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("Pack"),
                type_params: vec![TypeParam {
                    name: String::from("Ts"),
                    kind: TypeParamKind::TypeVarTuple,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                }],
                value: String::from("tuple[Unpack[Ts]]"),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(
            table.declarations[0].type_params,
            vec![GenericTypeParam {
                name: String::from("Ts"),
                kind: GenericTypeParamKind::TypeVarTuple,
                bound: None,
                constraints: Vec::new(),
                default: None,
            }]
        );
    }

    #[test]
    fn bind_formats_signature_with_positional_only_params() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/funcs.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("func"),
                type_params: Vec::new(),
                params: vec![
                    FunctionParam {
                        name: String::from("x"),
                        annotation: Some(String::from("int")),
                        has_default: false,
                        positional_only: true,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                    FunctionParam {
                        name: String::from("y"),
                        annotation: Some(String::from("int")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                ],
                returns: Some(String::from("int")),
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(table.declarations[0].detail, "(x:int,/,y:int)->int");
    }

    #[test]
    fn bind_formats_signature_with_keyword_only_params() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/funcs.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("func"),
                type_params: Vec::new(),
                params: vec![
                    FunctionParam {
                        name: String::from("x"),
                        annotation: Some(String::from("int")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                    FunctionParam {
                        name: String::from("y"),
                        annotation: Some(String::from("str")),
                        has_default: true,
                        positional_only: false,
                        keyword_only: true,
                        variadic: false,
                        keyword_variadic: false,
                    },
                ],
                returns: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(table.declarations[0].detail, "(x:int,*,y:str=)->");
    }

    #[test]
    fn bind_formats_signature_with_variadic_and_keyword_variadic() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/funcs.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("func"),
                type_params: Vec::new(),
                params: vec![
                    FunctionParam {
                        name: String::from("args"),
                        annotation: Some(String::from("int")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: true,
                        keyword_variadic: false,
                    },
                    FunctionParam {
                        name: String::from("kwargs"),
                        annotation: Some(String::from("str")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: true,
                    },
                ],
                returns: Some(String::from("None")),
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(table.declarations[0].detail, "(*args:int,**kwargs:str)->None");
    }

    #[test]
    fn bind_collects_method_kinds_static_and_class() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/models.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Util"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("create"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Static),
                        annotation: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("from_json"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Class),
                        annotation: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                ],
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 3);
        assert_eq!(table.declarations[1].name, "create");
        assert_eq!(table.declarations[1].method_kind, Some(MethodKind::Static));
        assert_eq!(table.declarations[2].name, "from_json");
        assert_eq!(table.declarations[2].method_kind, Some(MethodKind::Class));
    }

    #[test]
    fn bind_collects_isinstance_guard_with_multiple_types() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/guards.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::If(typepython_syntax::IfStatement {
                owner_name: Some(String::from("check")),
                owner_type_name: None,
                guard: Some(typepython_syntax::GuardCondition::IsInstance {
                    name: String::from("x"),
                    types: vec![String::from("int"), String::from("str")],
                }),
                line: 1,
                true_start_line: 2,
                true_end_line: 2,
                false_start_line: None,
                false_end_line: None,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.if_guards,
            vec![IfGuardSite {
                owner_name: Some(String::from("check")),
                owner_type_name: None,
                guard: Some(GuardConditionSite::IsInstance {
                    name: String::from("x"),
                    types: vec![String::from("int"), String::from("str")],
                }),
                line: 1,
                true_start_line: 2,
                true_end_line: 2,
                false_start_line: None,
                false_end_line: None,
            }]
        );
    }

    #[test]
    fn bind_collects_predicate_call_guard() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/guards.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::If(typepython_syntax::IfStatement {
                owner_name: Some(String::from("validate")),
                owner_type_name: None,
                guard: Some(typepython_syntax::GuardCondition::PredicateCall {
                    name: String::from("x"),
                    callee: String::from("is_valid"),
                }),
                line: 1,
                true_start_line: 2,
                true_end_line: 2,
                false_start_line: None,
                false_end_line: None,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.if_guards,
            vec![IfGuardSite {
                owner_name: Some(String::from("validate")),
                owner_type_name: None,
                guard: Some(GuardConditionSite::PredicateCall {
                    name: String::from("x"),
                    callee: String::from("is_valid"),
                }),
                line: 1,
                true_start_line: 2,
                true_end_line: 2,
                false_start_line: None,
                false_end_line: None,
            }]
        );
    }

    #[test]
    fn bind_collects_composite_and_or_guards() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/guards.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::If(typepython_syntax::IfStatement {
                owner_name: Some(String::from("check")),
                owner_type_name: None,
                guard: Some(typepython_syntax::GuardCondition::And(vec![
                    typepython_syntax::GuardCondition::IsNone {
                        name: String::from("a"),
                        negated: true,
                    },
                    typepython_syntax::GuardCondition::TruthyName { name: String::from("b") },
                ])),
                line: 1,
                true_start_line: 2,
                true_end_line: 2,
                false_start_line: None,
                false_end_line: None,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.if_guards,
            vec![IfGuardSite {
                owner_name: Some(String::from("check")),
                owner_type_name: None,
                guard: Some(GuardConditionSite::And(vec![
                    GuardConditionSite::IsNone { name: String::from("a"), negated: true },
                    GuardConditionSite::TruthyName { name: String::from("b") },
                ])),
                line: 1,
                true_start_line: 2,
                true_end_line: 2,
                false_start_line: None,
                false_end_line: None,
            }]
        );
    }

    #[test]
    fn bind_collects_invalidation_rebind_like() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/invalidate.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Invalidate(
                typepython_syntax::InvalidationStatement {
                    kind: typepython_syntax::InvalidationKind::RebindLike,
                    owner_name: Some(String::from("update")),
                    owner_type_name: None,
                    names: vec![String::from("count")],
                    line: 2,
                },
            )],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.invalidations,
            vec![InvalidationSite {
                kind: InvalidationKind::RebindLike,
                owner_name: Some(String::from("update")),
                owner_type_name: None,
                names: vec![String::from("count")],
                line: 2,
            }]
        );
    }

    #[test]
    fn bind_collects_invalidation_scope_change() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/invalidate.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Invalidate(
                typepython_syntax::InvalidationStatement {
                    kind: typepython_syntax::InvalidationKind::ScopeChange,
                    owner_name: Some(String::from("handler")),
                    owner_type_name: None,
                    names: vec![String::from("state"), String::from("flag")],
                    line: 5,
                },
            )],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.invalidations,
            vec![InvalidationSite {
                kind: InvalidationKind::ScopeChange,
                owner_name: Some(String::from("handler")),
                owner_type_name: None,
                names: vec![String::from("state"), String::from("flag")],
                line: 5,
            }]
        );
    }

    #[test]
    fn bind_excludes_rebind_like_value_from_declarations() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/helpers.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("count")],
                destructuring_target_names: None,
                annotation: None,
                value_type: None,
                is_awaited: false,
                value_callee: None,
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
                value_binop_left: Some(Box::new(DirectExprMetadata {
                    value_type: None,
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("count")),
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
                })),
                value_binop_right: None,
                value_binop_operator: Some(String::from("+")),
                value_lambda: None,
                value_list_comprehension: None,
                value_generator_comprehension: None,
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                owner_name: None,
                owner_type_name: None,
                is_final: false,
                is_class_var: false,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(
            table.declarations.is_empty(),
            "rebind-like update should not appear in declarations"
        );
        assert_eq!(table.assignments.len(), 1);
        assert_eq!(table.assignments[0].name, "count");
        assert_eq!(table.assignments[0].value_binop_operator, Some(String::from("+")));
    }

    #[test]
    fn bind_collects_match_literal_and_unsupported_patterns() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/match.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Match(typepython_syntax::MatchStatement {
                owner_name: Some(String::from("route")),
                owner_type_name: None,
                subject_type: None,
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("code")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![
                    typepython_syntax::MatchCaseStatement {
                        patterns: vec![typepython_syntax::MatchPattern::Literal(String::from("1"))],
                        has_guard: false,
                        line: 2,
                    },
                    typepython_syntax::MatchCaseStatement {
                        patterns: vec![typepython_syntax::MatchPattern::Unsupported],
                        has_guard: false,
                        line: 3,
                    },
                ],
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.matches.len(), 1);
        assert_eq!(
            table.matches[0].cases,
            vec![
                MatchCaseSite {
                    patterns: vec![MatchPatternSite::Literal(String::from("1"))],
                    has_guard: false,
                    line: 2,
                },
                MatchCaseSite {
                    patterns: vec![MatchPatternSite::Unsupported],
                    has_guard: false,
                    line: 3,
                },
            ]
        );
    }

    #[test]
    fn bind_collects_class_with_multiple_bases() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/models.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Widget"),
                type_params: Vec::new(),
                header_suffix: String::from("(Base1, Base2, Mixin)"),
                bases: vec![String::from("Base1"), String::from("Base2"), String::from("Mixin")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.declarations.len(), 1);
        assert_eq!(
            table.declarations[0].bases,
            vec![String::from("Base1"), String::from("Base2"), String::from("Mixin"),]
        );
        assert_eq!(table.declarations[0].detail, "Base1,Base2,Mixin");
    }

    #[test]
    fn bind_collects_yield_from_sites() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/gen.py"),
                kind: SourceKind::Python,
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Yield(typepython_syntax::YieldStatement {
                owner_name: String::from("delegate"),
                owner_type_name: None,
                value_type: Some(String::from("list[int]")),
                value_callee: None,
                value_name: Some(String::from("items")),
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
                value_list_elements: None,
                value_set_elements: None,
                value_dict_entries: None,
                is_yield_from: true,
                line: 2,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(table.yields.len(), 1);
        assert!(table.yields[0].is_yield_from);
        assert_eq!(table.yields[0].owner_name, "delegate");
        assert_eq!(table.yields[0].value_name, Some(String::from("items")));
    }
}
