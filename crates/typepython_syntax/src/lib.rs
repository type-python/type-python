//! Source classification and parser boundary for TypePython.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use ruff_python_ast::{visitor, visitor::Visitor, Expr, Stmt, TypeParam as AstTypeParam};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};

/// Supported input file kinds from the spec.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SourceKind {
    /// `.tpy` TypePython source.
    TypePython,
    /// `.py` pass-through Python.
    Python,
    /// `.pyi` stub input.
    Stub,
}

impl SourceKind {
    /// Infers the source kind from a path suffix.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(OsStr::to_str) {
            Some("tpy") => Some(Self::TypePython),
            Some("py") => Some(Self::Python),
            Some("pyi") => Some(Self::Stub),
            _ => None,
        }
    }
}

/// An in-memory source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Filesystem location.
    pub path: PathBuf,
    /// Classified input kind.
    pub kind: SourceKind,
    pub logical_module: String,
    /// Source text.
    pub text: String,
}

impl SourceFile {
    /// Reads a source file from disk.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let path = path.as_ref().to_path_buf();
        let text = fs::read_to_string(&path)?;
        let kind = SourceKind::from_path(&path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported source suffix for {}", path.display()),
            )
        })?;

        Ok(Self { path, kind, logical_module: String::new(), text })
    }
}

/// Parser output for a source file.
#[derive(Debug, Clone)]
pub struct SyntaxTree {
    /// Original source file.
    pub source: SourceFile,
    pub statements: Vec<SyntaxStatement>,
    pub type_ignore_directives: Vec<TypeIgnoreDirective>,
    pub diagnostics: DiagnosticReport,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeIgnoreDirective {
    pub line: usize,
    pub codes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SyntaxStatement {
    TypeAlias(TypeAliasStatement),
    Interface(NamedBlockStatement),
    DataClass(NamedBlockStatement),
    SealedClass(NamedBlockStatement),
    OverloadDef(FunctionStatement),
    ClassDef(NamedBlockStatement),
    FunctionDef(FunctionStatement),
    Import(ImportStatement),
    Value(ValueStatement),
    Call(CallStatement),
    MemberAccess(MemberAccessStatement),
    MethodCall(MethodCallStatement),
    Return(ReturnStatement),
    Yield(YieldStatement),
    If(IfStatement),
    Assert(AssertStatement),
    Invalidate(InvalidationStatement),
    Match(MatchStatement),
    For(ForStatement),
    With(WithStatement),
    ExceptHandler(ExceptionHandlerStatement),
    Unsafe(UnsafeStatement),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeAliasStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NamedBlockStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub header_suffix: String,
    pub bases: Vec<String>,
    pub is_final_decorator: bool,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub is_abstract_class: bool,
    pub members: Vec<ClassMember>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FunctionStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
    pub is_async: bool,
    pub is_override: bool,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FunctionParam {
    pub name: String,
    pub annotation: Option<String>,
    pub has_default: bool,
    pub positional_only: bool,
    pub keyword_only: bool,
    pub variadic: bool,
    pub keyword_variadic: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ImportStatement {
    pub bindings: Vec<ImportBinding>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ImportBinding {
    pub local_name: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValueStatement {
    pub names: Vec<String>,
    pub destructuring_target_names: Option<Vec<String>>,
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
    pub value_subscript_target: Option<Box<DirectExprMetadata>>,
    pub value_subscript_string_key: Option<String>,
    pub value_subscript_index: Option<String>,
    pub value_if_true: Option<Box<DirectExprMetadata>>,
    pub value_if_false: Option<Box<DirectExprMetadata>>,
    pub value_if_guard: Option<GuardCondition>,
    pub value_bool_left: Option<Box<DirectExprMetadata>>,
    pub value_bool_right: Option<Box<DirectExprMetadata>>,
    pub value_binop_left: Option<Box<DirectExprMetadata>>,
    pub value_binop_right: Option<Box<DirectExprMetadata>>,
    pub value_binop_operator: Option<String>,
    pub value_lambda: Option<Box<LambdaMetadata>>,
    pub value_list_comprehension: Option<Box<ComprehensionMetadata>>,
    pub value_generator_comprehension: Option<Box<ComprehensionMetadata>>,
    pub value_list_elements: Option<Vec<DirectExprMetadata>>,
    pub value_set_elements: Option<Vec<DirectExprMetadata>>,
    pub value_dict_entries: Option<Vec<TypedDictLiteralEntry>>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub is_final: bool,
    pub is_class_var: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CallStatement {
    pub callee: String,
    pub arg_count: usize,
    pub arg_types: Vec<String>,
    pub arg_values: Vec<DirectExprMetadata>,
    pub starred_arg_types: Vec<String>,
    pub starred_arg_values: Vec<DirectExprMetadata>,
    pub keyword_names: Vec<String>,
    pub keyword_arg_types: Vec<String>,
    pub keyword_arg_values: Vec<DirectExprMetadata>,
    pub keyword_expansion_types: Vec<String>,
    pub keyword_expansion_values: Vec<DirectExprMetadata>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemberAccessStatement {
    pub owner_name: String,
    pub member: String,
    pub through_instance: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MethodCallStatement {
    pub owner_name: String,
    pub method: String,
    pub through_instance: bool,
    pub arg_count: usize,
    pub arg_types: Vec<String>,
    pub arg_values: Vec<DirectExprMetadata>,
    pub starred_arg_types: Vec<String>,
    pub starred_arg_values: Vec<DirectExprMetadata>,
    pub keyword_names: Vec<String>,
    pub keyword_arg_types: Vec<String>,
    pub keyword_arg_values: Vec<DirectExprMetadata>,
    pub keyword_expansion_types: Vec<String>,
    pub keyword_expansion_values: Vec<DirectExprMetadata>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ReturnStatement {
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
    pub value_subscript_target: Option<Box<DirectExprMetadata>>,
    pub value_subscript_string_key: Option<String>,
    pub value_subscript_index: Option<String>,
    pub value_if_true: Option<Box<DirectExprMetadata>>,
    pub value_if_false: Option<Box<DirectExprMetadata>>,
    pub value_if_guard: Option<GuardCondition>,
    pub value_bool_left: Option<Box<DirectExprMetadata>>,
    pub value_bool_right: Option<Box<DirectExprMetadata>>,
    pub value_binop_left: Option<Box<DirectExprMetadata>>,
    pub value_binop_right: Option<Box<DirectExprMetadata>>,
    pub value_binop_operator: Option<String>,
    pub value_lambda: Option<Box<LambdaMetadata>>,
    pub value_list_elements: Option<Vec<DirectExprMetadata>>,
    pub value_set_elements: Option<Vec<DirectExprMetadata>>,
    pub value_dict_entries: Option<Vec<TypedDictLiteralEntry>>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct YieldStatement {
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
    pub value_subscript_target: Option<Box<DirectExprMetadata>>,
    pub value_subscript_string_key: Option<String>,
    pub value_subscript_index: Option<String>,
    pub value_if_true: Option<Box<DirectExprMetadata>>,
    pub value_if_false: Option<Box<DirectExprMetadata>>,
    pub value_if_guard: Option<GuardCondition>,
    pub value_bool_left: Option<Box<DirectExprMetadata>>,
    pub value_bool_right: Option<Box<DirectExprMetadata>>,
    pub value_binop_left: Option<Box<DirectExprMetadata>>,
    pub value_binop_right: Option<Box<DirectExprMetadata>>,
    pub value_binop_operator: Option<String>,
    pub value_lambda: Option<Box<LambdaMetadata>>,
    pub value_list_elements: Option<Vec<DirectExprMetadata>>,
    pub value_set_elements: Option<Vec<DirectExprMetadata>>,
    pub value_dict_entries: Option<Vec<TypedDictLiteralEntry>>,
    pub is_yield_from: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct IfStatement {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub guard: Option<GuardCondition>,
    pub line: usize,
    pub true_start_line: usize,
    pub true_end_line: usize,
    pub false_start_line: Option<usize>,
    pub false_end_line: Option<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AssertStatement {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub guard: Option<GuardCondition>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InvalidationStatement {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub names: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum GuardCondition {
    IsNone { name: String, negated: bool },
    IsInstance { name: String, types: Vec<String> },
    PredicateCall { name: String, callee: String },
    TruthyName { name: String },
    Not(Box<GuardCondition>),
    And(Vec<GuardCondition>),
    Or(Vec<GuardCondition>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MatchStatement {
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
    pub cases: Vec<MatchCaseStatement>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MatchCaseStatement {
    pub patterns: Vec<MatchPattern>,
    pub has_guard: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MatchPattern {
    Wildcard,
    Literal(String),
    Class(String),
    Unsupported,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ForStatement {
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WithStatement {
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExceptionHandlerStatement {
    pub exception_type: String,
    pub binding_name: Option<String>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ClassMember {
    pub name: String,
    pub kind: ClassMemberKind,
    pub method_kind: Option<MethodKind>,
    pub annotation: Option<String>,
    pub value_type: Option<String>,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
    pub is_async: bool,
    pub is_override: bool,
    pub is_abstract_method: bool,
    pub is_final_decorator: bool,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub is_final: bool,
    pub is_class_var: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClassMemberKind {
    Field,
    Method,
    Overload,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum MethodKind {
    Instance,
    Class,
    Static,
    Property,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UnsafeStatement {
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeParam {
    pub name: String,
    pub bound: Option<String>,
}

struct ParsedTypeParams<'source> {
    type_params: Vec<TypeParam>,
    remainder: &'source str,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct LambdaMetadata {
    pub param_names: Vec<String>,
    pub body: Box<DirectExprMetadata>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ComprehensionClauseMetadata {
    pub target_name: String,
    pub target_names: Vec<String>,
    pub iter: Box<DirectExprMetadata>,
    pub filters: Vec<GuardCondition>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ComprehensionMetadata {
    pub kind: ComprehensionKind,
    pub clauses: Vec<ComprehensionClauseMetadata>,
    pub key: Option<Box<DirectExprMetadata>>,
    pub element: Box<DirectExprMetadata>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ComprehensionKind {
    List,
    Set,
    Dict,
    Generator,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DirectExprMetadata {
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
    pub value_subscript_target: Option<Box<DirectExprMetadata>>,
    pub value_subscript_string_key: Option<String>,
    pub value_subscript_index: Option<String>,
    pub value_if_true: Option<Box<DirectExprMetadata>>,
    pub value_if_false: Option<Box<DirectExprMetadata>>,
    pub value_if_guard: Option<GuardCondition>,
    pub value_bool_left: Option<Box<DirectExprMetadata>>,
    pub value_bool_right: Option<Box<DirectExprMetadata>>,
    pub value_binop_left: Option<Box<DirectExprMetadata>>,
    pub value_binop_right: Option<Box<DirectExprMetadata>>,
    pub value_binop_operator: Option<String>,
    pub value_lambda: Option<Box<LambdaMetadata>>,
    pub value_list_comprehension: Option<Box<ComprehensionMetadata>>,
    pub value_generator_comprehension: Option<Box<ComprehensionMetadata>>,
    pub value_list_elements: Option<Vec<DirectExprMetadata>>,
    pub value_set_elements: Option<Vec<DirectExprMetadata>>,
    pub value_dict_entries: Option<Vec<TypedDictLiteralEntry>>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TypedDictLiteralEntry {
    pub key: Option<String>,
    pub key_value: Option<Box<DirectExprMetadata>>,
    pub is_expansion: bool,
    pub value: DirectExprMetadata,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypedDictLiteralSite {
    pub annotation: String,
    pub entries: Vec<TypedDictLiteralEntry>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DirectCallContextSite {
    pub callee: String,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TypedDictMutationKind {
    Assignment,
    AugmentedAssignment,
    Delete,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypedDictMutationSite {
    pub kind: TypedDictMutationKind,
    pub key: Option<String>,
    pub target: DirectExprMetadata,
    pub value: Option<DirectExprMetadata>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConditionalReturnSite {
    pub function_name: String,
    pub target_name: String,
    pub target_type: String,
    pub case_input_types: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct DataclassTransformMetadata {
    pub field_specifiers: Vec<String>,
    pub kw_only_default: bool,
    pub frozen_default: bool,
    pub eq_default: bool,
    pub order_default: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DataclassTransformProviderSite {
    pub name: String,
    pub metadata: DataclassTransformMetadata,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DataclassTransformFieldSite {
    pub name: String,
    pub annotation: String,
    pub value_type: Option<String>,
    pub value_metadata: Option<DirectExprMetadata>,
    pub has_default: bool,
    pub is_class_var: bool,
    pub field_specifier_name: Option<String>,
    pub field_specifier_has_default: bool,
    pub field_specifier_has_default_factory: bool,
    pub field_specifier_init: Option<bool>,
    pub field_specifier_kw_only: Option<bool>,
    pub field_specifier_alias: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DataclassTransformClassSite {
    pub name: String,
    pub decorators: Vec<String>,
    pub plain_dataclass_frozen: bool,
    pub plain_dataclass_kw_only: bool,
    pub plain_dataclass_init: bool,
    pub bases: Vec<String>,
    pub metaclass: Option<String>,
    pub methods: Vec<String>,
    pub fields: Vec<DataclassTransformFieldSite>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct DataclassTransformModuleInfo {
    pub providers: Vec<DataclassTransformProviderSite>,
    pub classes: Vec<DataclassTransformClassSite>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DirectFunctionParamSite {
    pub name: String,
    pub annotation: Option<String>,
    pub has_default: bool,
    pub positional_only: bool,
    pub keyword_only: bool,
    pub variadic: bool,
    pub keyword_variadic: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DirectFunctionSignatureSite {
    pub name: String,
    pub params: Vec<DirectFunctionParamSite>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DirectMethodSignatureSite {
    pub owner_type_name: String,
    pub name: String,
    pub method_kind: MethodKind,
    pub params: Vec<DirectFunctionParamSite>,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FrozenFieldMutationKind {
    Assignment,
    AugmentedAssignment,
    Delete,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FrozenFieldMutationSite {
    pub kind: FrozenFieldMutationKind,
    pub field_name: String,
    pub target: DirectExprMetadata,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum UnsafeOperationKind {
    EvalCall,
    ExecCall,
    GlobalsWrite,
    LocalsWrite,
    DictWrite,
    SetAttrNonLiteral,
    DelAttrNonLiteral,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UnsafeOperationSite {
    pub kind: UnsafeOperationKind,
    pub line: usize,
    pub in_unsafe_block: bool,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct ParseOptions {
    pub enable_conditional_returns: bool,
}

/// Parses a source file into a syntax tree.
#[must_use]
pub fn parse(source: SourceFile) -> SyntaxTree {
    parse_with_options(source, ParseOptions::default())
}

#[must_use]
pub fn parse_with_options(source: SourceFile, options: ParseOptions) -> SyntaxTree {
    match source.kind {
        SourceKind::TypePython => parse_typepython_source(source, options),
        SourceKind::Python | SourceKind::Stub => parse_python_source(source),
    }
}

#[must_use]
pub fn collect_typed_dict_literal_sites(source: &str) -> Vec<TypedDictLiteralSite> {
    let Ok(parsed) = parse_module(source) else {
        return Vec::new();
    };

    let mut sites = Vec::new();
    collect_typed_dict_literal_sites_in_suite(source, parsed.suite(), None, None, &mut sites);
    sites
}

#[must_use]
pub fn collect_direct_call_context_sites(source: &str) -> Vec<DirectCallContextSite> {
    let Ok(parsed) = parse_module(source) else {
        return Vec::new();
    };

    let mut sites = Vec::new();
    collect_direct_call_context_sites_in_suite(source, parsed.suite(), None, None, &mut sites);
    sites
}

#[must_use]
pub fn collect_typed_dict_mutation_sites(source: &str) -> Vec<TypedDictMutationSite> {
    let Ok(parsed) = parse_module(source) else {
        return Vec::new();
    };

    let mut sites = Vec::new();
    collect_typed_dict_mutation_sites_in_suite(source, parsed.suite(), None, None, &mut sites);
    sites
}

#[must_use]
pub fn collect_unsafe_operation_sites(source: &str) -> Vec<UnsafeOperationSite> {
    let tree = parse(SourceFile {
        path: PathBuf::from("<unsafe>.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: source.to_owned(),
    });
    let normalized = normalize_typepython_source(source, &tree.statements);
    let Ok(parsed) = parse_module(&normalized) else {
        return Vec::new();
    };

    let unsafe_ranges = collect_unsafe_block_ranges(source, &tree.statements);
    let mut collector =
        UnsafeOperationCollector { source: &normalized, unsafe_ranges, sites: Vec::new() };
    for stmt in parsed.suite() {
        visitor::Visitor::visit_stmt(&mut collector, stmt);
    }
    collector.sites
}

#[must_use]
pub fn collect_conditional_return_sites(source: &str) -> Vec<ConditionalReturnSite> {
    conditional_return_blocks(source)
        .into_iter()
        .filter_map(|block| {
            let params = block.header.split_once('(')?.1.rsplit_once(')')?.0;
            let target_type = parameter_annotation(params, &block.target_name)?;
            Some(ConditionalReturnSite {
                function_name: block.function_name,
                target_name: block.target_name,
                target_type,
                case_input_types: block.case_input_types,
                line: block.line,
            })
        })
        .collect()
}

#[must_use]
pub fn collect_dataclass_transform_module_info(source: &str) -> DataclassTransformModuleInfo {
    let Ok(parsed) = parse_module(source) else {
        return DataclassTransformModuleInfo::default();
    };

    let import_bindings = collect_import_bindings(parsed.suite());
    let mut providers = Vec::new();
    let mut classes = Vec::new();
    for stmt in parsed.suite() {
        match stmt {
            Stmt::FunctionDef(function) => {
                if let Some(metadata) =
                    dataclass_transform_metadata(source, &function.decorator_list, &import_bindings)
                {
                    providers.push(DataclassTransformProviderSite {
                        name: function.name.as_str().to_owned(),
                        metadata,
                        line: offset_to_line_column(source, function.range.start().to_usize()).0,
                    });
                }
            }
            Stmt::ClassDef(class_def) => {
                if let Some(metadata) = dataclass_transform_metadata(
                    source,
                    &class_def.decorator_list,
                    &import_bindings,
                ) {
                    providers.push(DataclassTransformProviderSite {
                        name: class_def.name.as_str().to_owned(),
                        metadata,
                        line: offset_to_line_column(source, class_def.range.start().to_usize()).0,
                    });
                }
                classes.push(collect_dataclass_transform_class_site(
                    source,
                    class_def,
                    &import_bindings,
                ));
            }
            _ => {}
        }
    }

    DataclassTransformModuleInfo { providers, classes }
}

#[must_use]
pub fn collect_direct_function_signature_sites(source: &str) -> Vec<DirectFunctionSignatureSite> {
    let Ok(parsed) = parse_module(source) else {
        return Vec::new();
    };

    parsed
        .suite()
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => Some(DirectFunctionSignatureSite {
                name: function.name.as_str().to_owned(),
                params: collect_direct_function_param_sites(source, &function.parameters),
                line: offset_to_line_column(source, function.range.start().to_usize()).0,
            }),
            _ => None,
        })
        .collect()
}

#[must_use]
pub fn collect_direct_method_signature_sites(source: &str) -> Vec<DirectMethodSignatureSite> {
    let Ok(parsed) = parse_module(source) else {
        return Vec::new();
    };

    parsed
        .suite()
        .iter()
        .flat_map(|stmt| match stmt {
            Stmt::ClassDef(class_def) => class_def
                .body
                .iter()
                .filter_map(|member| match member {
                    Stmt::FunctionDef(function) => Some(DirectMethodSignatureSite {
                        owner_type_name: class_def.name.as_str().to_owned(),
                        name: function.name.as_str().to_owned(),
                        method_kind: method_kind_from_decorators(&function.decorator_list),
                        params: collect_direct_function_param_sites(source, &function.parameters),
                        line: offset_to_line_column(source, function.range.start().to_usize()).0,
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

fn collect_direct_function_param_sites(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
) -> Vec<DirectFunctionParamSite> {
    let positional_only = parameters.posonlyargs.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: parameter.default().is_some(),
        positional_only: true,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    });
    let positional = parameters.args.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: parameter.default().is_some(),
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    });
    let variadic = parameters.vararg.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: true,
        keyword_variadic: false,
    });
    let keyword_only = parameters.kwonlyargs.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: parameter.default().is_some(),
        positional_only: false,
        keyword_only: true,
        variadic: false,
        keyword_variadic: false,
    });
    let keyword_variadic = parameters.kwarg.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: true,
    });

    positional_only
        .chain(positional)
        .chain(variadic)
        .chain(keyword_only)
        .chain(keyword_variadic)
        .collect()
}

#[must_use]
pub fn collect_frozen_field_mutation_sites(source: &str) -> Vec<FrozenFieldMutationSite> {
    let Ok(parsed) = parse_module(source) else {
        return Vec::new();
    };

    let mut sites = Vec::new();
    collect_frozen_field_mutation_sites_in_suite(source, parsed.suite(), None, None, &mut sites);
    sites
}

fn collect_frozen_field_mutation_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<FrozenFieldMutationSite>,
) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        sites.extend(extract_frozen_field_mutation_sites_from_stmt(
            source,
            stmt,
            line,
            owner_name,
            owner_type_name,
        ));

        match stmt {
            Stmt::FunctionDef(function) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_frozen_field_mutation_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_frozen_field_mutation_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_frozen_field_mutation_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            _ => {}
        }
    }
}

fn extract_frozen_field_mutation_sites_from_stmt(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Vec<FrozenFieldMutationSite> {
    match stmt {
        Stmt::Assign(assign) => assign
            .targets
            .iter()
            .filter_map(|target| {
                extract_frozen_field_mutation_site(
                    source,
                    target,
                    FrozenFieldMutationKind::Assignment,
                    line,
                    owner_name,
                    owner_type_name,
                )
            })
            .collect(),
        Stmt::AugAssign(assign) => extract_frozen_field_mutation_site(
            source,
            &assign.target,
            FrozenFieldMutationKind::AugmentedAssignment,
            line,
            owner_name,
            owner_type_name,
        )
        .into_iter()
        .collect(),
        Stmt::Delete(delete) => delete
            .targets
            .iter()
            .filter_map(|target| {
                extract_frozen_field_mutation_site(
                    source,
                    target,
                    FrozenFieldMutationKind::Delete,
                    line,
                    owner_name,
                    owner_type_name,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_frozen_field_mutation_site(
    source: &str,
    expr: &Expr,
    kind: FrozenFieldMutationKind,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<FrozenFieldMutationSite> {
    let Expr::Attribute(attribute) = expr else {
        return None;
    };
    Some(FrozenFieldMutationSite {
        kind,
        field_name: attribute.attr.as_str().to_owned(),
        target: extract_direct_expr_metadata(source, &attribute.value),
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        line,
    })
}

fn collect_dataclass_transform_class_site(
    source: &str,
    class_def: &ruff_python_ast::StmtClassDef,
    import_bindings: &BTreeMap<String, String>,
) -> DataclassTransformClassSite {
    let plain_dataclass = dataclass_decorator_metadata(&class_def.decorator_list, import_bindings);
    DataclassTransformClassSite {
        name: class_def.name.as_str().to_owned(),
        decorators: class_def
            .decorator_list
            .iter()
            .filter_map(|decorator| decorator_target_name(&decorator.expression))
            .map(|name| normalize_imported_name(&name, import_bindings))
            .collect(),
        plain_dataclass_frozen: plain_dataclass.as_ref().is_some_and(|metadata| metadata.frozen),
        plain_dataclass_kw_only: plain_dataclass.as_ref().is_some_and(|metadata| metadata.kw_only),
        plain_dataclass_init: plain_dataclass
            .as_ref()
            .map(|metadata| metadata.init)
            .unwrap_or(true),
        bases: class_def
            .arguments
            .as_ref()
            .map(|arguments| {
                arguments
                    .args
                    .iter()
                    .filter_map(decorator_target_name)
                    .map(|name| normalize_imported_name(&name, import_bindings))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        metaclass: class_def.arguments.as_ref().and_then(|arguments| {
            arguments.keywords.iter().find_map(|keyword| {
                (keyword.arg.as_ref().map(|arg| arg.as_str()) == Some("metaclass"))
                    .then(|| decorator_target_name(&keyword.value))
                    .flatten()
                    .map(|name| normalize_imported_name(&name, import_bindings))
            })
        }),
        methods: class_def
            .body
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::FunctionDef(function) => Some(function.name.as_str().to_owned()),
                _ => None,
            })
            .collect(),
        fields: class_def
            .body
            .iter()
            .filter_map(|stmt| extract_dataclass_transform_field(source, stmt, import_bindings))
            .collect(),
        line: offset_to_line_column(source, class_def.range.start().to_usize()).0,
    }
}

fn extract_dataclass_transform_field(
    source: &str,
    stmt: &Stmt,
    import_bindings: &BTreeMap<String, String>,
) -> Option<DataclassTransformFieldSite> {
    let Stmt::AnnAssign(assign) = stmt else {
        return None;
    };
    let Expr::Name(name) = assign.target.as_ref() else {
        return None;
    };
    let value = assign.value.as_deref();
    let field_specifier =
        value.and_then(|expr| extract_field_specifier_site(source, expr, import_bindings));
    Some(DataclassTransformFieldSite {
        name: name.id.as_str().to_owned(),
        annotation: slice_range(source, assign.annotation.range())?.to_owned(),
        value_type: value.map(infer_literal_arg_type),
        value_metadata: value.map(|expr| extract_direct_expr_metadata(source, expr)),
        has_default: value.is_some(),
        is_class_var: is_classvar_annotation(&assign.annotation),
        field_specifier_name: field_specifier.as_ref().and_then(|site| site.name.clone()),
        field_specifier_has_default: field_specifier.as_ref().is_some_and(|site| site.has_default),
        field_specifier_has_default_factory: field_specifier
            .as_ref()
            .is_some_and(|site| site.has_default_factory),
        field_specifier_init: field_specifier.as_ref().and_then(|site| site.init),
        field_specifier_kw_only: field_specifier.as_ref().and_then(|site| site.kw_only),
        field_specifier_alias: field_specifier.as_ref().and_then(|site| site.alias.clone()),
        line: offset_to_line_column(source, assign.range.start().to_usize()).0,
    })
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct FieldSpecifierSite {
    name: Option<String>,
    has_default: bool,
    has_default_factory: bool,
    init: Option<bool>,
    kw_only: Option<bool>,
    alias: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct DataclassDecoratorMetadata {
    frozen: bool,
    kw_only: bool,
    init: bool,
}

fn extract_field_specifier_site(
    source: &str,
    expr: &Expr,
    import_bindings: &BTreeMap<String, String>,
) -> Option<FieldSpecifierSite> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let mut result = FieldSpecifierSite {
        name: decorator_target_name(call.func.as_ref())
            .map(|name| normalize_imported_name(&name, import_bindings)),
        has_default: false,
        has_default_factory: false,
        init: None,
        kw_only: None,
        alias: None,
    };
    for keyword in &call.arguments.keywords {
        let Some(name) = keyword.arg.as_ref().map(|name| name.as_str()) else {
            continue;
        };
        match name {
            "default" => result.has_default = true,
            "default_factory" => result.has_default_factory = true,
            "init" => result.init = expr_static_bool(&keyword.value),
            "kw_only" => result.kw_only = expr_static_bool(&keyword.value),
            "alias" => result.alias = extract_string_literal_value(source, &keyword.value),
            _ => {}
        }
    }
    Some(result)
}

fn dataclass_transform_metadata(
    source: &str,
    decorators: &[ruff_python_ast::Decorator],
    import_bindings: &BTreeMap<String, String>,
) -> Option<DataclassTransformMetadata> {
    decorators.iter().find_map(|decorator| {
        let expression = &decorator.expression;
        if is_dataclass_transform_expr(expression, import_bindings) {
            return Some(dataclass_transform_metadata_from_call(
                source,
                expression,
                import_bindings,
            ));
        }
        None
    })
}

fn dataclass_decorator_metadata(
    decorators: &[ruff_python_ast::Decorator],
    import_bindings: &BTreeMap<String, String>,
) -> Option<DataclassDecoratorMetadata> {
    decorators.iter().find_map(|decorator| {
        let expression = &decorator.expression;
        if !is_dataclass_expr(expression, import_bindings) {
            return None;
        }
        Some(dataclass_decorator_metadata_from_expr(expression))
    })
}

fn dataclass_decorator_metadata_from_expr(expr: &Expr) -> DataclassDecoratorMetadata {
    let Expr::Call(call) = expr else {
        return DataclassDecoratorMetadata { frozen: false, kw_only: false, init: true };
    };
    let mut metadata = DataclassDecoratorMetadata { frozen: false, kw_only: false, init: true };
    for keyword in &call.arguments.keywords {
        let Some(name) = keyword.arg.as_ref().map(|name| name.as_str()) else {
            continue;
        };
        match name {
            "frozen" => metadata.frozen = expr_static_bool(&keyword.value).unwrap_or(false),
            "kw_only" => metadata.kw_only = expr_static_bool(&keyword.value).unwrap_or(false),
            "init" => metadata.init = expr_static_bool(&keyword.value).unwrap_or(true),
            _ => {}
        }
    }
    metadata
}

fn is_dataclass_expr(expr: &Expr, import_bindings: &BTreeMap<String, String>) -> bool {
    decorator_target_name(expr)
        .map(|name| normalize_imported_name(&name, import_bindings))
        .is_some_and(|name| matches!(name.as_str(), "dataclass" | "dataclasses.dataclass"))
}

fn dataclass_transform_metadata_from_call(
    source: &str,
    expr: &Expr,
    import_bindings: &BTreeMap<String, String>,
) -> DataclassTransformMetadata {
    let Expr::Call(call) = expr else {
        return DataclassTransformMetadata::default();
    };
    let mut metadata =
        DataclassTransformMetadata { eq_default: true, ..DataclassTransformMetadata::default() };
    for keyword in &call.arguments.keywords {
        let Some(name) = keyword.arg.as_ref().map(|name| name.as_str()) else {
            continue;
        };
        match name {
            "kw_only_default" => {
                metadata.kw_only_default = expr_static_bool(&keyword.value).unwrap_or(false)
            }
            "frozen_default" => {
                metadata.frozen_default = expr_static_bool(&keyword.value).unwrap_or(false)
            }
            "eq_default" => metadata.eq_default = expr_static_bool(&keyword.value).unwrap_or(true),
            "order_default" => {
                metadata.order_default = expr_static_bool(&keyword.value).unwrap_or(false)
            }
            "field_specifiers" => {
                metadata.field_specifiers = expr_name_list(&keyword.value, source, import_bindings);
            }
            _ => {}
        }
    }
    metadata
}

fn is_dataclass_transform_expr(expr: &Expr, import_bindings: &BTreeMap<String, String>) -> bool {
    decorator_target_name(expr)
        .map(|name| normalize_imported_name(&name, import_bindings))
        .is_some_and(|name| {
            matches!(
                name.as_str(),
                "dataclass_transform"
                    | "typing.dataclass_transform"
                    | "typing_extensions.dataclass_transform"
            )
        })
}

fn decorator_target_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str().to_owned()),
        Expr::Attribute(attribute) => Some(format!(
            "{}.{}",
            decorator_target_name(attribute.value.as_ref())?,
            attribute.attr.as_str()
        )),
        Expr::Call(call) => decorator_target_name(call.func.as_ref()),
        _ => None,
    }
}

fn expr_static_bool(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::BooleanLiteral(boolean) => Some(boolean.value),
        Expr::Name(name) if name.id.as_str() == "True" => Some(true),
        Expr::Name(name) if name.id.as_str() == "False" => Some(false),
        _ => None,
    }
}

fn expr_name_list(
    expr: &Expr,
    source: &str,
    import_bindings: &BTreeMap<String, String>,
) -> Vec<String> {
    match expr {
        Expr::Tuple(tuple) => tuple
            .elts
            .iter()
            .flat_map(|expr| expr_name_list(expr, source, import_bindings))
            .collect(),
        Expr::List(list) => list
            .elts
            .iter()
            .flat_map(|expr| expr_name_list(expr, source, import_bindings))
            .collect(),
        _ => decorator_target_name(expr)
            .map(|name| normalize_imported_name(&name, import_bindings))
            .or_else(|| extract_string_literal_value(source, expr))
            .into_iter()
            .collect(),
    }
}

fn collect_import_bindings(suite: &[Stmt]) -> BTreeMap<String, String> {
    let mut bindings = BTreeMap::new();
    for stmt in suite {
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    bindings.insert(
                        alias
                            .asname
                            .as_ref()
                            .map(|name| name.as_str())
                            .unwrap_or_else(|| alias.name.as_str())
                            .to_owned(),
                        alias.name.as_str().to_owned(),
                    );
                }
            }
            Stmt::ImportFrom(import) => {
                let module = import.module.as_deref().unwrap_or("");
                for alias in &import.names {
                    bindings.insert(
                        alias
                            .asname
                            .as_ref()
                            .map(|name| name.as_str())
                            .unwrap_or_else(|| alias.name.as_str())
                            .to_owned(),
                        if module.is_empty() {
                            alias.name.as_str().to_owned()
                        } else {
                            format!("{module}.{}", alias.name)
                        },
                    );
                }
            }
            _ => {}
        }
    }
    bindings
}

fn normalize_imported_name(name: &str, import_bindings: &BTreeMap<String, String>) -> String {
    let mut parts = name.split('.');
    let head = parts.next().unwrap_or(name);
    let tail = parts.collect::<Vec<_>>();
    let head = import_bindings.get(head).cloned().unwrap_or_else(|| head.to_owned());
    if tail.is_empty() {
        head
    } else {
        format!("{head}.{}", tail.join("."))
    }
}

fn parameter_annotation(params: &str, target_name: &str) -> Option<String> {
    split_top_level_commas(params).into_iter().find_map(|param| {
        let (name, annotation) = param.split_once(':')?;
        let name = name.split('=').next()?.trim();
        (name == target_name)
            .then(|| annotation.split('=').next().unwrap_or(annotation).trim().to_owned())
    })
}

fn normalize_conditional_return_source(source: &str) -> String {
    let blocks = conditional_return_blocks(source);
    if blocks.is_empty() {
        return source.to_owned();
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut output = Vec::with_capacity(lines.len());
    let mut line_number = 1usize;
    let mut blocks = blocks.into_iter().peekable();
    while line_number <= lines.len() {
        if let Some(block) = blocks.peek() {
            if block.line == line_number {
                let original = lines[line_number - 1];
                let indent = &original[..original.len() - original.trim_start().len()];
                output.push(format!("{indent}{}:", block.header));
                let case_indent = format!("{indent}    ");
                output.push(format!("{case_indent}pass"));
                for _ in block.line + 1..=block.end_line {
                    output.push(String::new());
                }
                line_number = block.end_line + 1;
                blocks.next();
                continue;
            }
        }
        output.push(lines[line_number - 1].to_owned());
        line_number += 1;
    }

    let mut normalized = output.join("\n");
    if source.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

#[derive(Debug, Clone)]
struct ConditionalReturnBlock {
    function_name: String,
    header: String,
    target_name: String,
    case_input_types: Vec<String>,
    line: usize,
    end_line: usize,
}

fn conditional_return_blocks(source: &str) -> Vec<ConditionalReturnBlock> {
    let lines: Vec<&str> = source.lines().collect();
    let mut blocks = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        if !trimmed.starts_with("def ") {
            index += 1;
            continue;
        }
        let indent = line.len() - trimmed.len();

        let mut header_parts = vec![trimmed];
        let mut header_cursor = index;
        let mut conditional_header = None;
        while header_cursor < lines.len() {
            let header_line = lines[header_cursor];
            let header_trimmed = header_line.trim_start();
            if header_cursor > index {
                header_parts.push(header_trimmed);
            }

            if header_trimmed.contains("-> match ") && header_trimmed.ends_with(':') {
                conditional_header = Some(header_parts.join(" "));
                break;
            }

            if header_cursor > index {
                let continuation_indent = header_line.len() - header_trimmed.len();
                if continuation_indent <= indent || header_trimmed.starts_with("case ") {
                    break;
                }
            }

            header_cursor += 1;
        }

        let Some(header_line) = conditional_header else {
            index += 1;
            continue;
        };
        let Some((header, rest)) = header_line.split_once("-> match ") else {
            index += 1;
            continue;
        };
        let Some(target_name) =
            rest.strip_suffix(':').map(str::trim).filter(|name| !name.is_empty())
        else {
            index += 1;
            continue;
        };
        let Some(function_name) = header
            .strip_prefix("def ")
            .and_then(|rest| rest.split('(').next())
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            index += 1;
            continue;
        };

        let mut case_input_types = Vec::new();
        let mut cursor = header_cursor + 1;
        while cursor < lines.len() {
            let case_line = lines[cursor];
            let case_trimmed = case_line.trim_start();
            if case_trimmed.is_empty() {
                cursor += 1;
                continue;
            }
            let case_indent = case_line.len() - case_trimmed.len();
            if case_indent <= indent || !case_trimmed.starts_with("case ") {
                break;
            }
            if let Some((case_type, _)) =
                case_trimmed.strip_prefix("case ").and_then(|rest| rest.split_once(':'))
            {
                case_input_types.push(case_type.trim().to_owned());
            }
            cursor += 1;
        }

        if !case_input_types.is_empty() {
            blocks.push(ConditionalReturnBlock {
                function_name: function_name.to_owned(),
                header: header.trim_end().to_owned(),
                target_name: target_name.to_owned(),
                case_input_types,
                line: index + 1,
                end_line: cursor,
            });
            index = cursor;
        } else {
            index += 1;
        }
    }
    blocks
}

fn collect_typed_dict_mutation_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<TypedDictMutationSite>,
) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        sites.extend(extract_typed_dict_mutation_sites_from_stmt(
            source,
            stmt,
            line,
            owner_name,
            owner_type_name,
        ));

        match stmt {
            Stmt::FunctionDef(function) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_typed_dict_mutation_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_typed_dict_mutation_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_typed_dict_mutation_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            _ => {}
        }
    }
}

fn extract_typed_dict_mutation_sites_from_stmt(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Vec<TypedDictMutationSite> {
    match stmt {
        Stmt::Assign(assign) => assign
            .targets
            .iter()
            .filter_map(|target| {
                extract_typed_dict_mutation_site(
                    source,
                    target,
                    Some(&assign.value),
                    TypedDictMutationKind::Assignment,
                    line,
                    owner_name,
                    owner_type_name,
                )
            })
            .collect(),
        Stmt::AugAssign(assign) => extract_typed_dict_mutation_site(
            source,
            &assign.target,
            Some(&assign.value),
            TypedDictMutationKind::AugmentedAssignment,
            line,
            owner_name,
            owner_type_name,
        )
        .into_iter()
        .collect(),
        Stmt::Delete(delete) => delete
            .targets
            .iter()
            .filter_map(|target| {
                extract_typed_dict_mutation_site(
                    source,
                    target,
                    None,
                    TypedDictMutationKind::Delete,
                    line,
                    owner_name,
                    owner_type_name,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_typed_dict_mutation_site(
    source: &str,
    expr: &Expr,
    value: Option<&Expr>,
    kind: TypedDictMutationKind,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<TypedDictMutationSite> {
    let Expr::Subscript(subscript) = expr else {
        return None;
    };
    Some(TypedDictMutationSite {
        kind,
        key: extract_string_literal_value(source, &subscript.slice),
        target: extract_direct_expr_metadata(source, &subscript.value),
        value: value.map(|expr| extract_direct_expr_metadata(source, expr)),
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        line,
    })
}

fn collect_direct_call_context_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<DirectCallContextSite>,
) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        if let Some(site) =
            extract_direct_call_context_site(stmt, line, owner_name, owner_type_name)
        {
            sites.push(site);
        }

        match stmt {
            Stmt::FunctionDef(function) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_direct_call_context_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                collect_direct_call_context_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_direct_call_context_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_direct_call_context_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_direct_call_context_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_direct_call_context_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_direct_call_context_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            _ => {}
        }
    }
}

fn extract_direct_call_context_site(
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<DirectCallContextSite> {
    let expr = match stmt {
        Stmt::Expr(expr) => Some(expr.value.as_ref()),
        Stmt::Assign(assign) => Some(assign.value.as_ref()),
        Stmt::AnnAssign(assign) => assign.value.as_deref(),
        Stmt::Return(return_stmt) => return_stmt.value.as_deref(),
        _ => None,
    }?;

    Some(DirectCallContextSite {
        callee: extract_direct_call_context_callee(expr)?,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        line,
    })
}

fn extract_direct_call_context_callee(expr: &Expr) -> Option<String> {
    if let Expr::Await(await_expr) = expr {
        return extract_direct_call_context_callee(&await_expr.value);
    }

    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };
    Some(name.id.as_str().to_owned())
}

fn collect_typed_dict_literal_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<TypedDictLiteralSite>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_typed_dict_literal_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_typed_dict_literal_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_typed_dict_literal_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::AnnAssign(assign) => {
                let line = offset_to_line_column(source, assign.range.start().to_usize()).0;
                if let Some(site) = extract_typed_dict_literal_site(
                    source,
                    assign,
                    line,
                    owner_name,
                    owner_type_name,
                ) {
                    sites.push(site);
                }
            }
            _ => {}
        }
    }
}

struct UnsafeOperationCollector<'source> {
    source: &'source str,
    unsafe_ranges: Vec<(usize, usize)>,
    sites: Vec<UnsafeOperationSite>,
}

impl<'source, 'ast> visitor::Visitor<'ast> for UnsafeOperationCollector<'source> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match stmt {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    self.push_unsafe_write_target(target);
                }
            }
            Stmt::AnnAssign(assign) => self.push_unsafe_write_target(&assign.target),
            Stmt::AugAssign(assign) => self.push_unsafe_write_target(&assign.target),
            Stmt::Delete(delete) => {
                for target in &delete.targets {
                    self.push_unsafe_write_target(target);
                }
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let Expr::Call(call) = expr {
            let Expr::Name(name) = call.func.as_ref() else {
                visitor::walk_expr(self, expr);
                return;
            };
            match name.id.as_str() {
                "eval" => self.push_site(expr.range(), UnsafeOperationKind::EvalCall),
                "exec" => self.push_site(expr.range(), UnsafeOperationKind::ExecCall),
                "setattr" if call.arguments.args.len() >= 2 => {
                    if !matches!(call.arguments.args[1], Expr::StringLiteral(_)) {
                        self.push_site(expr.range(), UnsafeOperationKind::SetAttrNonLiteral);
                    }
                }
                "delattr" if call.arguments.args.len() >= 2 => {
                    if !matches!(call.arguments.args[1], Expr::StringLiteral(_)) {
                        self.push_site(expr.range(), UnsafeOperationKind::DelAttrNonLiteral);
                    }
                }
                _ => {}
            }
        }
        visitor::walk_expr(self, expr);
    }
}

impl<'source> UnsafeOperationCollector<'source> {
    fn push_unsafe_write_target(&mut self, target: &Expr) {
        if let Some(kind) = unsafe_write_target_kind(target) {
            self.push_site(target.range(), kind);
        }
    }

    fn push_site(&mut self, range: ruff_text_size::TextRange, kind: UnsafeOperationKind) {
        let line = offset_to_line_column(self.source, range.start().to_usize()).0;
        let in_unsafe_block =
            self.unsafe_ranges.iter().any(|(start, end)| *start <= line && line <= *end);
        self.sites.push(UnsafeOperationSite { kind, line, in_unsafe_block });
    }
}

fn unsafe_write_target_kind(target: &Expr) -> Option<UnsafeOperationKind> {
    match target {
        Expr::Subscript(subscript) => match subscript.value.as_ref() {
            Expr::Call(call) => {
                let Expr::Name(name) = call.func.as_ref() else {
                    return None;
                };
                match name.id.as_str() {
                    "globals" => Some(UnsafeOperationKind::GlobalsWrite),
                    "locals" => Some(UnsafeOperationKind::LocalsWrite),
                    _ => None,
                }
            }
            Expr::Attribute(attribute) if attribute.attr.as_str() == "__dict__" => {
                Some(UnsafeOperationKind::DictWrite)
            }
            _ => None,
        },
        Expr::Attribute(attribute) if attribute.attr.as_str() == "__dict__" => {
            Some(UnsafeOperationKind::DictWrite)
        }
        _ => None,
    }
}

fn collect_unsafe_block_ranges(
    source: &str,
    statements: &[SyntaxStatement],
) -> Vec<(usize, usize)> {
    let unsafe_lines = statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Unsafe(statement) => Some(statement.line),
            _ => None,
        })
        .collect::<Vec<_>>();
    let lines = source.lines().collect::<Vec<_>>();
    unsafe_lines
        .into_iter()
        .filter_map(|line_number| {
            let header = lines.get(line_number.saturating_sub(1))?;
            let header_indent =
                header.chars().take_while(|character| character.is_whitespace()).count();
            let mut end_line = line_number;
            for (index, line) in lines.iter().enumerate().skip(line_number) {
                let trimmed = line.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let indent = line.chars().take_while(|character| character.is_whitespace()).count();
                if indent <= header_indent {
                    break;
                }
                end_line = index + 1;
            }
            Some((line_number, end_line))
        })
        .collect()
}

fn extract_typed_dict_literal_site(
    source: &str,
    assign: &ruff_python_ast::StmtAnnAssign,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<TypedDictLiteralSite> {
    let annotation = slice_range(source, assign.annotation.range())?.to_owned();
    let value = assign.value.as_deref()?.as_dict_expr()?;
    let entries = extract_typed_dict_literal_entries(source, value);
    Some(TypedDictLiteralSite {
        annotation,
        entries,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        line,
    })
}

fn extract_typed_dict_literal_entries(
    source: &str,
    value: &ruff_python_ast::ExprDict,
) -> Vec<TypedDictLiteralEntry> {
    value
        .iter()
        .map(|item| TypedDictLiteralEntry {
            key: item.key.as_ref().and_then(|key| extract_string_literal_value(source, key)),
            key_value: item
                .key
                .as_ref()
                .map(|key| Box::new(extract_direct_expr_metadata(source, key))),
            is_expansion: item.key.is_none(),
            value: extract_direct_expr_metadata(source, &item.value),
        })
        .collect()
}

fn extract_string_literal_value(source: &str, expr: &Expr) -> Option<String> {
    let Expr::StringLiteral(_) = expr else {
        return None;
    };
    let raw = slice_range(source, expr.range())?.trim();
    let quote_start = raw.find(['\'', '"'])?;
    let quoted = &raw[quote_start..];
    if let Some(inner) =
        quoted.strip_prefix("\"\"\"").and_then(|inner| inner.strip_suffix("\"\"\""))
    {
        return Some(inner.to_owned());
    }
    if let Some(inner) =
        quoted.strip_prefix("'''").and_then(|inner| inner.strip_suffix("'''")).map(str::to_owned)
    {
        return Some(inner);
    }
    if let Some(inner) = quoted.strip_prefix('"').and_then(|inner| inner.strip_suffix('"')) {
        return Some(inner.to_owned());
    }
    quoted.strip_prefix('\'').and_then(|inner| inner.strip_suffix('\'')).map(str::to_owned)
}

fn parse_python_source(source: SourceFile) -> SyntaxTree {
    let mut statements = Vec::new();
    let mut diagnostics = DiagnosticReport::default();
    let type_ignore_directives = parse_type_ignore_directives(&source.text);

    match parse_module(&source.text) {
        Ok(parsed) => {
            collect_invalid_annotation_placement_diagnostics(
                &source.path,
                &source.text,
                parsed.suite(),
                false,
                &mut diagnostics,
            );
            statements.extend(extract_ast_backed_statements(
                &source.path,
                &source.logical_module,
                &source.text,
                &source.text,
                parsed.suite(),
                &[],
                &mut diagnostics,
            ));
            collect_return_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_yield_statements(&source.text, parsed.suite(), None, &mut statements);
            collect_if_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_assert_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_invalidation_statements(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_match_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_for_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_with_statements(&source.text, parsed.suite(), None, None, &mut statements);
            collect_except_handler_statements(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_nested_call_statements(&source.text, parsed.suite(), &mut statements);
            collect_function_body_assignments(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_function_body_bare_assignments(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            collect_function_body_namedexpr_assignments(
                &source.text,
                parsed.suite(),
                None,
                None,
                &mut statements,
            );
            statements.sort_by_key(statement_line);
        }
        Err(error) => {
            let code = parse_error_code(&error.error.to_string());
            diagnostics.push(
                Diagnostic::error(code, format!("Python syntax error: {}", error.error)).with_span(
                    parse_error_span(
                        &source.path,
                        &source.text,
                        error.location.start().to_usize(),
                        error.location.end().to_usize(),
                    ),
                ),
            );
        }
    }

    SyntaxTree { source, statements, type_ignore_directives, diagnostics }
}

fn parse_typepython_source(source: SourceFile, options: ParseOptions) -> SyntaxTree {
    let mut statements = Vec::new();
    let mut diagnostics = DiagnosticReport::default();
    let type_ignore_directives = parse_type_ignore_directives(&source.text);

    for (index, line) in source.text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(statement) =
            parse_extension_statement(&source.path, trimmed, line_number, &mut diagnostics)
        {
            statements.push(statement);
        }
    }

    if !diagnostics.has_errors() {
        let normalized_typepython_source = normalize_typepython_source(&source.text, &statements);
        let normalized = if options.enable_conditional_returns {
            normalize_conditional_return_source(&normalized_typepython_source)
        } else {
            normalized_typepython_source
        };
        match parse_module(&normalized) {
            Ok(parsed) => {
                collect_invalid_annotation_placement_diagnostics(
                    &source.path,
                    &normalized,
                    parsed.suite(),
                    false,
                    &mut diagnostics,
                );
                collect_deferred_async_construct_diagnostics(
                    &source.path,
                    &normalized,
                    parsed.suite(),
                    &mut diagnostics,
                );
                refresh_custom_statements_from_ast(
                    &source.path,
                    &normalized,
                    parsed.suite(),
                    &mut statements,
                    &mut diagnostics,
                );
                statements.extend(extract_ast_backed_statements(
                    &source.path,
                    &source.logical_module,
                    &normalized,
                    &normalized,
                    parsed.suite(),
                    &statements,
                    &mut diagnostics,
                ));
                collect_return_statements(&normalized, parsed.suite(), None, None, &mut statements);
                collect_yield_statements(&normalized, parsed.suite(), None, &mut statements);
                collect_if_statements(&normalized, parsed.suite(), None, None, &mut statements);
                collect_assert_statements(&normalized, parsed.suite(), None, None, &mut statements);
                collect_invalidation_statements(
                    &normalized,
                    parsed.suite(),
                    None,
                    None,
                    &mut statements,
                );
                collect_match_statements(&normalized, parsed.suite(), None, None, &mut statements);
                collect_for_statements(&normalized, parsed.suite(), None, None, &mut statements);
                collect_with_statements(&normalized, parsed.suite(), None, None, &mut statements);
                collect_except_handler_statements(
                    &normalized,
                    parsed.suite(),
                    None,
                    None,
                    &mut statements,
                );
                collect_nested_call_statements(&normalized, parsed.suite(), &mut statements);
                collect_function_body_assignments(
                    &normalized,
                    parsed.suite(),
                    None,
                    None,
                    &mut statements,
                );
                collect_function_body_bare_assignments(
                    &normalized,
                    parsed.suite(),
                    None,
                    None,
                    &mut statements,
                );
                collect_function_body_namedexpr_assignments(
                    &normalized,
                    parsed.suite(),
                    None,
                    None,
                    &mut statements,
                );
                statements.sort_by_key(statement_line);
            }
            Err(error) => {
                let code = parse_error_code(&error.error.to_string());
                diagnostics.push(
                    Diagnostic::error(code, format!("TypePython syntax error: {}", error.error))
                        .with_span(parse_error_span(
                            &source.path,
                            &source.text,
                            error.location.start().to_usize(),
                            error.location.end().to_usize(),
                        )),
                );
            }
        }
    }

    SyntaxTree { source, statements, type_ignore_directives, diagnostics }
}

fn parse_error_code(message: &str) -> &'static str {
    if matches!(
        message,
        "Invalid assignment target"
            | "Invalid delete target"
            | "Assignment expression target must be an identifier"
    ) {
        "TPY4011"
    } else {
        "TPY2001"
    }
}

pub fn apply_type_ignore_directives(
    syntax_trees: &[SyntaxTree],
    diagnostics: &mut DiagnosticReport,
) {
    let directives_by_path = syntax_trees
        .iter()
        .map(|tree| {
            (tree.source.path.to_string_lossy().into_owned(), tree.type_ignore_directives.clone())
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    diagnostics.diagnostics.retain(|diagnostic| {
        let Some(span) = &diagnostic.span else {
            return true;
        };
        let Some(directives) = directives_by_path.get(&span.path) else {
            return true;
        };
        !directives.iter().any(|directive| {
            directive.line == span.line
                && match &directive.codes {
                    None => true,
                    Some(codes) => codes.iter().any(|code| code == &diagnostic.code),
                }
        })
    });
}

fn parse_type_ignore_directives(text: &str) -> Vec<TypeIgnoreDirective> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| parse_type_ignore_directive_line(index + 1, line))
        .collect()
}

fn parse_type_ignore_directive_line(line_number: usize, line: &str) -> Option<TypeIgnoreDirective> {
    let (_, comment) = line.split_once('#')?;
    let comment = comment.trim();
    let remainder = comment.strip_prefix("type: ignore")?.trim();
    let codes = if remainder.is_empty() {
        None
    } else {
        let inner = remainder.strip_prefix('[')?.strip_suffix(']')?;
        Some(
            inner
                .split(',')
                .map(str::trim)
                .filter(|code| !code.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    };
    Some(TypeIgnoreDirective { line: line_number, codes })
}

fn collect_invalid_annotation_placement_diagnostics(
    path: &Path,
    source: &str,
    suite: &[Stmt],
    in_function_body: bool,
    diagnostics: &mut DiagnosticReport,
) {
    for statement in suite {
        match statement {
            Stmt::AnnAssign(assign)
                if in_function_body && is_classvar_annotation(&assign.annotation) =>
            {
                let line = offset_to_line_column(source, assign.range.start().to_usize()).0;
                diagnostics.push(
                    Diagnostic::error(
                        "TPY4001",
                        "ClassVar[...] is not allowed inside function or method bodies",
                    )
                    .with_span(Span::new(
                        path.display().to_string(),
                        line,
                        1,
                        line,
                        1,
                    )),
                );
            }
            Stmt::FunctionDef(function) => {
                collect_invalid_parameter_annotation_diagnostics(
                    path,
                    source,
                    &function.parameters,
                    diagnostics,
                );
                collect_invalid_annotation_placement_diagnostics(
                    path,
                    source,
                    &function.body,
                    true,
                    diagnostics,
                )
            }
            Stmt::ClassDef(class_def) => collect_invalid_annotation_placement_diagnostics(
                path,
                source,
                &class_def.body,
                false,
                diagnostics,
            ),
            _ => {}
        }
    }
}

fn collect_deferred_async_construct_diagnostics(
    path: &Path,
    source: &str,
    suite: &[Stmt],
    diagnostics: &mut DiagnosticReport,
) {
    let mut visitor = DeferredAsyncConstructVisitor { path, source, diagnostics };
    visitor.visit_body(suite);
}

struct DeferredAsyncConstructVisitor<'a> {
    path: &'a Path,
    source: &'a str,
    diagnostics: &'a mut DiagnosticReport,
}

impl<'a> DeferredAsyncConstructVisitor<'a> {
    fn push_deferred(&mut self, range: ruff_text_size::TextRange, construct: &str) {
        let line = offset_to_line_column(self.source, range.start().to_usize()).0;
        self.diagnostics.push(
            Diagnostic::error(
                "TPY4010",
                format!("{construct} in .tpy source is deferred beyond v1"),
            )
            .with_span(Span::new(self.path.display().to_string(), line, 1, line, 1)),
        );
    }
}

impl<'a> visitor::Visitor<'a> for DeferredAsyncConstructVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(function) if function.is_async => {
                self.push_deferred(function.range(), "`async def`");
            }
            Stmt::For(for_stmt) if for_stmt.is_async => {
                self.push_deferred(for_stmt.range(), "`async for`");
            }
            Stmt::With(with_stmt) if with_stmt.is_async => {
                self.push_deferred(with_stmt.range(), "`async with`");
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Await(await_expr) => self.push_deferred(await_expr.range(), "`await`"),
            Expr::Yield(yield_expr) => self.push_deferred(yield_expr.range(), "`yield`"),
            Expr::YieldFrom(yield_from_expr) => {
                self.push_deferred(yield_from_expr.range(), "`yield from`");
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

fn collect_invalid_parameter_annotation_diagnostics(
    path: &Path,
    source: &str,
    parameters: &ruff_python_ast::Parameters,
    diagnostics: &mut DiagnosticReport,
) {
    for parameter in parameters.iter() {
        let Some(annotation) = parameter.annotation() else {
            continue;
        };
        let line = offset_to_line_column(source, parameter.range().start().to_usize()).0;

        if is_classvar_annotation(annotation) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4001",
                    "ClassVar[...] is not allowed in function or method parameter annotations",
                )
                .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
            );
        }
        if is_final_annotation(annotation) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4010",
                    "Final[...] in function or method parameter position is deferred beyond v1",
                )
                .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
            );
        }
    }
}

fn refresh_custom_statements_from_ast(
    path: &Path,
    normalized: &str,
    suite: &[Stmt],
    statements: &mut [SyntaxStatement],
    diagnostics: &mut DiagnosticReport,
) {
    for statement in statements.iter_mut() {
        match statement {
            SyntaxStatement::Interface(existing) => {
                if let Some(ast_statement) =
                    ast_class_def_for_line(normalized, suite, existing.line)
                {
                    for body_statement in &ast_statement.body {
                        if !is_valid_interface_body_statement(body_statement) {
                            diagnostics.push(
                                Diagnostic::error(
                                    "TPY2001",
                                    format!(
                                        "interface `{}` body must not contain executable statements",
                                        existing.name
                                    ),
                                )
                                .with_span(Span::new(
                                    path.display().to_string(),
                                    offset_to_line_column(
                                        normalized,
                                        body_statement.range().start().to_usize(),
                                    )
                                    .0,
                                    1,
                                    offset_to_line_column(
                                        normalized,
                                        body_statement.range().end().to_usize(),
                                    )
                                    .0,
                                    1,
                                )),
                            );
                            break;
                        }
                    }
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "interface declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.is_final_decorator =
                            ast_statement.decorator_list.iter().any(is_final_decorator);
                        existing.deprecation_message =
                            deprecated_decorator_message(&ast_statement.decorator_list);
                        existing.is_deprecated = existing.deprecation_message.is_some();
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.bases = ast_statement
                            .arguments
                            .as_ref()
                            .map(|arguments| extract_class_bases(normalized, arguments))
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                        existing.is_abstract_class = is_abstract_class(existing);
                    }
                }
            }
            SyntaxStatement::DataClass(existing) => {
                if let Some(ast_statement) =
                    ast_class_def_for_line(normalized, suite, existing.line)
                {
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "data class declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.is_final_decorator =
                            ast_statement.decorator_list.iter().any(is_final_decorator);
                        existing.deprecation_message =
                            deprecated_decorator_message(&ast_statement.decorator_list);
                        existing.is_deprecated = existing.deprecation_message.is_some();
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.bases = ast_statement
                            .arguments
                            .as_ref()
                            .map(|arguments| extract_class_bases(normalized, arguments))
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                        existing.is_abstract_class = is_abstract_class(existing);
                    }
                }
            }
            SyntaxStatement::SealedClass(existing) => {
                if let Some(ast_statement) =
                    ast_class_def_for_line(normalized, suite, existing.line)
                {
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "sealed class declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.is_final_decorator =
                            ast_statement.decorator_list.iter().any(is_final_decorator);
                        existing.deprecation_message =
                            deprecated_decorator_message(&ast_statement.decorator_list);
                        existing.is_deprecated = existing.deprecation_message.is_some();
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.bases = ast_statement
                            .arguments
                            .as_ref()
                            .map(|arguments| extract_class_bases(normalized, arguments))
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                        existing.is_abstract_class = is_abstract_class(existing);
                    }
                }
            }
            SyntaxStatement::OverloadDef(existing) => {
                if let Some(ast_statement) =
                    ast_function_def_for_line(normalized, suite, existing.line)
                {
                    if !is_stub_like_function_body(&ast_statement.body) {
                        diagnostics.push(
                            Diagnostic::error(
                                "TPY2001",
                                format!(
                                    "overload declaration `{}` body must not contain executable statements",
                                    existing.name
                                ),
                            )
                            .with_span(Span::new(
                                path.display().to_string(),
                                offset_to_line_column(
                                    normalized,
                                    ast_statement.range.start().to_usize(),
                                )
                                .0,
                                1,
                                offset_to_line_column(
                                    normalized,
                                    ast_statement.range.end().to_usize(),
                                )
                                .0,
                                1,
                            )),
                        );
                    }
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "overload declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.params =
                            extract_function_params(normalized, &ast_statement.parameters);
                        existing.returns = ast_statement
                            .returns
                            .as_ref()
                            .and_then(|returns| slice_range(normalized, returns.range()))
                            .map(str::to_owned);
                        existing.is_async = ast_statement.is_async;
                        existing.is_override =
                            ast_statement.decorator_list.iter().any(is_override_decorator);
                        existing.deprecation_message =
                            deprecated_decorator_message(&ast_statement.decorator_list);
                        existing.is_deprecated = existing.deprecation_message.is_some();
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_valid_interface_body_statement(statement: &Stmt) -> bool {
    match statement {
        Stmt::AnnAssign(_) | Stmt::Pass(_) => true,
        Stmt::Expr(expr) => {
            matches!(expr.value.as_ref(), Expr::StringLiteral(_) | Expr::EllipsisLiteral(_))
        }
        Stmt::FunctionDef(function) => is_stub_like_function_body(&function.body),
        _ => false,
    }
}

fn is_stub_like_function_body(body: &[Stmt]) -> bool {
    body.iter().all(|statement| {
        matches!(statement, Stmt::Pass(_))
            || matches!(statement, Stmt::Expr(expr) if matches!(expr.value.as_ref(), Expr::StringLiteral(_) | Expr::EllipsisLiteral(_)))
    })
}

fn extract_class_members(normalized: &str, body: &[Stmt]) -> Vec<ClassMember> {
    let mut members = Vec::new();

    for statement in body {
        match statement {
            Stmt::FunctionDef(function) => members.push(ClassMember {
                name: function.name.as_str().to_owned(),
                kind: if function.decorator_list.iter().any(is_overload_decorator) {
                    ClassMemberKind::Overload
                } else {
                    ClassMemberKind::Method
                },
                method_kind: Some(method_kind_from_decorators(&function.decorator_list)),
                annotation: None,
                value_type: None,
                params: extract_function_params(normalized, &function.parameters),
                returns: function
                    .returns
                    .as_ref()
                    .and_then(|returns| slice_range(normalized, returns.range()))
                    .map(str::to_owned),
                is_async: function.is_async,
                is_override: function.decorator_list.iter().any(is_override_decorator),
                is_abstract_method: function.decorator_list.iter().any(is_abstractmethod_decorator),
                is_final_decorator: function.decorator_list.iter().any(is_final_decorator),
                deprecation_message: deprecated_decorator_message(&function.decorator_list),
                is_deprecated: deprecated_decorator_message(&function.decorator_list).is_some(),
                is_final: false,
                is_class_var: false,
                line: offset_to_line_column(normalized, function.range.start().to_usize()).0,
            }),
            Stmt::AnnAssign(assign) => {
                let is_final = is_final_annotation(&assign.annotation);
                let is_class_var = is_classvar_annotation(&assign.annotation);
                members.extend(extract_assignment_names(&assign.target).into_iter().map(|name| {
                    ClassMember {
                        name,
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: slice_range(normalized, assign.annotation.range())
                            .map(str::to_owned),
                        value_type: assign.value.as_deref().map(infer_literal_arg_type),
                        params: Vec::new(),
                        returns: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final,
                        is_class_var,
                        line: offset_to_line_column(normalized, assign.range.start().to_usize()).0,
                    }
                }));
            }
            Stmt::Assign(assign) => {
                let line = offset_to_line_column(normalized, assign.range.start().to_usize()).0;
                members.extend(assign.targets.iter().flat_map(extract_assignment_names).map(
                    |name| ClassMember {
                        name,
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
                        line,
                    },
                ));
            }
            _ => {}
        }
    }

    members
}

fn ast_class_def_for_line<'a>(
    normalized: &str,
    suite: &'a [Stmt],
    line: usize,
) -> Option<&'a ruff_python_ast::StmtClassDef> {
    suite.iter().find_map(|stmt| match stmt {
        Stmt::ClassDef(class_def)
            if offset_to_line_column(normalized, class_def.range.start().to_usize()).0 == line =>
        {
            Some(class_def)
        }
        _ => None,
    })
}

fn ast_function_def_for_line<'a>(
    normalized: &str,
    suite: &'a [Stmt],
    line: usize,
) -> Option<&'a ruff_python_ast::StmtFunctionDef> {
    suite.iter().find_map(|stmt| match stmt {
        Stmt::FunctionDef(function_def)
            if offset_to_line_column(normalized, function_def.range.start().to_usize()).0
                == line =>
        {
            Some(function_def)
        }
        _ => None,
    })
}

fn normalize_typepython_source(source: &str, statements: &[SyntaxStatement]) -> String {
    let statement_lines: std::collections::BTreeMap<usize, &SyntaxStatement> =
        statements.iter().map(|statement| (statement_line(statement), statement)).collect();

    let mut normalized_lines = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let normalized = if let Some(statement) = statement_lines.get(&line_number) {
            normalize_typepython_statement_line(line, statement)
        } else {
            normalize_generic_python_header_line(line)
        };
        normalized_lines.push(normalized);
    }

    let mut normalized = normalized_lines.join("\n");
    if source.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn statement_line(statement: &SyntaxStatement) -> usize {
    match statement {
        SyntaxStatement::TypeAlias(statement) => statement.line,
        SyntaxStatement::Interface(statement) => statement.line,
        SyntaxStatement::DataClass(statement) => statement.line,
        SyntaxStatement::SealedClass(statement) => statement.line,
        SyntaxStatement::OverloadDef(statement) => statement.line,
        SyntaxStatement::ClassDef(statement) => statement.line,
        SyntaxStatement::FunctionDef(statement) => statement.line,
        SyntaxStatement::Import(statement) => statement.line,
        SyntaxStatement::Value(statement) => statement.line,
        SyntaxStatement::Call(statement) => statement.line,
        SyntaxStatement::MethodCall(statement) => statement.line,
        SyntaxStatement::MemberAccess(statement) => statement.line,
        SyntaxStatement::Return(statement) => statement.line,
        SyntaxStatement::Yield(statement) => statement.line,
        SyntaxStatement::If(statement) => statement.line,
        SyntaxStatement::Assert(statement) => statement.line,
        SyntaxStatement::Invalidate(statement) => statement.line,
        SyntaxStatement::Match(statement) => statement.line,
        SyntaxStatement::For(statement) => statement.line,
        SyntaxStatement::With(statement) => statement.line,
        SyntaxStatement::ExceptHandler(statement) => statement.line,
        SyntaxStatement::Unsafe(statement) => statement.line,
    }
}

fn normalize_typepython_statement_line(line: &str, statement: &SyntaxStatement) -> String {
    match statement {
        SyntaxStatement::TypeAlias(statement) => {
            let indentation = leading_indent(line);
            format!("{indentation}{} = {}", statement.name, statement.value)
        }
        SyntaxStatement::Interface(statement)
        | SyntaxStatement::DataClass(statement)
        | SyntaxStatement::SealedClass(statement)
        | SyntaxStatement::ClassDef(statement) => {
            let indentation = leading_indent(line);
            format!(
                "{indentation}class {}{}{}:",
                statement.name,
                render_type_params(&statement.type_params),
                statement.header_suffix
            )
        }
        SyntaxStatement::OverloadDef(_) => {
            let indentation = leading_indent(line);
            let trimmed = line.trim_start();
            let rest = trimmed.strip_prefix("overload ").unwrap_or(trimmed);
            format!("{indentation}{rest}")
        }
        SyntaxStatement::FunctionDef(_) => line.to_owned(),
        SyntaxStatement::Import(_)
        | SyntaxStatement::Value(_)
        | SyntaxStatement::Call(_)
        | SyntaxStatement::MethodCall(_)
        | SyntaxStatement::MemberAccess(_)
        | SyntaxStatement::Return(_)
        | SyntaxStatement::Yield(_)
        | SyntaxStatement::If(_)
        | SyntaxStatement::Assert(_)
        | SyntaxStatement::Invalidate(_)
        | SyntaxStatement::Match(_)
        | SyntaxStatement::For(_)
        | SyntaxStatement::With(_)
        | SyntaxStatement::ExceptHandler(_) => line.to_owned(),
        SyntaxStatement::Unsafe(_) => {
            let indentation = leading_indent(line);
            format!("{indentation}if True:")
        }
    }
}

fn normalize_generic_python_header_line(line: &str) -> String {
    line.to_owned()
}

fn leading_indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

fn render_type_params(type_params: &[TypeParam]) -> String {
    if type_params.is_empty() {
        return String::new();
    }

    format!(
        "[{}]",
        type_params
            .iter()
            .map(|type_param| match &type_param.bound {
                Some(bound) => format!("{}: {}", type_param.name, bound),
                None => type_param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn extract_ast_backed_statements(
    path: &Path,
    current_module_key: &str,
    source: &str,
    normalized: &str,
    suite: &[Stmt],
    existing: &[SyntaxStatement],
    diagnostics: &mut DiagnosticReport,
) -> Vec<SyntaxStatement> {
    let existing_lines: std::collections::BTreeSet<_> =
        existing.iter().map(statement_line).collect();
    let mut statements = Vec::new();

    for stmt in suite {
        let line = offset_to_line_column(normalized, stmt.range().start().to_usize()).0;
        if existing_lines.contains(&line) {
            continue;
        }
        if let Some(statement) = extract_ast_backed_statement(
            path,
            current_module_key,
            source,
            normalized,
            stmt,
            line,
            diagnostics,
        ) {
            statements.push(statement);
        }
        if let Some(call_statement) = extract_supplemental_call_statement(source, stmt, line) {
            statements.push(call_statement);
        }
        if let Some(method_call) = extract_method_call_statement(source, stmt, line) {
            statements.push(method_call);
        }
        if let Some(member_access) = extract_member_access_statement(source, stmt, line) {
            statements.push(member_access);
        }
    }

    statements
}

fn extract_ast_backed_statement(
    path: &Path,
    current_module_key: &str,
    source: &str,
    normalized: &str,
    stmt: &Stmt,
    line: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    match stmt {
        Stmt::ClassDef(stmt) => {
            let deprecation_message = deprecated_decorator_message(&stmt.decorator_list);
            let mut statement = NamedBlockStatement {
                name: stmt.name.as_str().to_owned(),
                type_params: extract_ast_type_params(
                    path,
                    source,
                    stmt.type_params.as_deref(),
                    line,
                    "class declaration",
                    diagnostics,
                )?,
                header_suffix: stmt
                    .arguments
                    .as_ref()
                    .and_then(|arguments| slice_range(source, arguments.range()))
                    .map(str::to_owned)
                    .unwrap_or_default(),
                bases: stmt
                    .arguments
                    .as_ref()
                    .map(|arguments| extract_class_bases(source, arguments))
                    .unwrap_or_default(),
                is_final_decorator: stmt.decorator_list.iter().any(is_final_decorator),
                is_deprecated: deprecation_message.is_some(),
                deprecation_message,
                members: extract_class_members(normalized, &stmt.body),
                is_abstract_class: false,
                line,
            };
            statement.is_abstract_class = is_abstract_class(&statement);
            Some(SyntaxStatement::ClassDef(statement))
        }
        Stmt::FunctionDef(stmt) => {
            let is_overload = stmt.decorator_list.iter().any(is_overload_decorator);
            let deprecation_message = deprecated_decorator_message(&stmt.decorator_list);
            let statement = FunctionStatement {
                name: stmt.name.as_str().to_owned(),
                type_params: extract_ast_type_params(
                    path,
                    source,
                    stmt.type_params.as_deref(),
                    line,
                    if is_overload { "overload declaration" } else { "function declaration" },
                    diagnostics,
                )?,
                params: extract_function_params(source, &stmt.parameters),
                returns: stmt
                    .returns
                    .as_ref()
                    .and_then(|returns| slice_range(source, returns.range()))
                    .map(str::to_owned),
                is_async: stmt.is_async,
                is_override: stmt.decorator_list.iter().any(is_override_decorator),
                is_deprecated: deprecation_message.is_some(),
                deprecation_message,
                line,
            };

            Some(if is_overload {
                SyntaxStatement::OverloadDef(statement)
            } else {
                SyntaxStatement::FunctionDef(statement)
            })
        }
        Stmt::Import(stmt) => {
            let bindings = stmt
                .names
                .iter()
                .map(|alias| ImportBinding {
                    local_name: alias
                        .asname
                        .as_ref()
                        .map(|identifier| identifier.as_str().to_owned())
                        .unwrap_or_else(|| {
                            alias
                                .name
                                .as_str()
                                .split('.')
                                .next()
                                .unwrap_or(alias.name.as_str())
                                .to_owned()
                        }),
                    source_path: alias.name.as_str().to_owned(),
                })
                .collect::<Vec<_>>();
            (!bindings.is_empty())
                .then_some(SyntaxStatement::Import(ImportStatement { bindings, line }))
        }
        Stmt::ImportFrom(stmt) => {
            let bindings = stmt
                .names
                .iter()
                .map(|alias| {
                    let imported_name = alias.name.as_str();
                    let module = stmt.module.as_ref().map(|id| id.as_str()).unwrap_or("");
                    let module =
                        normalize_import_module(path, current_module_key, stmt.level, module);
                    ImportBinding {
                        local_name: alias
                            .asname
                            .as_ref()
                            .unwrap_or(&alias.name)
                            .as_str()
                            .to_owned(),
                        source_path: if module.is_empty() {
                            imported_name.to_owned()
                        } else {
                            format!("{module}.{imported_name}")
                        },
                    }
                })
                .collect::<Vec<_>>();
            (!bindings.is_empty())
                .then_some(SyntaxStatement::Import(ImportStatement { bindings, line }))
        }
        Stmt::Assign(stmt) => {
            let destructuring_target_names = (stmt.targets.len() == 1)
                .then(|| extract_simple_destructuring_target_names(&stmt.targets[0]))
                .flatten();
            let names = destructuring_target_names.clone().unwrap_or_else(|| {
                stmt.targets.iter().flat_map(extract_assignment_names).collect::<Vec<_>>()
            });
            if !names.is_empty() {
                let value = extract_direct_expr_metadata(source, &stmt.value);
                Some(SyntaxStatement::Value(ValueStatement {
                    names,
                    destructuring_target_names,
                    annotation: None,
                    value_type: value.value_type,
                    is_awaited: value.is_awaited,
                    value_callee: value.value_callee,
                    value_name: value.value_name,
                    value_member_owner_name: value.value_member_owner_name,
                    value_member_name: value.value_member_name,
                    value_member_through_instance: value.value_member_through_instance,
                    value_method_owner_name: value.value_method_owner_name,
                    value_method_name: value.value_method_name,
                    value_method_through_instance: value.value_method_through_instance,
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
                    value_lambda: value.value_lambda,
                    value_list_comprehension: value.value_list_comprehension,
                    value_generator_comprehension: value.value_generator_comprehension,
                    value_list_elements: value.value_list_elements,
                    value_set_elements: value.value_set_elements,
                    value_dict_entries: value.value_dict_entries,
                    owner_name: None,
                    owner_type_name: None,
                    is_final: false,
                    is_class_var: false,
                    line,
                }))
            } else {
                None
            }
        }
        Stmt::AnnAssign(stmt) => {
            let names = extract_assignment_names(&stmt.target);
            if !names.is_empty() {
                let value = stmt
                    .value
                    .as_deref()
                    .map(|expr| extract_direct_expr_metadata(source, expr))
                    .unwrap_or(DirectExprMetadata {
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
                    });
                Some(SyntaxStatement::Value(ValueStatement {
                    names,
                    destructuring_target_names: None,
                    annotation: slice_range(source, stmt.annotation.range()).map(str::to_owned),
                    value_type: value.value_type,
                    is_awaited: value.is_awaited,
                    value_callee: value.value_callee,
                    value_name: value.value_name,
                    value_member_owner_name: value.value_member_owner_name,
                    value_member_name: value.value_member_name,
                    value_member_through_instance: value.value_member_through_instance,
                    value_method_owner_name: value.value_method_owner_name,
                    value_method_name: value.value_method_name,
                    value_method_through_instance: value.value_method_through_instance,
                    value_subscript_target: value.value_subscript_target,
                    value_subscript_string_key: value.value_subscript_string_key,
                    value_subscript_index: value.value_subscript_index,
                    value_if_true: value.value_if_true,
                    value_if_false: value.value_if_false,
                    value_if_guard: value.value_if_guard,
                    value_bool_left: value.value_bool_left,
                    value_bool_right: value.value_bool_right,
                    value_binop_left: value.value_binop_left,
                    value_binop_right: value.value_binop_right,
                    value_binop_operator: value.value_binop_operator,
                    value_lambda: value.value_lambda,
                    value_list_comprehension: value.value_list_comprehension,
                    value_generator_comprehension: value.value_generator_comprehension,
                    value_list_elements: value.value_list_elements,
                    value_set_elements: value.value_set_elements,
                    value_dict_entries: value.value_dict_entries,
                    owner_name: None,
                    owner_type_name: None,
                    is_final: is_final_annotation(&stmt.annotation),
                    is_class_var: is_classvar_annotation(&stmt.annotation),
                    line,
                }))
            } else {
                None
            }
        }
        Stmt::If(stmt) => Some(SyntaxStatement::If(IfStatement {
            owner_name: None,
            owner_type_name: None,
            guard: extract_guard_condition(source, &stmt.test),
            line,
            true_start_line: suite_start_line(source, &stmt.body),
            true_end_line: suite_end_line(source, &stmt.body),
            false_start_line: if_false_start_line(source, stmt),
            false_end_line: if_false_end_line(source, stmt),
        })),
        Stmt::Assert(stmt) => Some(SyntaxStatement::Assert(AssertStatement {
            owner_name: None,
            owner_type_name: None,
            guard: extract_guard_condition(source, &stmt.test),
            line,
        })),
        Stmt::AugAssign(stmt) => {
            let names = extract_assignment_names(&stmt.target);
            (!names.is_empty()).then_some(SyntaxStatement::Invalidate(InvalidationStatement {
                owner_name: None,
                owner_type_name: None,
                names,
                line,
            }))
        }
        Stmt::Delete(stmt) => {
            let names = stmt.targets.iter().flat_map(extract_assignment_names).collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Invalidate(InvalidationStatement {
                owner_name: None,
                owner_type_name: None,
                names,
                line,
            }))
        }
        Stmt::Global(stmt) => {
            let names = stmt.names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Invalidate(InvalidationStatement {
                owner_name: None,
                owner_type_name: None,
                names,
                line,
            }))
        }
        Stmt::Nonlocal(stmt) => {
            let names = stmt.names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Invalidate(InvalidationStatement {
                owner_name: None,
                owner_type_name: None,
                names,
                line,
            }))
        }
        Stmt::Expr(stmt) => extract_call_statement(source, &stmt.value, line),
        _ => None,
    }
}

fn extract_guard_condition(source: &str, expr: &Expr) -> Option<GuardCondition> {
    match expr {
        Expr::UnaryOp(expr) if expr.op == ruff_python_ast::UnaryOp::Not => {
            extract_guard_condition(source, &expr.operand)
                .map(|guard| GuardCondition::Not(Box::new(guard)))
        }
        Expr::BoolOp(expr) => {
            let conditions = expr
                .values
                .iter()
                .map(|value| extract_guard_condition(source, value))
                .collect::<Option<Vec<_>>>()?;
            match expr.op {
                ruff_python_ast::BoolOp::And => Some(GuardCondition::And(conditions)),
                ruff_python_ast::BoolOp::Or => Some(GuardCondition::Or(conditions)),
            }
        }
        Expr::Name(name) => Some(GuardCondition::TruthyName { name: name.id.as_str().to_owned() }),
        Expr::Compare(compare) if compare.ops.len() == 1 && compare.comparators.len() == 1 => {
            let Expr::Name(name) = compare.left.as_ref() else {
                return None;
            };
            let right = compare.comparators.first()?;
            match (compare.ops.first()?, right) {
                (ruff_python_ast::CmpOp::Is, Expr::NoneLiteral(_)) => {
                    Some(GuardCondition::IsNone {
                        name: name.id.as_str().to_owned(),
                        negated: false,
                    })
                }
                (ruff_python_ast::CmpOp::IsNot, Expr::NoneLiteral(_)) => {
                    Some(GuardCondition::IsNone {
                        name: name.id.as_str().to_owned(),
                        negated: true,
                    })
                }
                _ => None,
            }
        }
        Expr::Call(call) => {
            let Expr::Name(callee) = call.func.as_ref() else {
                return None;
            };
            if call.arguments.args.is_empty() {
                return None;
            }
            let Expr::Name(name) = &call.arguments.args[0] else {
                return None;
            };
            if callee.id.as_str() != "isinstance" {
                return Some(GuardCondition::PredicateCall {
                    name: name.id.as_str().to_owned(),
                    callee: callee.id.as_str().to_owned(),
                });
            }
            if call.arguments.args.len() != 2 {
                return None;
            }
            let guard_types = match &call.arguments.args[1] {
                Expr::Tuple(tuple) => tuple
                    .elts
                    .iter()
                    .filter_map(|elt| slice_range(source, elt.range()).map(str::to_owned))
                    .collect::<Vec<_>>(),
                other => slice_range(source, other.range())
                    .map(|text| vec![text.to_owned()])
                    .unwrap_or_default(),
            };
            (!guard_types.is_empty()).then_some(GuardCondition::IsInstance {
                name: name.id.as_str().to_owned(),
                types: guard_types,
            })
        }
        _ => None,
    }
}

fn suite_start_line(source: &str, suite: &[Stmt]) -> usize {
    suite_start_line_optional(source, suite).unwrap_or(0)
}

fn suite_end_line(source: &str, suite: &[Stmt]) -> usize {
    suite_end_line_optional(source, suite).unwrap_or(0)
}

fn suite_start_line_optional(source: &str, suite: &[Stmt]) -> Option<usize> {
    suite.first().map(|stmt| offset_to_line_column(source, stmt.range().start().to_usize()).0)
}

fn suite_end_line_optional(source: &str, suite: &[Stmt]) -> Option<usize> {
    suite.last().map(|stmt| offset_to_line_column(source, stmt.range().end().to_usize()).0)
}

fn if_false_start_line(source: &str, stmt: &ruff_python_ast::StmtIf) -> Option<usize> {
    stmt.elif_else_clauses
        .first()
        .and_then(|clause| suite_start_line_optional(source, &clause.body))
}

fn if_false_end_line(source: &str, stmt: &ruff_python_ast::StmtIf) -> Option<usize> {
    stmt.elif_else_clauses.last().and_then(|clause| suite_end_line_optional(source, &clause.body))
}

fn for_each_if_false_suite(stmt: &ruff_python_ast::StmtIf, mut callback: impl FnMut(&[Stmt])) {
    for clause in &stmt.elif_else_clauses {
        callback(&clause.body);
    }
}

fn for_each_nested_suite(stmt: &Stmt, mut callback: impl FnMut(&[Stmt])) {
    match stmt {
        Stmt::If(if_stmt) => {
            callback(&if_stmt.body);
            for_each_if_false_suite(if_stmt, |suite| callback(suite));
        }
        Stmt::Try(try_stmt) => {
            callback(&try_stmt.body);
            for handler in &try_stmt.handlers {
                let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                callback(&handler.body);
            }
            callback(&try_stmt.orelse);
            callback(&try_stmt.finalbody);
        }
        Stmt::Match(match_stmt) => {
            for case in &match_stmt.cases {
                callback(&case.body);
            }
        }
        Stmt::For(for_stmt) => {
            callback(&for_stmt.body);
            callback(&for_stmt.orelse);
        }
        Stmt::While(while_stmt) => {
            callback(&while_stmt.body);
            callback(&while_stmt.orelse);
        }
        Stmt::With(with_stmt) => callback(&with_stmt.body),
        _ => {}
    }
}

fn extract_call_statement(source: &str, expr: &Expr, line: usize) -> Option<SyntaxStatement> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };

    Some(SyntaxStatement::Call(CallStatement {
        callee: name.id.as_str().to_owned(),
        arg_count: call
            .arguments
            .args
            .iter()
            .filter(|expr| !matches!(expr, Expr::Starred(_)))
            .count(),
        arg_types: call
            .arguments
            .args
            .iter()
            .filter(|expr| !matches!(expr, Expr::Starred(_)))
            .map(infer_literal_arg_type)
            .collect(),
        arg_values: call
            .arguments
            .args
            .iter()
            .filter(|expr| !matches!(expr, Expr::Starred(_)))
            .map(|expr| extract_direct_expr_metadata(source, expr))
            .collect(),
        starred_arg_types: call
            .arguments
            .args
            .iter()
            .filter_map(|expr| match expr {
                Expr::Starred(starred) => Some(infer_literal_arg_type(&starred.value)),
                _ => None,
            })
            .collect(),
        starred_arg_values: call
            .arguments
            .args
            .iter()
            .filter_map(|expr| match expr {
                Expr::Starred(starred) => {
                    Some(extract_direct_expr_metadata(source, &starred.value))
                }
                _ => None,
            })
            .collect(),
        keyword_names: call
            .arguments
            .keywords
            .iter()
            .filter_map(|keyword| keyword.arg.as_ref().map(|name| name.as_str().to_owned()))
            .collect(),
        keyword_arg_types: call
            .arguments
            .keywords
            .iter()
            .filter(|keyword| keyword.arg.is_some())
            .map(|keyword| infer_literal_arg_type(&keyword.value))
            .collect(),
        keyword_arg_values: call
            .arguments
            .keywords
            .iter()
            .filter(|keyword| keyword.arg.is_some())
            .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
            .collect(),
        keyword_expansion_types: call
            .arguments
            .keywords
            .iter()
            .filter(|keyword| keyword.arg.is_none())
            .map(|keyword| infer_literal_arg_type(&keyword.value))
            .collect(),
        keyword_expansion_values: call
            .arguments
            .keywords
            .iter()
            .filter(|keyword| keyword.arg.is_none())
            .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
            .collect(),
        line,
    }))
}

fn extract_method_call_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
) -> Option<SyntaxStatement> {
    let expr = match stmt {
        Stmt::Expr(expr) => expr.value.as_ref(),
        Stmt::Assign(assign) => &assign.value,
        Stmt::AnnAssign(assign) => assign.value.as_deref()?,
        _ => return None,
    };

    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Attribute(attribute) = call.func.as_ref() else {
        return None;
    };

    match attribute.value.as_ref() {
        Expr::Name(name) => Some(SyntaxStatement::MethodCall(MethodCallStatement {
            owner_name: name.id.as_str().to_owned(),
            method: attribute.attr.as_str().to_owned(),
            through_instance: false,
            arg_count: call
                .arguments
                .args
                .iter()
                .filter(|expr| !matches!(expr, Expr::Starred(_)))
                .count(),
            arg_types: call
                .arguments
                .args
                .iter()
                .filter(|expr| !matches!(expr, Expr::Starred(_)))
                .map(infer_literal_arg_type)
                .collect(),
            arg_values: call
                .arguments
                .args
                .iter()
                .filter(|expr| !matches!(expr, Expr::Starred(_)))
                .map(|expr| extract_direct_expr_metadata(source, expr))
                .collect(),
            starred_arg_types: call
                .arguments
                .args
                .iter()
                .filter_map(|expr| match expr {
                    Expr::Starred(starred) => Some(infer_literal_arg_type(&starred.value)),
                    _ => None,
                })
                .collect(),
            starred_arg_values: call
                .arguments
                .args
                .iter()
                .filter_map(|expr| match expr {
                    Expr::Starred(starred) => {
                        Some(extract_direct_expr_metadata(source, &starred.value))
                    }
                    _ => None,
                })
                .collect(),
            keyword_names: call
                .arguments
                .keywords
                .iter()
                .filter_map(|keyword| keyword.arg.as_ref().map(|name| name.as_str().to_owned()))
                .collect(),
            keyword_arg_types: call
                .arguments
                .keywords
                .iter()
                .filter(|keyword| keyword.arg.is_some())
                .map(|keyword| infer_literal_arg_type(&keyword.value))
                .collect(),
            keyword_arg_values: call
                .arguments
                .keywords
                .iter()
                .filter(|keyword| keyword.arg.is_some())
                .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
                .collect(),
            keyword_expansion_types: call
                .arguments
                .keywords
                .iter()
                .filter(|keyword| keyword.arg.is_none())
                .map(|keyword| infer_literal_arg_type(&keyword.value))
                .collect(),
            keyword_expansion_values: call
                .arguments
                .keywords
                .iter()
                .filter(|keyword| keyword.arg.is_none())
                .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
                .collect(),
            line,
        })),
        Expr::Call(inner_call) => {
            let Expr::Name(name) = inner_call.func.as_ref() else {
                return None;
            };
            Some(SyntaxStatement::MethodCall(MethodCallStatement {
                owner_name: name.id.as_str().to_owned(),
                method: attribute.attr.as_str().to_owned(),
                through_instance: true,
                arg_count: call
                    .arguments
                    .args
                    .iter()
                    .filter(|expr| !matches!(expr, Expr::Starred(_)))
                    .count(),
                arg_types: call
                    .arguments
                    .args
                    .iter()
                    .filter(|expr| !matches!(expr, Expr::Starred(_)))
                    .map(infer_literal_arg_type)
                    .collect(),
                arg_values: call
                    .arguments
                    .args
                    .iter()
                    .filter(|expr| !matches!(expr, Expr::Starred(_)))
                    .map(|expr| extract_direct_expr_metadata(source, expr))
                    .collect(),
                starred_arg_types: call
                    .arguments
                    .args
                    .iter()
                    .filter_map(|expr| match expr {
                        Expr::Starred(starred) => Some(infer_literal_arg_type(&starred.value)),
                        _ => None,
                    })
                    .collect(),
                starred_arg_values: call
                    .arguments
                    .args
                    .iter()
                    .filter_map(|expr| match expr {
                        Expr::Starred(starred) => {
                            Some(extract_direct_expr_metadata(source, &starred.value))
                        }
                        _ => None,
                    })
                    .collect(),
                keyword_names: call
                    .arguments
                    .keywords
                    .iter()
                    .filter_map(|keyword| keyword.arg.as_ref().map(|name| name.as_str().to_owned()))
                    .collect(),
                keyword_arg_types: call
                    .arguments
                    .keywords
                    .iter()
                    .filter(|keyword| keyword.arg.is_some())
                    .map(|keyword| infer_literal_arg_type(&keyword.value))
                    .collect(),
                keyword_arg_values: call
                    .arguments
                    .keywords
                    .iter()
                    .filter(|keyword| keyword.arg.is_some())
                    .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
                    .collect(),
                keyword_expansion_types: call
                    .arguments
                    .keywords
                    .iter()
                    .filter(|keyword| keyword.arg.is_none())
                    .map(|keyword| infer_literal_arg_type(&keyword.value))
                    .collect(),
                keyword_expansion_values: call
                    .arguments
                    .keywords
                    .iter()
                    .filter(|keyword| keyword.arg.is_none())
                    .map(|keyword| extract_direct_expr_metadata(source, &keyword.value))
                    .collect(),
                line,
            }))
        }
        _ => None,
    }
}

fn infer_literal_arg_type(expr: &Expr) -> String {
    infer_direct_literal_type(expr).unwrap_or_default()
}

fn infer_direct_literal_type(expr: &Expr) -> Option<String> {
    match expr {
        Expr::NumberLiteral(_) => Some(String::from("int")),
        Expr::StringLiteral(_) => Some(String::from("str")),
        Expr::BooleanLiteral(_) => Some(String::from("bool")),
        Expr::NoneLiteral(_) => Some(String::from("None")),
        Expr::Compare(_) => Some(String::from("bool")),
        Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::Not => {
            Some(String::from("bool"))
        }
        Expr::BoolOp(bool_op) => {
            let mut types =
                bool_op.values.iter().map(infer_direct_literal_type).collect::<Option<Vec<_>>>()?;
            if types.is_empty() {
                None
            } else {
                Some(
                    if bool_op.op == ruff_python_ast::BoolOp::And
                        || bool_op.op == ruff_python_ast::BoolOp::Or
                    {
                        join_union_literal_type_candidates(types)
                    } else {
                        types.remove(0)
                    },
                )
            }
        }
        Expr::BinOp(bin_op) => infer_direct_binop_type(bin_op),
        Expr::List(list) => {
            let element_types =
                list.elts.iter().map(infer_direct_literal_type).collect::<Option<Vec<_>>>()?;
            Some(format!("list[{}]", join_literal_type_candidates(element_types)))
        }
        Expr::Tuple(tuple) => {
            let element_types =
                tuple.elts.iter().map(infer_direct_literal_type).collect::<Option<Vec<_>>>()?;
            Some(if element_types.is_empty() {
                String::from("tuple[()]")
            } else {
                format!("tuple[{}]", element_types.join(", "))
            })
        }
        Expr::Set(set) => {
            let element_types =
                set.elts.iter().map(infer_direct_literal_type).collect::<Option<Vec<_>>>()?;
            Some(format!("set[{}]", join_literal_type_candidates(element_types)))
        }
        Expr::Dict(dict) => {
            let mut key_types = Vec::new();
            let mut value_types = Vec::new();
            for item in &dict.items {
                let key = item.key.as_ref()?;
                key_types.push(infer_direct_literal_type(key)?);
                value_types.push(infer_direct_literal_type(&item.value)?);
            }
            Some(format!(
                "dict[{}, {}]",
                join_literal_type_candidates(key_types),
                join_literal_type_candidates(value_types)
            ))
        }
        _ => None,
    }
}

fn extract_list_comprehension_clauses(
    source: &str,
    generators: &[ruff_python_ast::Comprehension],
) -> Vec<ComprehensionClauseMetadata> {
    generators
        .iter()
        .map(|generator| {
            let target_names = extract_assignment_names(&generator.target);
            ComprehensionClauseMetadata {
                target_name: target_names.first().cloned().unwrap_or_default(),
                target_names,
                iter: Box::new(extract_direct_expr_metadata(source, &generator.iter)),
                filters: generator
                    .ifs
                    .iter()
                    .filter_map(|expr| extract_guard_condition(source, expr))
                    .collect(),
            }
        })
        .collect()
}

fn infer_direct_binop_type(bin_op: &ruff_python_ast::ExprBinOp) -> Option<String> {
    let left = infer_direct_literal_type(&bin_op.left)?;
    let right = infer_direct_literal_type(&bin_op.right)?;
    match bin_op.op {
        ruff_python_ast::Operator::Add => {
            if left == "str" && right == "str" {
                return Some(String::from("str"));
            }
            if is_direct_numeric_type(&left) && is_direct_numeric_type(&right) {
                return Some(join_numeric_type(&left, &right));
            }
            let (left_head, left_args) = split_direct_generic_type(&left)?;
            let (right_head, right_args) = split_direct_generic_type(&right)?;
            match (left_head.as_str(), right_head.as_str()) {
                ("list", "list") if left_args.len() == 1 && right_args.len() == 1 => Some(format!(
                    "list[{}]",
                    join_literal_type_candidates(vec![left_args[0].clone(), right_args[0].clone()])
                )),
                ("tuple", "tuple") => {
                    let mut args = left_args;
                    args.extend(right_args);
                    Some(format!("tuple[{}]", args.join(", ")))
                }
                _ => None,
            }
        }
        ruff_python_ast::Operator::Sub
        | ruff_python_ast::Operator::Mult
        | ruff_python_ast::Operator::Div
        | ruff_python_ast::Operator::FloorDiv
        | ruff_python_ast::Operator::Mod
            if is_direct_numeric_type(&left) && is_direct_numeric_type(&right) =>
        {
            Some(if bin_op.op == ruff_python_ast::Operator::Div {
                String::from("float")
            } else {
                join_numeric_type(&left, &right)
            })
        }
        _ => None,
    }
}

fn is_direct_numeric_type(text: &str) -> bool {
    matches!(text, "int" | "float" | "complex")
}

fn join_numeric_type(left: &str, right: &str) -> String {
    if left == "complex" || right == "complex" {
        String::from("complex")
    } else if left == "float" || right == "float" {
        String::from("float")
    } else {
        String::from("int")
    }
}

fn split_direct_generic_type(text: &str) -> Option<(String, Vec<String>)> {
    let (head, inner) = text.split_once('[')?;
    let inner = inner.strip_suffix(']')?;
    Some((head.to_owned(), inner.split(',').map(|part| part.trim().to_owned()).collect()))
}

fn join_union_literal_type_candidates(types: Vec<String>) -> String {
    let joined = join_literal_type_candidates(types);
    if joined.contains(" | ") {
        format!("Union[{}]", joined.replace(" | ", ", "))
    } else {
        joined
    }
}

fn join_literal_type_candidates(types: Vec<String>) -> String {
    let mut unique = Vec::new();
    for value in types {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    if unique.is_empty() {
        String::from("Any")
    } else {
        unique.join(" | ")
    }
}

fn extract_supplemental_call_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
) -> Option<SyntaxStatement> {
    match stmt {
        Stmt::Assign(assign) => extract_call_statement(source, &assign.value, line),
        Stmt::AnnAssign(assign) => {
            assign.value.as_deref().and_then(|value| extract_call_statement(source, value, line))
        }
        _ => None,
    }
}

fn extract_member_access_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
) -> Option<SyntaxStatement> {
    match stmt {
        Stmt::Expr(expr) => extract_member_access_from_expr(source, &expr.value, line),
        Stmt::Assign(assign) => extract_member_access_from_expr(source, &assign.value, line),
        Stmt::AnnAssign(assign) => assign
            .value
            .as_deref()
            .and_then(|value| extract_member_access_from_expr(source, value, line)),
        _ => None,
    }
}

fn extract_member_access_from_expr(
    _source: &str,
    expr: &Expr,
    line: usize,
) -> Option<SyntaxStatement> {
    let Expr::Attribute(attribute) = expr else {
        return None;
    };

    match attribute.value.as_ref() {
        Expr::Name(name) => Some(SyntaxStatement::MemberAccess(MemberAccessStatement {
            owner_name: name.id.as_str().to_owned(),
            member: attribute.attr.as_str().to_owned(),
            through_instance: false,
            line,
        })),
        Expr::Call(call) => {
            let Expr::Name(name) = call.func.as_ref() else {
                return None;
            };
            Some(SyntaxStatement::MemberAccess(MemberAccessStatement {
                owner_name: name.id.as_str().to_owned(),
                member: attribute.attr.as_str().to_owned(),
                through_instance: true,
                line,
            }))
        }
        _ => None,
    }
}

fn collect_nested_call_statements(
    source: &str,
    suite: &[Stmt],
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_calls_from_suite(source, &function.body, statements);
                collect_nested_call_statements(source, &function.body, statements);
            }
            Stmt::ClassDef(class_def) => {
                collect_calls_from_suite(source, &class_def.body, statements);
                collect_nested_call_statements(source, &class_def.body, statements);
            }
            _ => {
                for_each_nested_suite(stmt, |suite| {
                    collect_calls_from_suite(source, suite, statements);
                    collect_nested_call_statements(source, suite, statements);
                });
            }
        }
    }
}

fn collect_return_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_return_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_return_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                for_each_nested_suite(stmt, |suite| {
                    collect_return_statements(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                });
                let Some(owner_name) = owner_name else {
                    continue;
                };
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(return_statement) =
                    extract_return_statement(source, stmt, line, owner_name, owner_type_name)
                {
                    statements.push(return_statement);
                }
            }
        }
    }
}

fn collect_yield_statements(
    source: &str,
    suite: &[Stmt],
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                for body_stmt in &function.body {
                    let line =
                        offset_to_line_column(source, body_stmt.range().start().to_usize()).0;
                    if let Some(yield_statement) = extract_yield_statement(
                        source,
                        body_stmt,
                        line,
                        function.name.as_str(),
                        owner_type_name,
                    ) {
                        statements.push(yield_statement);
                    }
                }
                collect_yield_statements(source, &function.body, owner_type_name, statements);
            }
            Stmt::ClassDef(class_def) => {
                collect_yield_statements(
                    source,
                    &class_def.body,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::If(if_stmt) => {
                collect_yield_statements(source, &if_stmt.body, owner_type_name, statements);
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_yield_statements(source, suite, owner_type_name, statements);
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_yield_statements(source, &case.body, owner_type_name, statements);
                }
            }
            _ => {}
        }
    }
}

fn collect_if_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_if_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_if_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_if_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_if_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_if_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_if_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            Stmt::If(if_stmt) => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if !statements.iter().any(|statement| statement_line(statement) == line) {
                    statements.push(SyntaxStatement::If(IfStatement {
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        guard: extract_guard_condition(source, &if_stmt.test),
                        line,
                        true_start_line: suite_start_line(source, &if_stmt.body),
                        true_end_line: suite_end_line(source, &if_stmt.body),
                        false_start_line: if_false_start_line(source, if_stmt),
                        false_end_line: if_false_end_line(source, if_stmt),
                    }));
                }
                collect_if_statements(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_if_statements(source, suite, owner_name, owner_type_name, statements);
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_if_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_assert_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_assert_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_assert_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_assert_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_assert_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_assert_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_assert_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            Stmt::If(if_stmt) => {
                collect_assert_statements(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_assert_statements(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_assert_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            Stmt::Assert(assert_stmt) => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if !statements.iter().any(|statement| statement_line(statement) == line) {
                    statements.push(SyntaxStatement::Assert(AssertStatement {
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        guard: extract_guard_condition(source, &assert_stmt.test),
                        line,
                    }));
                }
            }
            _ => {}
        }
    }
}

fn collect_invalidation_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_invalidation_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_invalidation_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_invalidation_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_invalidation_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_invalidation_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_invalidation_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            Stmt::If(if_stmt) => {
                collect_invalidation_statements(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_invalidation_statements(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_invalidation_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            Stmt::AugAssign(stmt) => {
                let names = extract_assignment_names(&stmt.target);
                if !names.is_empty() {
                    let line = offset_to_line_column(source, stmt.range.start().to_usize()).0;
                    statements.push(SyntaxStatement::Invalidate(InvalidationStatement {
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        names,
                        line,
                    }));
                }
            }
            Stmt::Delete(stmt) => {
                let names =
                    stmt.targets.iter().flat_map(extract_assignment_names).collect::<Vec<_>>();
                if !names.is_empty() {
                    let line = offset_to_line_column(source, stmt.range.start().to_usize()).0;
                    statements.push(SyntaxStatement::Invalidate(InvalidationStatement {
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        names,
                        line,
                    }));
                }
            }
            Stmt::Global(stmt) => {
                let names =
                    stmt.names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
                if !names.is_empty() {
                    let line = offset_to_line_column(source, stmt.range.start().to_usize()).0;
                    statements.push(SyntaxStatement::Invalidate(InvalidationStatement {
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        names,
                        line,
                    }));
                }
            }
            Stmt::Nonlocal(stmt) => {
                let names =
                    stmt.names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>();
                if !names.is_empty() {
                    let line = offset_to_line_column(source, stmt.range.start().to_usize()).0;
                    statements.push(SyntaxStatement::Invalidate(InvalidationStatement {
                        owner_name: owner_name.map(str::to_owned),
                        owner_type_name: owner_type_name.map(str::to_owned),
                        names,
                        line,
                    }));
                }
            }
            _ => {}
        }
    }
}

fn collect_match_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_match_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_match_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_match_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_match_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_match_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_match_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            _ => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(match_statement) =
                    extract_match_statement(source, stmt, line, owner_name, owner_type_name)
                {
                    statements.push(match_statement);
                }
            }
        }
    }
}

fn collect_for_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_for_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_for_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_for_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            _ => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(for_statement) =
                    extract_for_statement(source, stmt, line, owner_name, owner_type_name)
                {
                    statements.push(for_statement);
                }
            }
        }
    }
}

fn collect_with_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_with_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_with_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_with_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            _ => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                statements.extend(extract_with_statements(
                    source,
                    stmt,
                    line,
                    owner_name,
                    owner_type_name,
                ));
            }
        }
    }
}

fn collect_except_handler_statements(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_except_handler_statements(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_except_handler_statements(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            Stmt::Try(try_stmt) => {
                for handler in &try_stmt.handlers {
                    let line = offset_to_line_column(source, handler.range().start().to_usize()).0;
                    if let Some(statement) = extract_except_handler_statement(
                        source,
                        handler,
                        line,
                        owner_name,
                        owner_type_name,
                    ) {
                        statements.push(statement);
                    }
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_except_handler_statements(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
                collect_except_handler_statements(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_except_handler_statements(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    statements,
                );
                collect_except_handler_statements(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    statements,
                );
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_except_handler_statements(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        statements,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_function_body_assignments(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_function_body_assignments(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_function_body_assignments(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                let Some(owner_name) = owner_name else {
                    continue;
                };
                if let Some(assignment) = extract_function_body_assignment_statement(
                    source,
                    stmt,
                    line,
                    owner_name,
                    owner_type_name,
                ) {
                    statements.push(assignment);
                }
                for_each_nested_suite(stmt, |suite| {
                    collect_function_body_assignments(
                        source,
                        suite,
                        Some(owner_name),
                        owner_type_name,
                        statements,
                    )
                });
            }
        }
    }
}

fn collect_function_body_bare_assignments(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_function_body_bare_assignments(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_function_body_bare_assignments(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                let Some(owner_name) = owner_name else {
                    continue;
                };
                let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
                if let Some(assignment) = extract_function_body_bare_assignment_statement(
                    source,
                    stmt,
                    line,
                    owner_name,
                    owner_type_name,
                ) {
                    statements.push(assignment);
                }
                for_each_nested_suite(stmt, |suite| {
                    collect_function_body_bare_assignments(
                        source,
                        suite,
                        Some(owner_name),
                        owner_type_name,
                        statements,
                    )
                });
            }
        }
    }
}

fn collect_function_body_namedexpr_assignments(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    statements: &mut Vec<SyntaxStatement>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_function_body_namedexpr_assignments(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    statements,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_function_body_namedexpr_assignments(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    statements,
                );
            }
            _ => {
                let Some(owner_name) = owner_name else {
                    continue;
                };
                let mut collector = NamedExprAssignmentCollector {
                    source,
                    owner_name,
                    owner_type_name,
                    statements: Vec::new(),
                };
                collector.visit_stmt(stmt);
                statements.extend(collector.statements);
            }
        }
    }
}

struct NamedExprAssignmentCollector<'a> {
    source: &'a str,
    owner_name: &'a str,
    owner_type_name: Option<&'a str>,
    statements: Vec<SyntaxStatement>,
}

impl<'a> NamedExprAssignmentCollector<'a> {
    fn push_namedexpr_assignment(&mut self, named_expr: &ruff_python_ast::ExprNamed) {
        let Expr::Name(name) = named_expr.target.as_ref() else {
            return;
        };
        let line = offset_to_line_column(self.source, named_expr.range().start().to_usize()).0;
        let value = extract_direct_expr_metadata(self.source, &named_expr.value);
        self.statements.push(SyntaxStatement::Value(ValueStatement {
            names: vec![name.id.as_str().to_owned()],
            destructuring_target_names: None,
            annotation: None,
            value_type: value.value_type,
            is_awaited: value.is_awaited,
            value_callee: value.value_callee,
            value_name: value.value_name,
            value_member_owner_name: value.value_member_owner_name,
            value_member_name: value.value_member_name,
            value_member_through_instance: value.value_member_through_instance,
            value_method_owner_name: value.value_method_owner_name,
            value_method_name: value.value_method_name,
            value_method_through_instance: value.value_method_through_instance,
            value_subscript_target: value.value_subscript_target,
            value_subscript_string_key: value.value_subscript_string_key,
            value_subscript_index: value.value_subscript_index,
            value_if_true: value.value_if_true,
            value_if_false: value.value_if_false,
            value_if_guard: value.value_if_guard,
            value_bool_left: value.value_bool_left,
            value_bool_right: value.value_bool_right,
            value_binop_left: value.value_binop_left,
            value_binop_right: value.value_binop_right,
            value_binop_operator: value.value_binop_operator,
            value_lambda: value.value_lambda,
            value_list_comprehension: value.value_list_comprehension,
            value_generator_comprehension: value.value_generator_comprehension,
            value_list_elements: value.value_list_elements,
            value_set_elements: value.value_set_elements,
            value_dict_entries: value.value_dict_entries,
            owner_name: Some(self.owner_name.to_owned()),
            owner_type_name: self.owner_type_name.map(str::to_owned),
            is_final: false,
            is_class_var: false,
            line,
        }));
    }
}

impl<'a> visitor::Visitor<'a> for NamedExprAssignmentCollector<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
            return;
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_)
            | Expr::ListComp(_)
            | Expr::SetComp(_)
            | Expr::DictComp(_)
            | Expr::Generator(_) => return,
            Expr::Named(named_expr) => {
                self.push_namedexpr_assignment(named_expr);
                self.visit_expr(&named_expr.value);
                return;
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

fn extract_function_body_assignment_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::AnnAssign(assign) = stmt else {
        return None;
    };
    let names = extract_assignment_names(&assign.target);
    if names.is_empty() {
        return None;
    }
    let value =
        assign.value.as_deref().map(|expr| extract_direct_expr_metadata(source, expr)).unwrap_or(
            DirectExprMetadata {
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
            },
        );
    Some(SyntaxStatement::Value(ValueStatement {
        names,
        destructuring_target_names: None,
        annotation: slice_range(source, assign.annotation.range()).map(str::to_owned),
        value_type: value.value_type,
        is_awaited: value.is_awaited,
        value_callee: value.value_callee,
        value_name: value.value_name,
        value_member_owner_name: value.value_member_owner_name,
        value_member_name: value.value_member_name,
        value_member_through_instance: value.value_member_through_instance,
        value_method_owner_name: value.value_method_owner_name,
        value_method_name: value.value_method_name,
        value_method_through_instance: value.value_method_through_instance,
        value_subscript_target: value.value_subscript_target,
        value_subscript_string_key: value.value_subscript_string_key,
        value_subscript_index: value.value_subscript_index,
        value_if_true: value.value_if_true,
        value_if_false: value.value_if_false,
        value_if_guard: value.value_if_guard,
        value_bool_left: value.value_bool_left,
        value_bool_right: value.value_bool_right,
        value_binop_left: value.value_binop_left,
        value_binop_right: value.value_binop_right,
        value_binop_operator: value.value_binop_operator,
        value_lambda: value.value_lambda,
        value_list_comprehension: value.value_list_comprehension,
        value_generator_comprehension: value.value_generator_comprehension,
        value_list_elements: value.value_list_elements,
        value_set_elements: value.value_set_elements,
        value_dict_entries: value.value_dict_entries,
        owner_name: Some(owner_name.to_owned()),
        owner_type_name: owner_type_name.map(str::to_owned),
        is_final: is_final_annotation(&assign.annotation),
        is_class_var: is_classvar_annotation(&assign.annotation),
        line,
    }))
}

fn extract_function_body_bare_assignment_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::Assign(assign) = stmt else {
        return None;
    };
    let destructuring_target_names = (assign.targets.len() == 1)
        .then(|| extract_simple_destructuring_target_names(&assign.targets[0]))
        .flatten();
    let names = destructuring_target_names.clone().unwrap_or_else(|| {
        assign.targets.iter().flat_map(extract_assignment_names).collect::<Vec<_>>()
    });
    if names.is_empty() {
        return None;
    }
    let value = extract_direct_expr_metadata(source, &assign.value);
    Some(SyntaxStatement::Value(ValueStatement {
        names,
        destructuring_target_names,
        annotation: None,
        value_type: value.value_type,
        is_awaited: value.is_awaited,
        value_callee: value.value_callee,
        value_name: value.value_name,
        value_member_owner_name: value.value_member_owner_name,
        value_member_name: value.value_member_name,
        value_member_through_instance: value.value_member_through_instance,
        value_method_owner_name: None,
        value_method_name: None,
        value_method_through_instance: false,
        value_subscript_target: value.value_subscript_target,
        value_subscript_string_key: value.value_subscript_string_key,
        value_subscript_index: value.value_subscript_index,
        value_if_true: value.value_if_true,
        value_if_false: value.value_if_false,
        value_if_guard: value.value_if_guard,
        value_bool_left: value.value_bool_left,
        value_bool_right: value.value_bool_right,
        value_binop_left: value.value_binop_left,
        value_binop_right: value.value_binop_right,
        value_binop_operator: value.value_binop_operator,
        value_lambda: None,
        value_list_comprehension: None,
        value_generator_comprehension: None,
        value_list_elements: value.value_list_elements,
        value_set_elements: value.value_set_elements,
        value_dict_entries: value.value_dict_entries,
        owner_name: Some(owner_name.to_owned()),
        owner_type_name: owner_type_name.map(str::to_owned),
        is_final: false,
        is_class_var: false,
        line,
    }))
}

fn extract_yield_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::Expr(expr_stmt) = stmt else {
        return None;
    };

    let (value, is_yield_from) = match expr_stmt.value.as_ref() {
        Expr::Yield(yield_expr) => (
            yield_expr
                .value
                .as_deref()
                .map(|expr| extract_direct_expr_metadata(source, expr))
                .unwrap_or(DirectExprMetadata {
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
                }),
            false,
        ),
        Expr::YieldFrom(yield_expr) => {
            (extract_direct_expr_metadata(source, &yield_expr.value), true)
        }
        _ => return None,
    };

    Some(SyntaxStatement::Yield(YieldStatement {
        owner_name: owner_name.to_owned(),
        owner_type_name: owner_type_name.map(str::to_owned),
        value_type: value.value_type,
        value_callee: value.value_callee,
        value_name: value.value_name,
        value_member_owner_name: value.value_member_owner_name,
        value_member_name: value.value_member_name,
        value_member_through_instance: value.value_member_through_instance,
        value_method_owner_name: value.value_method_owner_name,
        value_method_name: value.value_method_name,
        value_method_through_instance: value.value_method_through_instance,
        value_subscript_target: value.value_subscript_target,
        value_subscript_string_key: value.value_subscript_string_key,
        value_subscript_index: value.value_subscript_index,
        value_if_true: value.value_if_true,
        value_if_false: value.value_if_false,
        value_if_guard: value.value_if_guard,
        value_bool_left: value.value_bool_left,
        value_bool_right: value.value_bool_right,
        value_binop_left: value.value_binop_left,
        value_binop_right: value.value_binop_right,
        value_binop_operator: value.value_binop_operator,
        value_lambda: value.value_lambda,
        value_list_elements: value.value_list_elements,
        value_set_elements: value.value_set_elements,
        value_dict_entries: value.value_dict_entries,
        is_yield_from,
        line,
    }))
}

fn extract_match_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::Match(match_stmt) = stmt else {
        return None;
    };
    let subject = extract_direct_expr_metadata(source, &match_stmt.subject);
    Some(SyntaxStatement::Match(MatchStatement {
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        subject_type: subject.value_type,
        subject_is_awaited: subject.is_awaited,
        subject_callee: subject.value_callee,
        subject_name: subject.value_name,
        subject_member_owner_name: subject.value_member_owner_name,
        subject_member_name: subject.value_member_name,
        subject_member_through_instance: subject.value_member_through_instance,
        subject_method_owner_name: subject.value_method_owner_name,
        subject_method_name: subject.value_method_name,
        subject_method_through_instance: subject.value_method_through_instance,
        cases: match_stmt
            .cases
            .iter()
            .map(|case| MatchCaseStatement {
                patterns: extract_match_patterns(source, &case.pattern),
                has_guard: case.guard.is_some(),
                line: offset_to_line_column(source, case.range.start().to_usize()).0,
            })
            .collect(),
        line,
    }))
}

fn extract_match_patterns(source: &str, pattern: &ruff_python_ast::Pattern) -> Vec<MatchPattern> {
    use ruff_python_ast::Pattern;

    if pattern.is_wildcard() {
        return vec![MatchPattern::Wildcard];
    }

    match pattern {
        Pattern::MatchOr(pattern) => pattern
            .patterns
            .iter()
            .flat_map(|pattern| extract_match_patterns(source, pattern))
            .collect(),
        Pattern::MatchClass(pattern) => slice_range(source, pattern.cls.range())
            .map(|text| vec![MatchPattern::Class(text.to_owned())])
            .unwrap_or_else(|| vec![MatchPattern::Unsupported]),
        Pattern::MatchValue(pattern) => slice_range(source, pattern.value.range())
            .map(|text| vec![MatchPattern::Literal(text.to_owned())])
            .unwrap_or_else(|| vec![MatchPattern::Unsupported]),
        Pattern::MatchSingleton(pattern) => slice_range(source, pattern.range())
            .map(|text| vec![MatchPattern::Literal(text.to_owned())])
            .unwrap_or_else(|| vec![MatchPattern::Unsupported]),
        _ => vec![MatchPattern::Unsupported],
    }
}

fn extract_for_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::For(for_stmt) = stmt else {
        return None;
    };
    if for_stmt.is_async {
        return None;
    }
    let (target_name, target_names) = match &*for_stmt.target {
        Expr::Name(name) => (name.id.as_str().to_owned(), Vec::new()),
        Expr::Tuple(tuple) => {
            let names = tuple
                .elts
                .iter()
                .map(|elt| {
                    let Expr::Name(name) = elt else {
                        return None;
                    };
                    Some(name.id.as_str().to_owned())
                })
                .collect::<Option<Vec<_>>>()?;
            (String::new(), names)
        }
        _ => return None,
    };
    let iter = extract_direct_expr_metadata(source, &for_stmt.iter);
    Some(SyntaxStatement::For(ForStatement {
        target_name,
        target_names,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        iter_type: iter.value_type,
        iter_is_awaited: iter.is_awaited,
        iter_callee: iter.value_callee,
        iter_name: iter.value_name,
        iter_member_owner_name: iter.value_member_owner_name,
        iter_member_name: iter.value_member_name,
        iter_member_through_instance: iter.value_member_through_instance,
        iter_method_owner_name: iter.value_method_owner_name,
        iter_method_name: iter.value_method_name,
        iter_method_through_instance: iter.value_method_through_instance,
        line,
    }))
}

fn extract_with_statements(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Vec<SyntaxStatement> {
    let Stmt::With(with_stmt) = stmt else {
        return Vec::new();
    };
    if with_stmt.is_async {
        return Vec::new();
    }
    with_stmt
        .items
        .iter()
        .filter_map(|item| {
            let target_name = match item.optional_vars.as_deref() {
                Some(Expr::Name(name)) => Some(name.id.as_str().to_owned()),
                Some(_) => return None,
                None => None,
            };
            let context = extract_direct_expr_metadata(source, &item.context_expr);
            Some(SyntaxStatement::With(WithStatement {
                target_name,
                owner_name: owner_name.map(str::to_owned),
                owner_type_name: owner_type_name.map(str::to_owned),
                context_type: context.value_type,
                context_is_awaited: context.is_awaited,
                context_callee: context.value_callee,
                context_name: context.value_name,
                context_member_owner_name: context.value_member_owner_name,
                context_member_name: context.value_member_name,
                context_member_through_instance: context.value_member_through_instance,
                context_method_owner_name: context.value_method_owner_name,
                context_method_name: context.value_method_name,
                context_method_through_instance: context.value_method_through_instance,
                line,
            }))
        })
        .collect()
}

fn extract_except_handler_statement(
    source: &str,
    handler: &ruff_python_ast::ExceptHandler,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
    Some(SyntaxStatement::ExceptHandler(ExceptionHandlerStatement {
        exception_type: handler
            .type_
            .as_ref()
            .and_then(|expr| slice_range(source, expr.range()))
            .map(str::to_owned)
            .unwrap_or_else(|| String::from("BaseException")),
        binding_name: handler.name.as_ref().map(|name| name.as_str().to_owned()),
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        line,
        end_line: offset_to_line_column(source, handler.range.end().to_usize()).0,
    }))
}

fn collect_calls_from_suite(source: &str, suite: &[Stmt], statements: &mut Vec<SyntaxStatement>) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        if let Some(call) =
            extract_supplemental_call_statement(source, stmt, line).or_else(|| match stmt {
                Stmt::Expr(expr) => extract_call_statement(source, &expr.value, line),
                _ => None,
            })
        {
            statements.push(call);
        }
    }
}

fn extract_return_statement(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: &str,
    owner_type_name: Option<&str>,
) -> Option<SyntaxStatement> {
    let Stmt::Return(return_stmt) = stmt else {
        return None;
    };

    let value = return_stmt
        .value
        .as_deref()
        .map(|expr| extract_direct_expr_metadata(source, expr))
        .unwrap_or(DirectExprMetadata {
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
        });

    Some(SyntaxStatement::Return(ReturnStatement {
        owner_name: owner_name.to_owned(),
        owner_type_name: owner_type_name.map(str::to_owned),
        value_type: value.value_type,
        is_awaited: value.is_awaited,
        value_callee: value.value_callee,
        value_name: value.value_name,
        value_member_owner_name: value.value_member_owner_name,
        value_member_name: value.value_member_name,
        value_member_through_instance: value.value_member_through_instance,
        value_method_owner_name: value.value_method_owner_name,
        value_method_name: value.value_method_name,
        value_method_through_instance: value.value_method_through_instance,
        value_subscript_target: value.value_subscript_target,
        value_subscript_string_key: value.value_subscript_string_key,
        value_subscript_index: value.value_subscript_index,
        value_if_true: value.value_if_true,
        value_if_false: value.value_if_false,
        value_if_guard: value.value_if_guard,
        value_bool_left: value.value_bool_left,
        value_bool_right: value.value_bool_right,
        value_binop_left: value.value_binop_left,
        value_binop_right: value.value_binop_right,
        value_binop_operator: value.value_binop_operator,
        value_lambda: value.value_lambda,
        value_list_elements: value.value_list_elements,
        value_set_elements: value.value_set_elements,
        value_dict_entries: value.value_dict_entries,
        line,
    }))
}

fn extract_direct_expr_metadata(source: &str, expr: &Expr) -> DirectExprMetadata {
    if let Expr::Await(await_expr) = expr {
        let mut metadata = extract_direct_expr_metadata(source, &await_expr.value);
        metadata.is_awaited = true;
        return metadata;
    }

    if let Expr::Named(named_expr) = expr {
        return extract_direct_expr_metadata(source, &named_expr.value);
    }

    if let Expr::Dict(dict) = expr {
        return DirectExprMetadata {
            value_type: Some(infer_literal_arg_type(expr)),
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
            value_dict_entries: Some(extract_typed_dict_literal_entries(source, dict)),
        };
    }

    if let Expr::List(list) = expr {
        return DirectExprMetadata {
            value_type: Some(infer_literal_arg_type(expr)),
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
            value_list_elements: Some(
                list.elts.iter().map(|item| extract_direct_expr_metadata(source, item)).collect(),
            ),
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::Set(set) = expr {
        return DirectExprMetadata {
            value_type: Some(infer_literal_arg_type(expr)),
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
            value_set_elements: Some(
                set.elts.iter().map(|item| extract_direct_expr_metadata(source, item)).collect(),
            ),
            value_dict_entries: None,
        };
    }

    if let Expr::Lambda(lambda) = expr {
        let param_names = lambda
            .parameters
            .iter()
            .flat_map(|parameters| parameters.iter_non_variadic_params())
            .map(|param| param.name().as_str().to_owned())
            .collect::<Vec<_>>();
        return DirectExprMetadata {
            value_type: Some(String::new()),
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
            value_lambda: Some(Box::new(LambdaMetadata {
                param_names,
                body: Box::new(extract_direct_expr_metadata(source, &lambda.body)),
            })),
            value_list_comprehension: None,
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::ListComp(comp) = expr {
        return DirectExprMetadata {
            value_type: Some(String::new()),
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
            value_list_comprehension: Some(Box::new(ComprehensionMetadata {
                kind: ComprehensionKind::List,
                clauses: extract_list_comprehension_clauses(source, &comp.generators),
                key: None,
                element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
            })),
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::SetComp(comp) = expr {
        return DirectExprMetadata {
            value_type: Some(String::new()),
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
            value_list_comprehension: Some(Box::new(ComprehensionMetadata {
                kind: ComprehensionKind::Set,
                clauses: extract_list_comprehension_clauses(source, &comp.generators),
                key: None,
                element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
            })),
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::DictComp(comp) = expr {
        return DirectExprMetadata {
            value_type: Some(String::new()),
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
            value_list_comprehension: Some(Box::new(ComprehensionMetadata {
                kind: ComprehensionKind::Dict,
                clauses: extract_list_comprehension_clauses(source, &comp.generators),
                key: Some(Box::new(extract_direct_expr_metadata(source, &comp.key))),
                element: Box::new(extract_direct_expr_metadata(source, &comp.value)),
            })),
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::Generator(comp) = expr {
        return DirectExprMetadata {
            value_type: Some(String::new()),
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
            value_generator_comprehension: Some(Box::new(ComprehensionMetadata {
                kind: ComprehensionKind::Generator,
                clauses: extract_list_comprehension_clauses(source, &comp.generators),
                key: None,
                element: Box::new(extract_direct_expr_metadata(source, &comp.elt)),
            })),
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Some((owner_name, method_name, through_instance)) = extract_direct_method_call(expr) {
        return DirectExprMetadata {
            value_type: Some(infer_literal_arg_type(expr)),
            is_awaited: false,
            value_callee: None,
            value_name: None,
            value_member_owner_name: None,
            value_member_name: None,
            value_member_through_instance: false,
            value_method_owner_name: Some(owner_name),
            value_method_name: Some(method_name),
            value_method_through_instance: through_instance,
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
        };
    }

    if let Expr::BoolOp(bool_op) = expr {
        let mut values = bool_op.values.iter();
        let left_expr = values.next();
        let left_guard = left_expr.and_then(|expr| extract_guard_condition(source, expr));
        let left = left_expr.map(|expr| extract_direct_expr_metadata(source, expr));
        let right = values.next().map(|expr| extract_direct_expr_metadata(source, expr));
        return DirectExprMetadata {
            value_type: Some(String::new()),
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
            value_if_guard: left_guard,
            value_bool_left: left.map(Box::new),
            value_bool_right: right.map(Box::new),
            value_binop_left: None,
            value_binop_right: None,
            value_binop_operator: Some(match bool_op.op {
                ruff_python_ast::BoolOp::And => String::from("and"),
                ruff_python_ast::BoolOp::Or => String::from("or"),
            }),
            value_lambda: None,
            value_list_comprehension: None,
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::BinOp(bin_op) = expr {
        return DirectExprMetadata {
            value_type: Some(String::new()),
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
            value_binop_left: Some(Box::new(extract_direct_expr_metadata(source, &bin_op.left))),
            value_binop_right: Some(Box::new(extract_direct_expr_metadata(source, &bin_op.right))),
            value_binop_operator: Some(match bin_op.op {
                ruff_python_ast::Operator::Add => String::from("+"),
                ruff_python_ast::Operator::Sub => String::from("-"),
                ruff_python_ast::Operator::Mult => String::from("*"),
                ruff_python_ast::Operator::Div => String::from("/"),
                ruff_python_ast::Operator::FloorDiv => String::from("//"),
                ruff_python_ast::Operator::Mod => String::from("%"),
                _ => String::new(),
            }),
            value_lambda: None,
            value_list_comprehension: None,
            value_generator_comprehension: None,
            value_list_elements: None,
            value_set_elements: None,
            value_dict_entries: None,
        };
    }

    if let Expr::If(if_expr) = expr {
        return DirectExprMetadata {
            value_type: Some(infer_literal_arg_type(expr)),
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
            value_if_true: Some(Box::new(extract_direct_expr_metadata(source, &if_expr.body))),
            value_if_false: Some(Box::new(extract_direct_expr_metadata(source, &if_expr.orelse))),
            value_if_guard: extract_guard_condition(source, &if_expr.test),
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
        };
    }

    if let Expr::Subscript(subscript) = expr {
        return DirectExprMetadata {
            value_type: Some(infer_literal_arg_type(expr)),
            is_awaited: false,
            value_callee: None,
            value_name: None,
            value_member_owner_name: None,
            value_member_name: None,
            value_member_through_instance: false,
            value_method_owner_name: None,
            value_method_name: None,
            value_method_through_instance: false,
            value_subscript_target: Some(Box::new(extract_direct_expr_metadata(
                source,
                &subscript.value,
            ))),
            value_subscript_string_key: extract_string_literal_value(source, &subscript.slice),
            value_subscript_index: match infer_literal_arg_type(&subscript.slice).as_str() {
                "int" => slice_range(source, subscript.slice.range()).map(str::to_owned),
                _ => None,
            },
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
        };
    }

    let member = extract_direct_member_access(expr);
    DirectExprMetadata {
        value_type: Some(infer_literal_arg_type(expr)),
        is_awaited: false,
        value_callee: extract_direct_callee(expr),
        value_name: extract_direct_name(expr),
        value_member_owner_name: member.as_ref().map(|(owner_name, _, _)| owner_name.clone()),
        value_member_name: member.as_ref().map(|(_, member, _)| member.clone()),
        value_member_through_instance: member
            .as_ref()
            .map(|(_, _, through_instance)| *through_instance)
            .unwrap_or(false),
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
    }
}

fn extract_direct_method_call(expr: &Expr) -> Option<(String, String, bool)> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Attribute(attribute) = call.func.as_ref() else {
        return None;
    };

    match attribute.value.as_ref() {
        Expr::Name(name) => {
            Some((name.id.as_str().to_owned(), attribute.attr.as_str().to_owned(), false))
        }
        Expr::Call(inner_call) => {
            let Expr::Name(name) = inner_call.func.as_ref() else {
                return None;
            };
            Some((name.id.as_str().to_owned(), attribute.attr.as_str().to_owned(), true))
        }
        _ => None,
    }
}

fn extract_direct_callee(expr: &Expr) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };
    Some(name.id.as_str().to_owned())
}

fn extract_direct_name(expr: &Expr) -> Option<String> {
    let Expr::Name(name) = expr else {
        return None;
    };
    Some(name.id.as_str().to_owned())
}

fn extract_direct_member_access(expr: &Expr) -> Option<(String, String, bool)> {
    let Expr::Attribute(attribute) = expr else {
        return None;
    };

    match attribute.value.as_ref() {
        Expr::Name(name) => {
            Some((name.id.as_str().to_owned(), attribute.attr.as_str().to_owned(), false))
        }
        Expr::Call(call) => {
            let Expr::Name(name) = call.func.as_ref() else {
                return None;
            };
            Some((name.id.as_str().to_owned(), attribute.attr.as_str().to_owned(), true))
        }
        _ => None,
    }
}

fn extract_ast_type_params(
    path: &Path,
    source: &str,
    type_params: Option<&ruff_python_ast::TypeParams>,
    line: usize,
    label: &str,
    diagnostics: &mut DiagnosticReport,
) -> Option<Vec<TypeParam>> {
    let mut parsed = Vec::new();

    for type_param in type_params.into_iter().flat_map(|type_params| type_params.iter()) {
        match type_param {
            AstTypeParam::TypeVar(type_var) => {
                if type_var.default.is_some() {
                    diagnostics.push(
                        Diagnostic::error(
                            "TPY4010",
                            format!("{label} uses deferred-beyond-v1 type parameter defaults"),
                        )
                        .with_span(Span::new(
                            path.display().to_string(),
                            line,
                            1,
                            line,
                            1,
                        )),
                    );
                    return None;
                }
                parsed.push(TypeParam {
                    name: type_var.name.as_str().to_owned(),
                    bound: type_var
                        .bound
                        .as_ref()
                        .and_then(|bound| slice_range(source, bound.range()))
                        .map(str::to_owned),
                });
            }
            AstTypeParam::TypeVarTuple(_) | AstTypeParam::ParamSpec(_) => {
                diagnostics.push(
                    Diagnostic::error(
                        "TPY4010",
                        format!("{label} uses deferred-beyond-v1 type parameter syntax"),
                    )
                    .with_span(Span::new(
                        path.display().to_string(),
                        line,
                        1,
                        line,
                        1,
                    )),
                );
                return None;
            }
        }
    }

    let mut seen = std::collections::BTreeSet::new();
    for type_param in &parsed {
        if !seen.insert(type_param.name.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4004",
                    format!("{label} declares type parameter `{}` more than once", type_param.name),
                )
                .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
            );
            return None;
        }
    }

    Some(parsed)
}

fn extract_function_params(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
) -> Vec<FunctionParam> {
    let positional_only = parameters.posonlyargs.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: parameter.default().is_some(),
        positional_only: true,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    });
    let positional = parameters.args.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: parameter.default().is_some(),
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    });
    let variadic = parameters.vararg.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: true,
        keyword_variadic: false,
    });
    let keyword_only = parameters.kwonlyargs.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: parameter.default().is_some(),
        positional_only: false,
        keyword_only: true,
        variadic: false,
        keyword_variadic: false,
    });
    let keyword_variadic = parameters.kwarg.iter().map(|parameter| FunctionParam {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: true,
    });

    positional_only
        .chain(positional)
        .chain(variadic)
        .chain(keyword_only)
        .chain(keyword_variadic)
        .collect()
}

fn extract_class_bases(source: &str, arguments: &ruff_python_ast::Arguments) -> Vec<String> {
    arguments
        .args
        .iter()
        .filter_map(|argument| slice_range(source, argument.range()).map(str::to_owned))
        .collect()
}

fn extract_assignment_names(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Name(name) => vec![name.id.as_str().to_owned()],
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(extract_assignment_names).collect(),
        Expr::List(list) => list.elts.iter().flat_map(extract_assignment_names).collect(),
        Expr::Starred(starred) => extract_assignment_names(&starred.value),
        _ => Vec::new(),
    }
}

fn extract_simple_destructuring_target_names(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::Tuple(tuple) => tuple
            .elts
            .iter()
            .map(|element| match element {
                Expr::Name(name) => Some(name.id.as_str().to_owned()),
                _ => None,
            })
            .collect(),
        Expr::List(list) => list
            .elts
            .iter()
            .map(|element| match element {
                Expr::Name(name) => Some(name.id.as_str().to_owned()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn is_overload_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "overload",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "overload"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "typing")
        }
        _ => false,
    }
}

fn is_override_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "override",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "override"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        _ => false,
    }
}

fn is_abstractmethod_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "abstractmethod",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "abstractmethod"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "abc")
        }
        _ => false,
    }
}

fn is_final_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "final",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "final"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        _ => false,
    }
}

fn deprecated_decorator_message(decorators: &[ruff_python_ast::Decorator]) -> Option<String> {
    decorators.iter().find_map(deprecated_decorator_arg)
}

fn deprecated_decorator_arg(decorator: &ruff_python_ast::Decorator) -> Option<String> {
    match &decorator.expression {
        Expr::Name(name) if name.id.as_str() == "deprecated" => Some(String::new()),
        Expr::Attribute(attribute)
            if attribute.attr.as_str() == "deprecated"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing_extensions" | "warnings")) =>
        {
            Some(String::new())
        }
        Expr::Call(call) => {
            let target = match &*call.func {
                Expr::Name(name) => name.id.as_str() == "deprecated",
                Expr::Attribute(attribute) => {
                    attribute.attr.as_str() == "deprecated"
                        && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing_extensions" | "warnings"))
                }
                _ => false,
            };
            if !target {
                return None;
            }
            call.arguments
                .args
                .first()
                .and_then(|arg| match arg {
                    Expr::StringLiteral(string) => Some(string.value.to_str().to_owned()),
                    _ => None,
                })
                .or(Some(String::new()))
        }
        _ => None,
    }
}

fn method_kind_from_decorators(decorators: &[ruff_python_ast::Decorator]) -> MethodKind {
    for decorator in decorators {
        match &decorator.expression {
            Expr::Name(name) if name.id.as_str() == "classmethod" => return MethodKind::Class,
            Expr::Name(name) if name.id.as_str() == "staticmethod" => return MethodKind::Static,
            Expr::Name(name) if name.id.as_str() == "property" => return MethodKind::Property,
            _ => {}
        }
    }

    MethodKind::Instance
}

fn is_abstract_class(statement: &NamedBlockStatement) -> bool {
    statement.bases.iter().any(|base| matches!(base.as_str(), "ABC" | "abc.ABC"))
        || statement.members.iter().any(|member| member.is_abstract_method)
}

fn is_final_annotation(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "Final",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "Final"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        Expr::Subscript(subscript) => is_final_annotation(&subscript.value),
        _ => false,
    }
}

fn is_classvar_annotation(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "ClassVar",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "ClassVar"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        Expr::Subscript(subscript) => is_classvar_annotation(&subscript.value),
        _ => false,
    }
}

fn normalize_import_module(
    path: &Path,
    current_module_key: &str,
    level: u32,
    module: &str,
) -> String {
    if level == 0 {
        return module.to_owned();
    }

    let mut parts: Vec<_> = current_module_key.split('.').filter(|part| !part.is_empty()).collect();
    if path.file_stem().and_then(|stem| stem.to_str()) != Some("__init__") {
        parts.pop();
    }
    for _ in 1..level {
        parts.pop();
    }
    if !module.is_empty() {
        parts.extend(module.split('.'));
    }
    parts.join(".")
}

fn slice_range(source: &str, range: ruff_text_size::TextRange) -> Option<&str> {
    source.get(range.start().to_usize()..range.end().to_usize())
}

fn parse_extension_statement(
    path: &Path,
    trimmed_line: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    if let Some(rest) = strip_soft_keyword(trimmed_line, "typealias") {
        return parse_typealias(path, trimmed_line, rest, line_number, diagnostics);
    }
    if let Some(rest) = strip_soft_keyword(trimmed_line, "interface") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "interface declaration",
            SyntaxStatement::Interface,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("data class ") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "data class declaration",
            SyntaxStatement::DataClass,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("sealed class ") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "sealed class declaration",
            SyntaxStatement::SealedClass,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("overload def ") {
        return parse_overload(path, trimmed_line, rest, line_number, diagnostics);
    }
    if trimmed_line.starts_with("unsafe") {
        return parse_unsafe(path, trimmed_line, line_number, diagnostics);
    }

    None
}

fn strip_soft_keyword<'source>(line: &'source str, keyword: &str) -> Option<&'source str> {
    let rest = line.strip_prefix(keyword)?;
    match rest.chars().next() {
        Some(character) if character == '_' || character.is_ascii_alphanumeric() => None,
        _ => Some(rest),
    }
}

fn parse_typealias(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    let (head, tail) = match split_top_level_once(rest, '=') {
        Some(parts) => parts,
        None => {
            diagnostics.push(parse_error(
                path,
                line_number,
                line,
                "typealias declaration must contain `=`",
            ));
            return None;
        }
    };

    if tail.trim().is_empty() {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "typealias declaration must define a target type expression",
        ));
        return None;
    }

    let Some((name, suffix)) = extract_decl_head(head) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "typealias declaration must name an alias before `=`",
        ));
        return None;
    };
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, "typealias declaration")?;

    Some(SyntaxStatement::TypeAlias(TypeAliasStatement {
        name,
        type_params: parsed_type_params.type_params,
        value: tail.trim().to_owned(),
        line: line_number,
    }))
}

fn parse_named_block(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
    label: &str,
    constructor: fn(NamedBlockStatement) -> SyntaxStatement,
) -> Option<SyntaxStatement> {
    if !line.ends_with(':') {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must end with `:`"),
        ));
        return None;
    }

    let header = &rest[..rest.len().saturating_sub(1)].trim_end();
    let Some((name, suffix)) = extract_decl_head(header) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a valid name"),
        ));
        return None;
    };
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, label)?;

    Some(constructor(NamedBlockStatement {
        name,
        type_params: parsed_type_params.type_params,
        header_suffix: parsed_type_params.remainder.trim().to_owned(),
        bases: Vec::new(),
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_abstract_class: false,
        members: Vec::new(),
        line: line_number,
    }))
}

fn parse_overload(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    parse_function(
        path,
        line,
        rest,
        line_number,
        diagnostics,
        "overload declaration",
        SyntaxStatement::OverloadDef,
    )
}

fn parse_function(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
    label: &str,
    constructor: fn(FunctionStatement) -> SyntaxStatement,
) -> Option<SyntaxStatement> {
    let Some((signature, _suite)) = split_top_level_once(rest.trim_end(), ':') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must end with `:`"),
        ));
        return None;
    };

    let Some((name_part, _)) = split_top_level_once(signature.trim_end(), '(') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a parameter list"),
        ));
        return None;
    };
    let Some((name, suffix)) = extract_decl_head(name_part) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a function name"),
        ));
        return None;
    };
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, label)?;

    Some(constructor(FunctionStatement {
        name,
        type_params: parsed_type_params.type_params,
        params: Vec::new(),
        returns: None,
        is_async: false,
        is_override: false,
        is_deprecated: false,
        deprecation_message: None,
        line: line_number,
    }))
}

fn parse_unsafe(
    path: &Path,
    line: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    if line == "unsafe:" {
        return Some(SyntaxStatement::Unsafe(UnsafeStatement { line: line_number }));
    }

    if line.starts_with("unsafe:") {
        return Some(SyntaxStatement::Unsafe(UnsafeStatement { line: line_number }));
    }

    diagnostics.push(parse_error(
        path,
        line_number,
        line,
        "unsafe block must start with `unsafe:`",
    ));
    None
}

fn extract_decl_head(header: &str) -> Option<(String, &str)> {
    let header = header.trim();
    if header.is_empty() {
        return None;
    }

    let end = header
        .find(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        .unwrap_or(header.len());
    let candidate = &header[..end];
    is_valid_identifier(candidate).then(|| (candidate.to_owned(), &header[end..]))
}

fn parse_type_params<'source>(
    path: &Path,
    line_number: usize,
    line: &str,
    suffix: &'source str,
    diagnostics: &mut DiagnosticReport,
    label: &str,
) -> Option<ParsedTypeParams<'source>> {
    let suffix = suffix.trim_start();
    if suffix.is_empty() || !suffix.starts_with('[') {
        return Some(ParsedTypeParams { type_params: Vec::new(), remainder: suffix });
    }

    let Some((content, remainder)) = split_bracketed(suffix) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} has an unterminated type parameter list"),
        ));
        return None;
    };
    if remainder.trim_start().starts_with('[') {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must not contain multiple type parameter lists"),
        ));
        return None;
    }

    let mut type_params = Vec::new();
    for item in split_top_level(content, ',') {
        match parse_type_param(path, line_number, line, item, label) {
            Ok(type_param) => type_params.push(type_param),
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                return None;
            }
        }
    }

    let mut seen = std::collections::BTreeSet::new();
    for type_param in &type_params {
        if !seen.insert(type_param.name.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4004",
                    format!("{label} declares type parameter `{}` more than once", type_param.name),
                )
                .with_span(Span::new(
                    path.display().to_string(),
                    line_number,
                    1,
                    line_number,
                    line.chars().count().max(1),
                )),
            );
            return None;
        }
    }

    Some(ParsedTypeParams { type_params, remainder })
}

fn split_top_level_once(input: &str, separator: char) -> Option<(&str, &str)> {
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, character) in input.char_indices() {
        if character == separator && bracket_depth == 0 && paren_depth == 0 {
            let tail_start = index + character.len_utf8();
            return Some((&input[..index], &input[tail_start..]));
        }

        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }

    None
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut start = 0usize;

    for (index, character) in input.char_indices() {
        if character == ',' && bracket_depth == 0 && paren_depth == 0 {
            parts.push(input[start..index].trim());
            start = index + character.len_utf8();
            continue;
        }

        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }

    let tail = input[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

fn parse_type_param(
    path: &Path,
    line_number: usize,
    line: &str,
    item: &str,
    label: &str,
) -> Result<TypeParam, Box<Diagnostic>> {
    let item = item.trim();
    if item.is_empty() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} contains an empty type parameter entry"),
        )));
    }
    if item.contains('=') {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} type parameter defaults are deferred beyond v1"),
        )));
    }

    let (name_part, bound) = match item.split_once(':') {
        Some((name_part, bound)) => (name_part.trim(), Some(bound.trim())),
        None => (item, None),
    };
    if !is_valid_identifier(name_part) {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} contains an invalid type parameter name"),
        )));
    }

    let bound = match bound {
        Some("") => {
            return Err(Box::new(parse_error(
                path,
                line_number,
                line,
                format!("{label} type parameter bound must not be empty"),
            )));
        }
        Some(bound) if bound.starts_with('(') => {
            return Err(Box::new(parse_error(
                path,
                line_number,
                line,
                format!("{label} type parameter constraint lists are deferred beyond v1"),
            )));
        }
        Some(bound) => Some(bound.to_owned()),
        None => None,
    };

    Ok(TypeParam { name: name_part.to_owned(), bound })
}

fn split_bracketed(input: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;

    for (index, character) in input.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((&input[1..index], &input[index + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, character) in input.char_indices() {
        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ if character == separator && bracket_depth == 0 && paren_depth == 0 => {
                items.push(&input[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }

    items.push(&input[start..]);
    items
}

fn is_valid_identifier(candidate: &str) -> bool {
    let mut characters = candidate.chars();
    match characters.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn parse_error(
    path: &Path,
    line_number: usize,
    line: &str,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error("TPY2001", message.into()).with_span(Span::new(
        path.display().to_string(),
        line_number,
        1,
        line_number,
        line.chars().count().max(1),
    ))
}

fn parse_error_span(path: &Path, source: &str, start: usize, end: usize) -> Span {
    let start = start.min(source.len());
    let end = end.max(start).min(source.len());
    let (line, column) = offset_to_line_column(source, start);
    let (end_line, end_column) = offset_to_line_column(source, end);

    Span::new(path.display().to_string(), line, column, end_line, end_column)
}

fn offset_to_line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;

    for (index, character) in source.char_indices() {
        if index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}

#[cfg(test)]
mod tests {
    use super::{
        parse, parse_with_options, AssertStatement, CallStatement, ClassMember, ClassMemberKind,
        ComprehensionKind, DirectExprMetadata, ExceptionHandlerStatement, ForStatement,
        FunctionParam, FunctionStatement, GuardCondition, IfStatement, ImportBinding,
        ImportStatement, InvalidationStatement, LambdaMetadata, MatchCaseStatement, MatchPattern,
        MatchStatement, MemberAccessStatement, MethodCallStatement, MethodKind,
        NamedBlockStatement, ParseOptions, ReturnStatement, SourceFile, SourceKind,
        SyntaxStatement, TypeAliasStatement, TypeIgnoreDirective, TypeParam, UnsafeStatement,
        ValueStatement, WithStatement, YieldStatement,
    };
    use std::path::PathBuf;

    #[test]
    fn parse_recognizes_typepython_extension_headers() {
        let tree = parse(SourceFile {
            path: PathBuf::from("example.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair[T] = tuple[T, T]\n",
                "interface Service:\n",
                "    pass\n",
                "data class Box:\n",
                "    pass\n",
                "sealed class Result:\n",
                "    pass\n",
                "overload def parse(value):\n",
                "    ...\n",
                "unsafe:\n",
                "    pass\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("Pair"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    value: String::from("tuple[T, T]"),
                    line: 1,
                }),
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Service"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 4,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Result"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 6,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 8,
                }),
                SyntaxStatement::Unsafe(UnsafeStatement { line: 10 }),
            ]
        );
    }

    #[test]
    fn parse_captures_type_params_and_bounds() {
        let tree = parse(SourceFile {
            path: PathBuf::from("generic.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair[T: Hashable] = tuple[T, T]\n",
                "interface Box[T]:\n",
                "    pass\n",
                "data class Node[T: Sequence[str]]:\n",
                "    pass\n",
                "sealed class Result[T]:\n",
                "    pass\n",
                "overload def first[T: Sequence[str]](value):\n",
                "    ...\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("Pair"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("Hashable")),
                    }],
                    value: String::from("tuple[T, T]"),
                    line: 1,
                }),
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Node"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("Sequence[str]")),
                    }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 4,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Result"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 6,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("Sequence[str]")),
                    }],
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 8,
                }),
            ]
        );
    }

    #[test]
    fn parse_reports_malformed_extension_headers() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair tuple[int, int]\n",
                "interface:\n",
                "overload def parse\n",
                "unsafe\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.has_errors());
        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("typealias declaration must contain `=`"));
        assert!(rendered.contains("interface declaration must include a valid name"));
        assert!(rendered.contains("overload declaration must end with `:`"));
        assert!(rendered.contains("unsafe block must start with `unsafe:`"));
    }

    #[test]
    fn parse_reports_malformed_type_parameter_lists() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken-generics.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: concat!(
                "typealias Pair[T = int] = tuple[T, T]\n",
                "interface Box[T:] :\n",
                "overload def first[T: (A, B)](value):\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.has_errors());
        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("type parameter defaults are deferred beyond v1"));
        assert!(rendered.contains("type parameter bound must not be empty"));
        assert!(rendered.contains("type parameter constraint lists are deferred beyond v1"));
    }

    #[test]
    fn parse_reports_duplicate_type_parameter_names() {
        let tree = parse(SourceFile {
            path: PathBuf::from("duplicate-generics.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("class Box[T, T]:\n    pass\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("declares type parameter `T` more than once"));
    }

    #[test]
    fn parse_captures_interface_base_list_suffix() {
        let tree = parse(SourceFile {
            path: PathBuf::from("interface-bases.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("interface SupportsClose(Closable):\n    pass\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::from("(Closable)"),
                bases: vec![String::from("Closable")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_rejects_executable_interface_bodies() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-interface.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("interface SupportsClose:\n    value = 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("body must not contain executable statements"));
    }

    #[test]
    fn parse_accepts_overload_simple_suite_form() {
        let tree = parse(SourceFile {
            path: PathBuf::from("overload-simple-suite.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("overload def parse(x: str) -> int: ...\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::OverloadDef(FunctionStatement {
                name: String::from("parse"),
                type_params: Vec::new(),
                params: vec![FunctionParam {
                    name: String::from("x"),
                    annotation: Some(String::from("str")),
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false
                }],
                returns: Some(String::from("int")),
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_rejects_executable_overload_bodies() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-overload.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("overload def parse(x: str) -> int:\n    return 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("body must not contain executable statements"));
    }

    #[test]
    fn parse_leaves_python_files_without_extension_analysis() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def unsafe(value):\n    return value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("unsafe"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: None,
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("unsafe"),
                    owner_type_name: None,
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                })
            ]
        );
    }

    #[test]
    fn parse_reports_invalid_python_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def broken(:\n    return 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("Python syntax error"));
    }

    #[test]
    fn parse_accepts_valid_stub_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.pyi"),
            kind: SourceKind::Stub,
            logical_module: String::new(),
            text: String::from("def helper() -> int: ...\n"),
        });

        assert!(tree.diagnostics.is_empty());
    }

    #[test]
    fn parse_classifies_decorated_overloads_in_stub_sources() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.pyi"),
            kind: SourceKind::Stub,
            logical_module: String::new(),
            text: String::from(
                "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("overload"),
                        source_path: String::from("typing.overload"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("x"),
                        annotation: Some(String::from("str")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("int")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_reports_invalid_typepython_body_syntax_after_normalization() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("typealias UserId = int\ndef broken():\n    return )\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("TypePython syntax error"));
    }

    #[test]
    fn parse_reports_invalid_assignment_target_as_tpy4011() {
        let tree = parse(SourceFile {
            path: PathBuf::from("invalid_assign.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("def build() -> None:\n    (x + 1) = 2\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4011"));
        assert!(rendered.contains("Invalid assignment target"));
    }

    #[test]
    fn parse_reports_invalid_delete_target_as_tpy4011() {
        let tree = parse(SourceFile {
            path: PathBuf::from("invalid_del.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("def build() -> None:\n    del (x + 1)\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4011"));
        assert!(rendered.contains("Invalid delete target"));
    }

    #[test]
    fn parse_accepts_generic_python_headers_in_typepython_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("generic.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class Box[T]:\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam { name: String::from("T"), bound: None }],
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("T")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("T")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("first"),
                    owner_type_name: None,
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_imports_and_values_from_ast_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "from pkg import foo, bar as baz\nimport tools.helpers, more.tools as alias\nvalue: int = 1\na = b = 2\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![
                        ImportBinding {
                            local_name: String::from("foo"),
                            source_path: String::from("pkg.foo"),
                        },
                        ImportBinding {
                            local_name: String::from("baz"),
                            source_path: String::from("pkg.bar"),
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![
                        ImportBinding {
                            local_name: String::from("tools"),
                            source_path: String::from("tools.helpers"),
                        },
                        ImportBinding {
                            local_name: String::from("alias"),
                            source_path: String::from("more.tools"),
                        },
                    ],
                    line: 2,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("int")),
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
                    is_class_var: false,
                    line: 3,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("a"), String::from("b")],
                    destructuring_target_names: None,
                    annotation: None,
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
                    is_class_var: false,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_annotated_assignment_direct_rhs_forms() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "value: int = helper()\ncopy: str = source\nfield: str = box.value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
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
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("helper"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
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
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("field")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("value")),
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
                    line: 3,
                }),
                SyntaxStatement::MemberAccess(MemberAccessStatement {
                    owner_name: String::from("box"),
                    member: String::from("value"),
                    through_instance: false,
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_function_body_annotated_assignments() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(value: str) -> None:\n    result: int = value\n    item: str = helper()\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("str")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
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
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("helper"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 3,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("item")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("str")),
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
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_function_body_bare_assignments() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build() -> None:\n    value = helper()\n    field = box.item\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
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
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("helper"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value")],
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
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("field")],
                    destructuring_target_names: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("item")),
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
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_distinguishes_destructuring_from_chained_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("destructure.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("a = b = pair\nleft, right = pair\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Value(chain), SyntaxStatement::Value(destructure)] =
            tree.statements.as_slice()
        else {
            panic!("expected two value statements");
        };
        assert_eq!(chain.names, vec![String::from("a"), String::from("b")]);
        assert_eq!(chain.destructuring_target_names, None);
        assert_eq!(destructure.names, vec![String::from("left"), String::from("right")]);
        assert_eq!(
            destructure.destructuring_target_names,
            Some(vec![String::from("left"), String::from("right")])
        );
    }

    #[test]
    fn parse_extracts_function_body_namedexpr_assignments() {
        let tree = parse(SourceFile {
            path: PathBuf::from("namedexpr.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build() -> int:\n    if (tmp := 1):\n        return tmp\n    return 0\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        let walrus_assignment = tree.statements.iter().find_map(|statement| match statement {
            SyntaxStatement::Value(statement)
                if statement.names == vec![String::from("tmp")]
                    && statement.owner_name.as_deref() == Some("build") =>
            {
                Some(statement)
            }
            _ => None,
        });
        let walrus_assignment = walrus_assignment.expect("named expression assignment statement");
        assert_eq!(walrus_assignment.annotation, None);
        assert_eq!(walrus_assignment.value_type.as_deref(), Some("int"));
        assert_eq!(walrus_assignment.value_name, None);
        assert_eq!(walrus_assignment.line, 2);
    }

    #[test]
    fn parse_reports_namedexpr_non_name_target_as_invalid_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("namedexpr-invalid.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("value: int = (box.item := 1)\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("TPY4011"));
        assert!(rendered.contains("Assignment expression target must be an identifier"));
    }

    #[test]
    fn parse_normalizes_relative_import_provenance() {
        let tree = parse(SourceFile {
            path: PathBuf::from("src/app/child.py"),
            kind: SourceKind::Python,
            logical_module: String::from("app.child"),
            text: String::from("from .base import Base\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Import(ImportStatement {
                bindings: vec![ImportBinding {
                    local_name: String::from("Base"),
                    source_path: String::from("app.base.Base"),
                }],
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_extracts_top_level_direct_calls() {
        let tree = parse(SourceFile {
            path: PathBuf::from("calls.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("Builder()\nvalue = Factory()\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("Builder"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value")],
                    destructuring_target_names: None,
                    annotation: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: Some(String::from("Factory")),
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
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("Factory"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_call_keyword_names() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-keywords.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build(x=1, y=2)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Call(CallStatement {
                callee: String::from("build"),
                arg_count: 0,
                arg_types: Vec::new(),
                arg_values: Vec::new(),
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: vec![String::from("x"), String::from("y")],
                keyword_arg_types: vec![String::from("int"), String::from("int")],
                keyword_arg_values: vec![
                    DirectExprMetadata {
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
                    },
                    DirectExprMetadata {
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
                    },
                ],
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_collects_nested_calls_returns_and_assignments_in_control_flow_suites() {
        let tree = parse(SourceFile {
            path: PathBuf::from("nested-control-flow.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(flag, items, ctx):\n    while flag:\n        helper()\n        value = helper()\n    for item in items:\n        helper()\n    with ctx:\n        helper()\n    try:\n        helper()\n    except Exception:\n        helper()\n    finally:\n        return 1\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        let call_lines = tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Call(statement) if statement.callee == "helper" => {
                    Some(statement.line)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let value_lines = tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Value(statement)
                    if statement.names == vec![String::from("value")] =>
                {
                    Some(statement.line)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let return_lines = tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Return(statement) if statement.owner_name == "build" => {
                    Some(statement.line)
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(call_lines, vec![3, 4, 6, 8, 10, 12]);
        assert_eq!(value_lines, vec![4]);
        assert_eq!(return_lines, vec![14]);
    }

    #[test]
    fn parse_retains_direct_call_literal_arg_types() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-types.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build(1, \"x\")\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Call(CallStatement {
                callee: String::from("build"),
                arg_count: 2,
                arg_types: vec![String::from("int"), String::from("str")],
                arg_values: vec![
                    DirectExprMetadata {
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
                    },
                    DirectExprMetadata {
                        value_type: Some(String::from("str")),
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
                    },
                ],
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_retains_direct_call_container_literal_arg_types() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-container-types.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build([1, 2], (1, \"x\"), {\"x\": 1}, {1, 2})\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Call(statement)] = tree.statements.as_slice() else {
            panic!("expected direct call statement");
        };
        assert_eq!(statement.callee, "build");
        assert_eq!(
            statement.arg_types,
            vec![
                String::from("list[int]"),
                String::from("tuple[int, str]"),
                String::from("dict[str, int]"),
                String::from("set[int]"),
            ]
        );
        let list_elements =
            statement.arg_values[0].value_list_elements.as_ref().expect("list elements");
        assert_eq!(list_elements.len(), 2);
        assert_eq!(list_elements[0].value_type.as_deref(), Some("int"));
        let dict_entries =
            statement.arg_values[2].value_dict_entries.as_ref().expect("dict entries");
        assert_eq!(dict_entries.len(), 1);
        assert_eq!(dict_entries[0].key.as_deref(), Some("x"));
        assert!(!dict_entries[0].is_expansion);
        let set_elements =
            statement.arg_values[3].value_set_elements.as_ref().expect("set elements");
        assert_eq!(set_elements.len(), 2);
        assert_eq!(set_elements[0].value_type.as_deref(), Some("int"));
    }

    #[test]
    fn parse_retains_starred_call_expansion_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-starred.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build(*[1, 2], **{\"x\": 1})\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Call(statement)] = tree.statements.as_slice() else {
            panic!("expected direct call statement");
        };
        assert_eq!(statement.starred_arg_types, vec![String::from("list[int]")]);
        assert_eq!(statement.keyword_expansion_types, vec![String::from("dict[str, int]")]);
        let dict_entries = statement.keyword_expansion_values[0]
            .value_dict_entries
            .as_ref()
            .expect("keyword expansion dict entries");
        assert_eq!(dict_entries.len(), 1);
        assert_eq!(dict_entries[0].key.as_deref(), Some("x"));
        assert!(!dict_entries[0].is_expansion);
    }

    #[test]
    fn parse_retains_typed_dict_literal_entries_in_call_args() {
        let tree = parse(SourceFile {
            path: PathBuf::from("call-typeddict.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build({\"id\": 1}, user={\"name\": \"Ada\"})\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Call(statement)] = tree.statements.as_slice() else {
            panic!("expected direct call statement");
        };
        let positional_entries =
            statement.arg_values[0].value_dict_entries.as_ref().expect("positional dict entries");
        assert_eq!(positional_entries.len(), 1);
        assert_eq!(positional_entries[0].key.as_deref(), Some("id"));
        let keyword_entries = statement.keyword_arg_values[0]
            .value_dict_entries
            .as_ref()
            .expect("keyword dict entries");
        assert_eq!(keyword_entries.len(), 1);
        assert_eq!(keyword_entries[0].key.as_deref(), Some("name"));
    }

    #[test]
    fn parse_retains_lambda_metadata_in_call_args() {
        let tree = parse(SourceFile {
            path: PathBuf::from("lambda.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("build(lambda x: x)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Call(CallStatement {
                callee: String::from("build"),
                arg_count: 1,
                arg_types: vec![String::new()],
                arg_values: vec![DirectExprMetadata {
                    value_type: Some(String::new()),
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
                    value_lambda: Some(Box::new(LambdaMetadata {
                        param_names: vec![String::from("x")],
                        body: Box::new(DirectExprMetadata {
                            value_type: Some(String::new()),
                            is_awaited: false,
                            value_callee: None,
                            value_name: Some(String::from("x")),
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
                    })),
                    value_list_comprehension: None,
                    value_generator_comprehension: None,
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                }],
                starred_arg_types: Vec::new(),
                starred_arg_values: Vec::new(),
                keyword_names: Vec::new(),
                keyword_arg_types: Vec::new(),
                keyword_arg_values: Vec::new(),
                keyword_expansion_types: Vec::new(),
                keyword_expansion_values: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_retains_list_comprehension_metadata_in_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("listcomp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("values = [x + 1 for x in [1, 2] if x is not None]\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Value(statement)] = tree.statements.as_slice() else {
            panic!("expected value statement");
        };
        assert_eq!(statement.names, vec![String::from("values")]);
        let comprehension =
            statement.value_list_comprehension.as_deref().expect("list comprehension");
        assert_eq!(comprehension.kind, ComprehensionKind::List);
        assert_eq!(comprehension.clauses.len(), 1);
        assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
        let iter_elements =
            comprehension.clauses[0].iter.value_list_elements.as_ref().expect("iter list elements");
        assert_eq!(iter_elements.len(), 2);
        assert_eq!(iter_elements[0].value_type.as_deref(), Some("int"));
        assert_eq!(
            comprehension.clauses[0].filters,
            vec![GuardCondition::IsNone { name: String::from("x"), negated: true }]
        );
        assert_eq!(comprehension.element.value_binop_operator.as_deref(), Some("+"));
    }

    #[test]
    fn parse_retains_generator_comprehension_metadata_in_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("gencomp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("values = (x + 1 for x in [1, 2] if x is not None)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let [SyntaxStatement::Value(statement)] = tree.statements.as_slice() else {
            panic!("expected value statement");
        };
        assert_eq!(statement.names, vec![String::from("values")]);
        let comprehension =
            statement.value_generator_comprehension.as_deref().expect("generator comprehension");
        assert_eq!(comprehension.kind, ComprehensionKind::Generator);
        assert_eq!(comprehension.clauses.len(), 1);
        assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
        let iter_elements =
            comprehension.clauses[0].iter.value_list_elements.as_ref().expect("iter list elements");
        assert_eq!(iter_elements.len(), 2);
        assert_eq!(iter_elements[0].value_type.as_deref(), Some("int"));
        assert_eq!(
            comprehension.clauses[0].filters,
            vec![GuardCondition::IsNone { name: String::from("x"), negated: true }]
        );
        assert_eq!(comprehension.element.value_binop_operator.as_deref(), Some("+"));
    }

    #[test]
    fn parse_retains_set_comprehension_metadata_in_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("setcomp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("values = {x + 1 for x in [1, 2]}\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let SyntaxStatement::Value(statement) = &tree.statements[0] else {
            panic!("expected value statement");
        };
        let comprehension =
            statement.value_list_comprehension.as_deref().expect("set comprehension");
        assert_eq!(comprehension.kind, ComprehensionKind::Set);
        assert!(comprehension.key.is_none());
        assert_eq!(comprehension.clauses.len(), 1);
        assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
    }

    #[test]
    fn parse_retains_dict_comprehension_metadata_in_assignment() {
        let tree = parse(SourceFile {
            path: PathBuf::from("dictcomp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("values = {x: x + 1 for x in [1, 2]}\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let SyntaxStatement::Value(statement) = &tree.statements[0] else {
            panic!("expected value statement");
        };
        let comprehension =
            statement.value_list_comprehension.as_deref().expect("dict comprehension");
        assert_eq!(comprehension.kind, ComprehensionKind::Dict);
        assert!(comprehension.key.is_some());
        assert_eq!(comprehension.clauses.len(), 1);
        assert_eq!(comprehension.clauses[0].target_names, vec![String::from("x")]);
    }

    #[test]
    fn parse_retains_ifexp_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("ifexp.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("value: int = 1 if True else 2\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Value(ValueStatement {
                names: vec![String::from("value")],
                destructuring_target_names: None,
                annotation: Some(String::from("int")),
                value_type: Some(String::new()),
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
                value_if_true: Some(Box::new(DirectExprMetadata {
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
                })),
                value_if_false: Some(Box::new(DirectExprMetadata {
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
                })),
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
            })]
        );
    }

    #[test]
    fn parse_retains_ifexp_guard_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("ifexp-guard.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("value: str = data if data is not None else \"\"\n"),
        });

        assert!(tree.diagnostics.is_empty());
        let SyntaxStatement::Value(statement) = &tree.statements[0] else {
            panic!("expected value statement");
        };
        assert_eq!(
            statement.value_if_guard,
            Some(GuardCondition::IsNone { name: String::from("data"), negated: true })
        );
    }

    #[test]
    fn parse_extracts_nested_direct_calls() {
        let tree = parse(SourceFile {
            path: PathBuf::from("nested-calls.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build() -> None:\n    Factory()\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
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
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("Factory"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_return_literals() {
        let tree = parse(SourceFile {
            path: PathBuf::from("returns.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build() -> int:\n    return \"x\"\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::from("str")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_bool_and_none_return_literals() {
        let tree = parse(SourceFile {
            path: PathBuf::from("returns.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def truthy() -> bool:\n    return True\n\ndef missing() -> None:\n    return None\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("truthy"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("bool")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("truthy"),
                    owner_type_name: None,
                    value_type: Some(String::from("bool")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("missing"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("None")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("missing"),
                    owner_type_name: None,
                    value_type: Some(String::from("None")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_return_call_callee() {
        let tree = parse(SourceFile {
            path: PathBuf::from("returns.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build() -> int:\n    return helper()\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_return_member_access() {
        let tree = parse(SourceFile {
            path: PathBuf::from("returns.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build(box: Box) -> str:\n    return box.value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("box"),
                        annotation: Some(String::from("Box")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: Some(String::from("box")),
                    value_member_name: Some(String::from("value")),
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
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_member_accesses() {
        let tree = parse(SourceFile {
            path: PathBuf::from("member-access.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("Box.missing\nBox().value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::MemberAccess(MemberAccessStatement {
                    owner_name: String::from("Box"),
                    member: String::from("missing"),
                    through_instance: false,
                    line: 1,
                }),
                SyntaxStatement::MemberAccess(MemberAccessStatement {
                    owner_name: String::from("Box"),
                    member: String::from("value"),
                    through_instance: true,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_direct_method_calls() {
        let tree = parse(SourceFile {
            path: PathBuf::from("method-call.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("Box.run(1)\nBox().build(x=1)\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::MethodCall(MethodCallStatement {
                    owner_name: String::from("Box"),
                    method: String::from("run"),
                    through_instance: false,
                    arg_count: 1,
                    arg_types: vec![String::from("int")],
                    arg_values: vec![DirectExprMetadata {
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
                    }],
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::MethodCall(MethodCallStatement {
                    owner_name: String::from("Box"),
                    method: String::from("build"),
                    through_instance: true,
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: vec![String::from("x")],
                    keyword_arg_types: vec![String::from("int")],
                    keyword_arg_values: vec![DirectExprMetadata {
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
                    }],
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_class_like_members_from_ast_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("members.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class Box:\n    value: int\n    total = 1\n    def get(self) -> int: ...\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Box"),
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
                        name: String::from("total"),
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
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("get"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        value_type: None,
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        }],
                        returns: Some(String::from("int")),
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
            })]
        );
    }

    #[test]
    fn parse_marks_decorated_class_methods_as_overload_members() {
        let tree = parse(SourceFile {
            path: PathBuf::from("class-overloads.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "from typing import overload\n\nclass Parser:\n    @overload\n    def parse(self, x: str) -> int: ...\n\n    def parse(self, x):\n        return 0\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("overload"),
                        source_path: String::from("typing.overload"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Parser"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("parse"),
                            kind: ClassMemberKind::Overload,
                            method_kind: Some(MethodKind::Instance),
                            annotation: None,
                            value_type: None,
                            params: vec![
                                FunctionParam {
                                    name: String::from("self"),
                                    annotation: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                                FunctionParam {
                                    name: String::from("x"),
                                    annotation: Some(String::from("str")),
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                            ],
                            returns: Some(String::from("int")),
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
                        ClassMember {
                            name: String::from("parse"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::Instance),
                            annotation: None,
                            value_type: None,
                            params: vec![
                                FunctionParam {
                                    name: String::from("self"),
                                    annotation: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                                FunctionParam {
                                    name: String::from("x"),
                                    annotation: None,
                                    has_default: false,
                                    positional_only: false,
                                    keyword_only: false,
                                    variadic: false,
                                    keyword_variadic: false
                                },
                            ],
                            returns: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 7,
                        },
                    ],
                    line: 3,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("parse"),
                    owner_type_name: Some(String::from("Parser")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 8,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_final_value_declarations() {
        let tree = parse(SourceFile {
            path: PathBuf::from("finals.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import Final\nMAX_SIZE: Final = 100\nclass Box:\n    limit: Final[int] = 1\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("Final"),
                        source_path: String::from("typing.Final"),
                    }],
                    line: 1,
                }),
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
                    line: 2,
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
                        annotation: Some(String::from("Final[int]")),
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
                        line: 4,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_final_decorated_classes_and_methods() {
        let tree = parse(SourceFile {
            path: PathBuf::from("final-decorators.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import final\n\n@final\nclass Base:\n    @final\n    def run(self) -> None:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("final"),
                        source_path: String::from("typing.final"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Base"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: true,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("run"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        value_type: None,
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        }],
                        returns: Some(String::from("None")),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: true,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 5,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_classvar_value_declarations() {
        let tree = parse(SourceFile {
            path: PathBuf::from("classvars.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import ClassVar\nVALUE: ClassVar[int] = 1\nclass Box:\n    cache: ClassVar[int] = 2\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("ClassVar"),
                        source_path: String::from("typing.ClassVar"),
                    }],
                    line: 1,
                }),
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
                    line: 2,
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
                        annotation: Some(String::from("ClassVar[int]")),
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
                        line: 4,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_rejects_classvar_inside_function_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-classvar.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import ClassVar\n\ndef build() -> None:\n    value: ClassVar[int] = 1\n",
            ),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("ClassVar[...] is not allowed inside function or method bodies"));
    }

    #[test]
    fn parse_rejects_classvar_in_parameter_position() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-classvar-param.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import ClassVar\n\ndef build(value: ClassVar[int]) -> None:\n    pass\n",
            ),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("parameter annotations"));
    }

    #[test]
    fn parse_rejects_final_in_parameter_position() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-final-param.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import Final\n\ndef build(value: Final[int]) -> None:\n    pass\n",
            ),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4010"));
        assert!(rendered.contains("deferred beyond v1"));
    }

    #[test]
    fn parse_rejects_deferred_async_constructs_in_typepython_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("async-deferred.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "async def fetch() -> int:\n    await work()\n    async for item in stream:\n        pass\n    async with manager:\n        pass\n\ndef produce():\n    yield 1\n\ndef relay():\n    yield from produce()\n",
            ),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4010"));
        assert!(rendered.contains("`async def` in .tpy source is deferred beyond v1"));
        assert!(rendered.contains("`await` in .tpy source is deferred beyond v1"));
        assert!(rendered.contains("`async for` in .tpy source is deferred beyond v1"));
        assert!(rendered.contains("`async with` in .tpy source is deferred beyond v1"));
        assert!(rendered.contains("`yield` in .tpy source is deferred beyond v1"));
        assert!(rendered.contains("`yield from` in .tpy source is deferred beyond v1"));
    }

    #[test]
    fn parse_allows_async_constructs_in_python_passthrough_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("async.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("async def fetch() -> int:\n    return 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(!rendered.contains("TPY4010"));
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("fetch"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    is_async: true,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("fetch"),
                    owner_type_name: None,
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_await_in_python_passthrough_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("await.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "async def fetch() -> int:\n    return 1\n\nasync def build() -> int:\n    return await fetch()\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("fetch"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    is_async: true,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("fetch"),
                    owner_type_name: None,
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 2,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("int")),
                    is_async: true,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: true,
                    value_callee: Some(String::from("fetch")),
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
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_yield_in_python_passthrough_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("yield.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def produce() -> Generator[int, None, None]:\n    yield 1\n\ndef relay() -> Generator[int, None, None]:\n    yield from values\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("produce"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("Generator[int, None, None]")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Yield(YieldStatement {
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
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("relay"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("Generator[int, None, None]")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 4,
                }),
                SyntaxStatement::Yield(YieldStatement {
                    owner_name: String::from("relay"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    is_yield_from: true,
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_method_call_result_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("methods.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(box: Box) -> str:\n    result: str = box.get()\n    return box.get()\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("box"),
                        annotation: Some(String::from("Box")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("result")],
                    destructuring_target_names: None,
                    annotation: Some(String::from("str")),
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("box")),
                    value_method_name: Some(String::from("get")),
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
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("box")),
                    value_method_name: Some(String::from("get")),
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
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_direct_method_call_result_metadata_through_instance() {
        let tree = parse(SourceFile {
            path: PathBuf::from("methods.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("def build() -> str:\n    return make_box().get()\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("str")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: None,
                    value_member_owner_name: None,
                    value_member_name: None,
                    value_member_through_instance: false,
                    value_method_owner_name: Some(String::from("make_box")),
                    value_method_name: Some(String::from("get")),
                    value_method_through_instance: true,
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
                    line: 2,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_simple_for_loop_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("for_loop.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(values: list[int]) -> int:\n    for item in values:\n        pass\n    return item\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("values"),
                        annotation: Some(String::from("list[int]")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("int")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::For(ForStatement {
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
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("item")),
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
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_tuple_for_loop_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("for_loop.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(pairs: tuple[tuple[int, str]]) -> str:\n    for a, b in pairs:\n        pass\n    return b\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("pairs"),
                        annotation: Some(String::from("tuple[tuple[int, str]]")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::For(ForStatement {
                    target_name: String::new(),
                    target_names: vec![String::from("a"), String::from("b")],
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    iter_type: Some(String::new()),
                    iter_is_awaited: false,
                    iter_callee: None,
                    iter_name: Some(String::from("pairs")),
                    iter_member_owner_name: None,
                    iter_member_name: None,
                    iter_member_through_instance: false,
                    iter_method_owner_name: None,
                    iter_method_name: None,
                    iter_method_through_instance: false,
                    line: 2,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("b")),
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
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_simple_with_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("with_stmt.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(manager: Manager) -> str:\n    with manager as value:\n        pass\n    return value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("manager"),
                        annotation: Some(String::from("Manager")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::With(WithStatement {
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
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_with_item_without_target() {
        let tree = parse(SourceFile {
            path: PathBuf::from("with_stmt.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(manager: Manager) -> None:\n    with manager:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("manager"),
                        annotation: Some(String::from("Manager")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("None")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::With(WithStatement {
                    target_name: None,
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
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_multiple_with_items() {
        let tree = parse(SourceFile {
            path: PathBuf::from("with_stmt.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(a: A, b: B) -> str:\n    with a as x, b as y:\n        pass\n    return y\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![
                        FunctionParam {
                            name: String::from("a"),
                            annotation: Some(String::from("A")),
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        },
                        FunctionParam {
                            name: String::from("b"),
                            annotation: Some(String::from("B")),
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        },
                    ],
                    returns: Some(String::from("str")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::With(WithStatement {
                    target_name: Some(String::from("x")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("a")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }),
                SyntaxStatement::With(WithStatement {
                    target_name: Some(String::from("y")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    context_type: Some(String::new()),
                    context_is_awaited: false,
                    context_callee: None,
                    context_name: Some(String::from("b")),
                    context_member_owner_name: None,
                    context_member_name: None,
                    context_member_through_instance: false,
                    context_method_owner_name: None,
                    context_method_name: None,
                    context_method_through_instance: false,
                    line: 2,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("y")),
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
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_except_handler_binding() {
        let tree = parse(SourceFile {
            path: PathBuf::from("try_stmt.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build() -> ValueError:\n    try:\n        risky()\n    except ValueError as e:\n        return e\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("ValueError")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 1,
                }),
                SyntaxStatement::Call(CallStatement {
                    callee: String::from("risky"),
                    arg_count: 0,
                    arg_types: Vec::new(),
                    arg_values: Vec::new(),
                    starred_arg_types: Vec::new(),
                    starred_arg_values: Vec::new(),
                    keyword_names: Vec::new(),
                    keyword_arg_types: Vec::new(),
                    keyword_arg_values: Vec::new(),
                    keyword_expansion_types: Vec::new(),
                    keyword_expansion_values: Vec::new(),
                    line: 3,
                }),
                SyntaxStatement::ExceptHandler(ExceptionHandlerStatement {
                    exception_type: String::from("ValueError"),
                    binding_name: Some(String::from("e")),
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    line: 4,
                    end_line: 5,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::new()),
                    is_awaited: false,
                    value_callee: None,
                    value_name: Some(String::from("e")),
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
                    line: 5,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_function_signature_shapes() {
        let tree = parse(SourceFile {
            path: PathBuf::from("signatures.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import overload\n\n@overload\ndef parse(value: str) -> int: ...\n\ndef build(value: int) -> str:\n    return \"x\"\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("overload"),
                        source_path: String::from("typing.overload"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("str")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("int")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 3,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("int")),
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false
                    }],
                    returns: Some(String::from("str")),
                    is_async: false,
                    is_override: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 6,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("build"),
                    owner_type_name: None,
                    value_type: Some(String::from("str")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 7,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_override_decorated_functions_and_members() {
        let tree = parse(SourceFile {
            path: PathBuf::from("override.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from typing import override\n\n@override\ndef top_level() -> None:\n    pass\n\nclass Child(Base):\n    @override\n    def run(self) -> None:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("override"),
                        source_path: String::from("typing.override"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("top_level"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: Some(String::from("None")),
                    is_async: false,
                    is_override: true,
                    is_deprecated: false,
                    deprecation_message: None,
                    line: 3,
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
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        }],
                        returns: Some(String::from("None")),
                        is_async: false,
                        is_override: true,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 8,
                    }],
                    line: 7,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_abstract_class_methods() {
        let tree = parse(SourceFile {
            path: PathBuf::from("abstracts.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "from abc import abstractmethod\n\nclass Base:\n    @abstractmethod\n    def run(self) -> None:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![ImportBinding {
                        local_name: String::from("abstractmethod"),
                        source_path: String::from("abc.abstractmethod"),
                    }],
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Base"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: true,
                    members: vec![ClassMember {
                        name: String::from("run"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        value_type: None,
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false
                        }],
                        returns: Some(String::from("None")),
                        is_async: false,
                        is_override: false,
                        is_abstract_method: true,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_method_kinds_from_decorators() {
        let tree = parse(SourceFile {
            path: PathBuf::from("member-kinds.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "class Box:\n    @classmethod\n    def make(cls) -> None:\n        pass\n\n    @staticmethod\n    def build() -> None:\n        pass\n\n    @property\n    def name(self) -> str:\n        return \"x\"\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("make"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::Class),
                            annotation: None,
                            value_type: None,
                            params: vec![FunctionParam {
                                name: String::from("cls"),
                                annotation: None,
                                has_default: false,
                                positional_only: false,
                                keyword_only: false,
                                variadic: false,
                                keyword_variadic: false
                            }],
                            returns: Some(String::from("None")),
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
                            name: String::from("build"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::Static),
                            annotation: None,
                            value_type: None,
                            params: Vec::new(),
                            returns: Some(String::from("None")),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 6,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Method,
                            method_kind: Some(MethodKind::Property),
                            annotation: None,
                            value_type: None,
                            params: vec![FunctionParam {
                                name: String::from("self"),
                                annotation: None,
                                has_default: false,
                                positional_only: false,
                                keyword_only: false,
                                variadic: false,
                                keyword_variadic: false
                            }],
                            returns: Some(String::from("str")),
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 10,
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::Return(ReturnStatement {
                    owner_name: String::from("name"),
                    owner_type_name: Some(String::from("Box")),
                    value_type: Some(String::from("str")),
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
                    value_list_elements: None,
                    value_set_elements: None,
                    value_dict_entries: None,
                    line: 12,
                })
            ]
        );
    }

    #[test]
    fn parse_retains_match_statement_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("match_case.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "match value:\n    case Add():\n        pass\n    case Mul() | Div():\n        pass\n    case _:\n        pass\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Match(MatchStatement {
                owner_name: None,
                owner_type_name: None,
                subject_type: Some(String::new()),
                subject_is_awaited: false,
                subject_callee: None,
                subject_name: Some(String::from("value")),
                subject_member_owner_name: None,
                subject_member_name: None,
                subject_member_through_instance: false,
                subject_method_owner_name: None,
                subject_method_name: None,
                subject_method_through_instance: false,
                cases: vec![
                    MatchCaseStatement {
                        patterns: vec![MatchPattern::Class(String::from("Add"))],
                        has_guard: false,
                        line: 2,
                    },
                    MatchCaseStatement {
                        patterns: vec![
                            MatchPattern::Class(String::from("Mul")),
                            MatchPattern::Class(String::from("Div")),
                        ],
                        has_guard: false,
                        line: 4,
                    },
                    MatchCaseStatement {
                        patterns: vec![MatchPattern::Wildcard],
                        has_guard: false,
                        line: 6,
                    },
                ],
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_retains_if_and_assert_guard_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("guards.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(value: str | None) -> str:\n    if value is not None:\n        return value\n    assert value is None\n    return \"fallback\"\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements[1],
            SyntaxStatement::If(IfStatement {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(GuardCondition::IsNone { name: String::from("value"), negated: true }),
                line: 2,
                true_start_line: 3,
                true_end_line: 3,
                false_start_line: None,
                false_end_line: None,
            })
        );
        assert_eq!(
            tree.statements[3],
            SyntaxStatement::Assert(AssertStatement {
                owner_name: Some(String::from("build")),
                owner_type_name: None,
                guard: Some(GuardCondition::IsNone { name: String::from("value"), negated: false }),
                line: 4,
            })
        );
    }

    #[test]
    fn parse_retains_invalidation_statement_metadata() {
        let tree = parse(SourceFile {
            path: PathBuf::from("invalidate.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from(
                "def build(value: int | None) -> int:\n    if value is not None:\n        value += 1\n        del value\n        global value\n        nonlocal value\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements
                .iter()
                .filter(|statement| matches!(statement, SyntaxStatement::Invalidate(_)))
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                SyntaxStatement::Invalidate(InvalidationStatement {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 3,
                }),
                SyntaxStatement::Invalidate(InvalidationStatement {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 4,
                }),
                SyntaxStatement::Invalidate(InvalidationStatement {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 5,
                }),
                SyntaxStatement::Invalidate(InvalidationStatement {
                    owner_name: Some(String::from("build")),
                    owner_type_name: None,
                    names: vec![String::from("value")],
                    line: 6,
                }),
            ]
        );
    }

    #[test]
    fn parse_retains_type_ignore_directives() {
        let tree = parse(SourceFile {
            path: PathBuf::from("ignore.py"),
            kind: SourceKind::Python,
            logical_module: String::new(),
            text: String::from("x = 1  # type: ignore[TPY4001]\ny = 2  # type: ignore\n"),
        });

        assert_eq!(
            tree.type_ignore_directives,
            vec![
                TypeIgnoreDirective { line: 1, codes: Some(vec![String::from("TPY4001")]) },
                TypeIgnoreDirective { line: 2, codes: None },
            ]
        );
    }

    #[test]
    fn parse_rejects_conditional_return_syntax_by_default() {
        let tree = parse(SourceFile {
            path: PathBuf::from("conditional-return.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
            ),
        });

        assert!(tree.diagnostics.has_errors());
    }

    #[test]
    fn parse_accepts_conditional_return_syntax_when_enabled() {
        let tree = parse_with_options(
            SourceFile {
                path: PathBuf::from("conditional-return.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
                ),
            },
            ParseOptions { enable_conditional_returns: true },
        );

        assert!(tree.diagnostics.is_empty());
        let sites = crate::collect_conditional_return_sites(&tree.source.text);
        assert_eq!(
            sites,
            vec![crate::ConditionalReturnSite {
                function_name: String::from("decode"),
                target_name: String::from("x"),
                target_type: String::from("str | bytes | None"),
                case_input_types: vec![
                    String::from("str"),
                    String::from("bytes"),
                    String::from("None"),
                ],
                line: 1,
            }]
        );
    }

    #[test]
    fn parse_accepts_multiline_conditional_return_syntax_when_enabled() {
        let tree = parse_with_options(
            SourceFile {
                path: PathBuf::from("conditional-return-multiline.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "def decode(\n    x: str | bytes | None,\n) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n\nvalue: int = 1\n",
                ),
            },
            ParseOptions { enable_conditional_returns: true },
        );

        assert!(tree.diagnostics.is_empty(), "{}", tree.diagnostics.as_text());
        let sites = crate::collect_conditional_return_sites(&tree.source.text);
        assert_eq!(
            sites,
            vec![crate::ConditionalReturnSite {
                function_name: String::from("decode"),
                target_name: String::from("x"),
                target_type: String::from("str | bytes | None"),
                case_input_types: vec![
                    String::from("str"),
                    String::from("bytes"),
                    String::from("None"),
                ],
                line: 1,
            }]
        );
    }
}
