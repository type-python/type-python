use super::*;

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
    /// Logical module name used by later binding/graph phases.
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
    /// Parsed top-level statements in source order.
    pub statements: Vec<SyntaxStatement>,
    /// `# type: ignore[...]` directives collected from the source.
    pub type_ignore_directives: Vec<TypeIgnoreDirective>,
    /// Parse diagnostics produced while classifying the source.
    pub diagnostics: DiagnosticReport,
}

/// One parsed `type: ignore` directive with an optional code filter list.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeIgnoreDirective {
    pub line: usize,
    pub codes: Option<Vec<String>>,
}

/// Top-level statements recognized by the TypePython parser.
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

/// Source-authored `typealias` declaration.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeAliasStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub value: String,
    pub value_expr: Option<TypeExpr>,
    pub line: usize,
}

/// Named type block such as `class`, `interface`, `data class`, or `sealed class`.
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

/// Function or overload definition with parsed signature metadata.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FunctionStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
    pub returns_expr: Option<TypeExpr>,
    pub is_async: bool,
    pub is_override: bool,
    pub is_deprecated: bool,
    pub deprecation_message: Option<String>,
    pub line: usize,
}

/// One function parameter in parsed source order.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct FunctionParam {
    pub name: String,
    pub annotation: Option<String>,
    pub annotation_expr: Option<TypeExpr>,
    pub has_default: bool,
    pub positional_only: bool,
    pub keyword_only: bool,
    pub variadic: bool,
    pub keyword_variadic: bool,
}

impl FunctionParam {
    #[must_use]
    pub fn rendered_annotation(&self) -> Option<String> {
        self.annotation_expr.as_ref().map(TypeExpr::render).or_else(|| self.annotation.clone())
    }
}

/// Parsed import statement.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ImportStatement {
    pub bindings: Vec<ImportBinding>,
    pub line: usize,
}

/// One local binding produced by an import statement.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ImportBinding {
    pub local_name: String,
    pub source_path: String,
}

/// Parsed value-producing statement, including assignment metadata.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValueStatement {
    pub names: Vec<String>,
    pub destructuring_target_names: Option<Vec<String>>,
    pub annotation: Option<String>,
    pub annotation_expr: Option<TypeExpr>,
    pub value_type: Option<String>,
    pub value_type_expr: Option<TypeExpr>,
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

/// Direct function call that appears as a top-level parser statement.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CallStatement {
    pub callee: String,
    pub arg_count: usize,
    pub arg_values: Vec<DirectExprMetadata>,
    pub starred_arg_values: Vec<DirectExprMetadata>,
    pub keyword_names: Vec<String>,
    pub keyword_arg_values: Vec<DirectExprMetadata>,
    pub keyword_expansion_values: Vec<DirectExprMetadata>,
    pub line: usize,
}

/// Direct attribute access statement.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MemberAccessStatement {
    pub current_owner_name: Option<String>,
    pub current_owner_type_name: Option<String>,
    pub owner_name: String,
    pub member: String,
    pub through_instance: bool,
    pub line: usize,
}

/// Method call statement with receiver metadata.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MethodCallStatement {
    pub current_owner_name: Option<String>,
    pub current_owner_type_name: Option<String>,
    pub owner_name: String,
    pub method: String,
    pub through_instance: bool,
    pub arg_count: usize,
    pub arg_values: Vec<DirectExprMetadata>,
    pub starred_arg_values: Vec<DirectExprMetadata>,
    pub keyword_names: Vec<String>,
    pub keyword_arg_values: Vec<DirectExprMetadata>,
    pub keyword_expansion_values: Vec<DirectExprMetadata>,
    pub line: usize,
}

/// Return statement annotated with parsed expression metadata.
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

/// Yield statement annotated with parsed expression metadata.
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

/// Parsed `if` statement guard range.
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

/// Parsed `assert` guard.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AssertStatement {
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub guard: Option<GuardCondition>,
    pub line: usize,
}

/// Why a previously known name shape becomes invalid.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum InvalidationKind {
    RebindLike,
    Delete,
    ScopeChange,
}

/// Parsed invalidation statement used by later flow-sensitive passes.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InvalidationStatement {
    pub kind: InvalidationKind,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub names: Vec<String>,
    pub line: usize,
}

/// Guard condition recognized directly from source.
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

/// Parsed match statement and subject metadata.
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

/// One `case` arm inside a parsed match statement.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MatchCaseStatement {
    pub patterns: Vec<MatchPattern>,
    pub has_guard: bool,
    pub line: usize,
}

/// Pattern category captured from a parsed match arm.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MatchPattern {
    Wildcard,
    Literal(String),
    Class(String),
    Unsupported,
}

/// Parsed `for` loop site and iterable metadata.
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

/// Parsed `with` statement and context-manager metadata.
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

/// Parsed exception handler block.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExceptionHandlerStatement {
    pub exception_type: String,
    pub binding_name: Option<String>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
    pub end_line: usize,
}

/// Member declared inside a parsed type block.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ClassMember {
    pub name: String,
    pub kind: ClassMemberKind,
    pub method_kind: Option<MethodKind>,
    pub annotation: Option<String>,
    pub annotation_expr: Option<TypeExpr>,
    pub value_type: Option<String>,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
    pub returns_expr: Option<TypeExpr>,
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

impl ClassMember {
    #[must_use]
    pub fn rendered_annotation(&self) -> Option<String> {
        self.annotation_expr.as_ref().map(TypeExpr::render).or_else(|| self.annotation.clone())
    }

    #[must_use]
    pub fn rendered_returns(&self) -> Option<String> {
        self.returns_expr.as_ref().map(TypeExpr::render).or_else(|| self.returns.clone())
    }
}

/// High-level class member categories.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClassMemberKind {
    Field,
    Method,
    Overload,
}

/// Method dispatch role extracted from decorators and syntax position.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum MethodKind {
    Instance,
    Class,
    Static,
    Property,
    PropertySetter,
}

/// Marker statement for an `unsafe:` block.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UnsafeStatement {
    pub line: usize,
}

/// Generic parameter forms supported by TypePython source parsing.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TypeParamKind {
    TypeVar,
    ParamSpec,
    TypeVarTuple,
}

/// Parsed generic parameter declaration.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeParam {
    pub kind: TypeParamKind,
    pub name: String,
    pub bound: Option<String>,
    pub bound_expr: Option<TypeExpr>,
    pub constraints: Vec<String>,
    pub constraint_exprs: Vec<TypeExpr>,
    pub default: Option<String>,
    pub default_expr: Option<TypeExpr>,
}

impl TypeParam {
    #[must_use]
    pub fn rendered_bound(&self) -> Option<String> {
        self.bound_expr.as_ref().map(TypeExpr::render).or_else(|| self.bound.clone())
    }

    #[must_use]
    pub fn rendered_constraints(&self) -> Vec<String> {
        if !self.constraint_exprs.is_empty() {
            self.constraint_exprs.iter().map(TypeExpr::render).collect()
        } else {
            self.constraints.clone()
        }
    }

    #[must_use]
    pub fn rendered_default(&self) -> Option<String> {
        self.default_expr.as_ref().map(TypeExpr::render).or_else(|| self.default.clone())
    }
}

pub(super) struct ParsedTypeParams<'source> {
    pub(super) type_params: Vec<TypeParam>,
    pub(super) remainder: &'source str,
}

/// Parsed lambda signature and body metadata.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct LambdaMetadata {
    pub params: Vec<FunctionParam>,
    pub body: Box<DirectExprMetadata>,
}

#[derive(Debug, Clone)]
pub(super) struct AnnotatedLambdaSite {
    pub line: usize,
    pub column: usize,
    pub param_names: Vec<String>,
    pub annotations: Vec<Option<String>>,
}

thread_local! {
    pub(super) static ACTIVE_ANNOTATED_LAMBDA_SITES: RefCell<Vec<AnnotatedLambdaSite>> =
        const { RefCell::new(Vec::new()) };
}

/// One comprehension clause with target, iterable, and filter information.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ComprehensionClauseMetadata {
    pub target_name: String,
    pub target_names: Vec<String>,
    pub iter: Box<DirectExprMetadata>,
    pub filters: Vec<GuardCondition>,
}

/// Parsed comprehension body shared by list/set/dict/generator forms.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ComprehensionMetadata {
    pub kind: ComprehensionKind,
    pub clauses: Vec<ComprehensionClauseMetadata>,
    pub key: Option<Box<DirectExprMetadata>>,
    pub element: Box<DirectExprMetadata>,
}

/// Comprehension output category.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ComprehensionKind {
    List,
    Set,
    Dict,
    Generator,
}

/// Expression metadata retained for direct checks and contextual re-typing.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DirectExprMetadata {
    pub value_type_expr: Option<TypeExpr>,
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

impl DirectExprMetadata {
    #[must_use]
    pub fn rendered_value_type(&self) -> Option<String> {
        self.value_type_expr.as_ref().map(TypeExpr::render)
    }

    #[must_use]
    pub fn from_type_text(text: impl Into<String>) -> Self {
        let value_type = text.into();
        Self {
            value_type_expr: TypeExpr::parse(&value_type),
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
        }
    }
}

#[must_use]
pub fn direct_expr_metadata_vec_from_type_texts(
    value_types: impl IntoIterator<Item = String>,
) -> Vec<DirectExprMetadata> {
    value_types.into_iter().map(DirectExprMetadata::from_type_text).collect()
}

/// One key/value entry inside a parsed dict or TypedDict literal.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TypedDictLiteralEntry {
    pub key: Option<String>,
    pub key_value: Option<Box<DirectExprMetadata>>,
    pub is_expansion: bool,
    pub value: DirectExprMetadata,
}

pub(super) fn direct_operator_text(operator: ruff_python_ast::Operator) -> String {
    match operator {
        ruff_python_ast::Operator::Add => String::from("+"),
        ruff_python_ast::Operator::Sub => String::from("-"),
        ruff_python_ast::Operator::Mult => String::from("*"),
        ruff_python_ast::Operator::Div => String::from("/"),
        ruff_python_ast::Operator::FloorDiv => String::from("//"),
        ruff_python_ast::Operator::Mod => String::from("%"),
        _ => String::new(),
    }
}

/// TypedDict literal occurrence annotated with its target type.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypedDictLiteralSite {
    pub annotation: String,
    pub entries: Vec<TypedDictLiteralEntry>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
}

/// Call site where a TypedDict-shaped argument may need contextual validation.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DirectCallContextSite {
    pub callee: String,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub positional_arg_count: usize,
    pub keyword_arg_count: usize,
    pub has_starred_args: bool,
    pub has_unpacked_kwargs: bool,
    pub line: usize,
}

/// Mutation forms tracked for TypedDict keys.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TypedDictMutationKind {
    Assignment,
    AugmentedAssignment,
    Delete,
}

/// Parsed TypedDict mutation site.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypedDictMutationSite {
    pub kind: TypedDictMutationKind,
    pub key: Option<String>,
    pub operator: Option<String>,
    pub key_value: DirectExprMetadata,
    pub target: DirectExprMetadata,
    pub value: Option<DirectExprMetadata>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
}

/// Parsed `extra_items` annotation attached to a TypedDict class.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypedDictExtraItemsMetadata {
    pub annotation: String,
    pub annotation_expr: Option<TypeExpr>,
}

impl TypedDictExtraItemsMetadata {
    #[must_use]
    pub fn rendered_annotation(&self) -> String {
        self.annotation_expr
            .as_ref()
            .map(TypeExpr::render)
            .unwrap_or_else(|| self.annotation.clone())
    }
}

/// Parsed TypedDict class-level metadata.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypedDictClassMetadata {
    pub name: String,
    pub total: Option<bool>,
    pub closed: Option<bool>,
    pub extra_items: Option<TypedDictExtraItemsMetadata>,
    pub line: usize,
}

/// Source-derived metadata summary reused by later binding/checking phases.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct ModuleSurfaceMetadata {
    pub typed_dict_classes: Vec<TypedDictClassMetadata>,
    pub dataclass_transform: DataclassTransformModuleInfo,
    pub decorator_transform: DecoratorTransformModuleInfo,
    pub direct_function_signatures: Vec<DirectFunctionSignatureSite>,
    pub direct_method_signatures: Vec<DirectMethodSignatureSite>,
}

/// Conditional-return rule site collected from source.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ConditionalReturnSite {
    pub function_name: String,
    pub target_name: String,
    pub target_type: String,
    pub case_input_types: Vec<String>,
    pub line: usize,
}

/// Parsed `@dataclass_transform` metadata for a provider.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct DataclassTransformMetadata {
    pub field_specifiers: Vec<String>,
    pub kw_only_default: bool,
    pub frozen_default: bool,
    pub eq_default: bool,
    pub order_default: bool,
}

/// One decorator provider annotated with dataclass-transform metadata.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DataclassTransformProviderSite {
    pub name: String,
    pub metadata: DataclassTransformMetadata,
    pub line: usize,
}

/// Field metadata collected from a dataclass-transform target.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DataclassTransformFieldSite {
    pub name: String,
    pub annotation: String,
    pub annotation_expr: Option<TypeExpr>,
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

impl DataclassTransformFieldSite {
    #[must_use]
    pub fn rendered_annotation(&self) -> String {
        self.annotation_expr
            .as_ref()
            .map(TypeExpr::render)
            .unwrap_or_else(|| self.annotation.clone())
    }
}

/// Class site affected by dataclass-transform semantics.
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

/// Dataclass-transform summary for one module.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct DataclassTransformModuleInfo {
    pub providers: Vec<DataclassTransformProviderSite>,
    pub classes: Vec<DataclassTransformClassSite>,
}

/// Decorated callable discovered while scanning a module.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DecoratedCallableSite {
    pub owner_type_name: Option<String>,
    pub name: String,
    pub decorators: Vec<String>,
    pub line: usize,
}

/// Decorator-transform summary for one module.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct DecoratorTransformModuleInfo {
    pub callables: Vec<DecoratedCallableSite>,
}

/// One parameter in a direct callable signature surface.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DirectFunctionParamSite {
    pub name: String,
    pub annotation: Option<String>,
    pub annotation_expr: Option<TypeExpr>,
    pub has_default: bool,
    pub positional_only: bool,
    pub keyword_only: bool,
    pub variadic: bool,
    pub keyword_variadic: bool,
}

impl DirectFunctionParamSite {
    #[must_use]
    pub fn rendered_annotation(&self) -> Option<String> {
        self.annotation_expr.as_ref().map(TypeExpr::render).or_else(|| self.annotation.clone())
    }
}

/// Function signature recovered directly from source text.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DirectFunctionSignatureSite {
    pub name: String,
    pub params: Vec<DirectFunctionParamSite>,
    pub line: usize,
}

/// Method signature recovered directly from source text.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DirectMethodSignatureSite {
    pub owner_type_name: String,
    pub name: String,
    pub method_kind: MethodKind,
    pub params: Vec<DirectFunctionParamSite>,
    pub line: usize,
}

/// Mutation forms tracked for frozen dataclass fields.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FrozenFieldMutationKind {
    Assignment,
    AugmentedAssignment,
    Delete,
}

/// Frozen dataclass field mutation site.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FrozenFieldMutationSite {
    pub kind: FrozenFieldMutationKind,
    pub field_name: String,
    pub operator: Option<String>,
    pub target: DirectExprMetadata,
    pub value: Option<DirectExprMetadata>,
    pub owner_name: Option<String>,
    pub owner_type_name: Option<String>,
    pub line: usize,
}

/// Unsafe operation categories tracked by the parser.
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

/// Unsafe operation occurrence and whether it is already guarded by `unsafe:`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UnsafeOperationSite {
    pub kind: UnsafeOperationKind,
    pub line: usize,
    pub in_unsafe_block: bool,
}

/// Feature flags that enable optional parser-side analyses.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct ParseOptions {
    /// Enables collection of conditional-return rule sites.
    pub enable_conditional_returns: bool,
    /// Target Python version used for parser-side guard evaluation.
    pub target_python: Option<ParsePythonVersion>,
    /// Target platform used for parser-side guard evaluation.
    pub target_platform: Option<ParseTargetPlatform>,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct ParsePythonVersion {
    pub major: u8,
    pub minor: u8,
}

impl ParsePythonVersion {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (major, minor) = text.trim().split_once('.')?;
        Some(Self { major: major.parse().ok()?, minor: minor.parse().ok()? })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ParseTargetPlatform {
    Darwin,
    Linux,
    Win32,
    Other,
}

impl ParseTargetPlatform {
    #[must_use]
    pub fn current() -> Self {
        match std::env::consts::OS {
            "macos" => Self::Darwin,
            "linux" => Self::Linux,
            "windows" => Self::Win32,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub fn sys_platform_name(self) -> &'static str {
        match self {
            Self::Darwin => "darwin",
            Self::Linux => "linux",
            Self::Win32 => "win32",
            Self::Other => "other",
        }
    }
}
