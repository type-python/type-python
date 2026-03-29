//! Symbol binding boundary for TypePython.

use std::path::PathBuf;

use typepython_syntax::{MethodKind, SourceKind, SyntaxStatement, SyntaxTree};

#[derive(Debug, Clone)]
pub struct BindingTable {
    pub module_path: PathBuf,
    pub module_key: String,
    pub module_kind: SourceKind,
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MemberAccessSite {
    pub owner_name: String,
    pub member: String,
    pub through_instance: bool,
}

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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ReturnSite {
    pub owner_name: String,
    pub owner_type_name: Option<String>,
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct YieldSite {
    pub owner_name: String,
    pub owner_type_name: Option<String>,
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AssertGuardSite {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub guard: Option<GuardConditionSite>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum InvalidationKind {
    RebindLike,
    Delete,
    ScopeChange,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct InvalidationSite {
    pub kind: InvalidationKind,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub names: Vec<String>,
    pub line: usize,
}

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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MatchSite {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MatchCaseSite {
    pub patterns: Vec<MatchPatternSite>,
    pub has_guard: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum MatchPatternSite {
    Wildcard,
    Literal(String),
    Class(String),
    Unsupported,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ForSite {
    pub target_name: String,
    pub target_names: Vec<String>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WithSite {
    pub target_name: Option<String>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ExceptHandlerSite {
    pub exception_type: String,
    pub binding_name: Option<String>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AssignmentSite {
    pub name: String,
    pub destructuring_target_names: Option<Vec<String>>,
    pub destructuring_index: Option<usize>,
    pub annotation: Option<String>,
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Declaration {
    pub name: String,
    pub kind: DeclarationKind,
    pub detail: String,
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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum GenericTypeParamKind {
    TypeVar,
    ParamSpec,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct GenericTypeParam {
    pub kind: GenericTypeParamKind,
    pub name: String,
    pub bound: Option<String>,
    pub constraints: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DeclarationOwner {
    pub name: String,
    pub kind: DeclarationOwnerKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum DeclarationOwnerKind {
    Class,
    Interface,
    DataClass,
    SealedClass,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum DeclarationKind {
    TypeAlias,
    Class,
    Function,
    Overload,
    Value,
    Import,
}

#[must_use]
pub fn bind(tree: &SyntaxTree) -> BindingTable {
    BindingTable {
        module_path: tree.source.path.clone(),
        module_key: tree.source.logical_module.clone(),
        module_kind: tree.source.kind,
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
                    owner_name: statement.owner_name.clone(),
                    member: statement.member.clone(),
                    through_instance: statement.through_instance,
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

fn bind_statement(statement: &SyntaxStatement) -> Vec<Declaration> {
    match statement {
        SyntaxStatement::TypeAlias(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::TypeAlias,
            detail: statement.value.clone(),
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
            detail: format_signature(&statement.params, statement.returns.as_deref()),
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
            detail: format_signature(&statement.params, statement.returns.as_deref()),
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

fn format_signature(params: &[typepython_syntax::FunctionParam], returns: Option<&str>) -> String {
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

fn bind_type_params(type_params: &[typepython_syntax::TypeParam]) -> Vec<GenericTypeParam> {
    type_params
        .iter()
        .map(|param| GenericTypeParam {
            kind: match param.kind {
                typepython_syntax::TypeParamKind::TypeVar => GenericTypeParamKind::TypeVar,
                typepython_syntax::TypeParamKind::ParamSpec => GenericTypeParamKind::ParamSpec,
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
        AssertGuardSite, AssignmentSite, Declaration, DeclarationKind, DeclarationOwner,
        DeclarationOwnerKind, ExceptHandlerSite, ForSite, GenericTypeParam, GenericTypeParamKind,
        GuardConditionSite, IfGuardSite, InvalidationKind, InvalidationSite, MatchCaseSite,
        MatchPatternSite, MatchSite, WithSite, YieldSite, bind,
    };
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{
        ClassMember, ClassMemberKind, FunctionStatement, ImportStatement, MethodKind,
        NamedBlockStatement, SourceFile, SourceKind, SyntaxStatement, SyntaxTree,
        TypeAliasStatement, TypeParam, TypeParamKind, ValueStatement,
    };

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
}
