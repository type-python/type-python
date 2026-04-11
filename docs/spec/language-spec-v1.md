# TypePython v1 Language Specification

**Status:** draft, normative for the first shippable v1 implementation  
**Scope:** language definition only  
**Numbering note:** original section numbering is preserved for stable reference.

This document is the normative language definition for TypePython v1. It covers terminology, syntax, types, expressions, statements, declarations, module semantics, type relations, and flow analysis.

Sections intentionally included here:

- Sections 1-3
- Sections 7-12 and 14-15
- Appendix A, Appendix B, Appendix D, Appendix E

Sections intentionally excluded from this document:

- project model, configuration, artifact authority, semantic elaboration and emission, diagnostics serialization, cache, CLI, and LSP
- conformance tiers and test obligations
- implementation notes and rollout guidance

---

## 1. Introduction

### 1.1 Positioning

TypePython is an authoring and tooling layer over Python for teams that want stronger static guarantees, cleaner typed-library publication, and a more structured migration path than ordinary annotation-only workflows provide.

- TypePython source lives in `.tpy` files.
- TypePython compiles to standard `.py` files.
- TypePython emits `.pyi` stubs as its declaration artifact.
- Generated output MUST run on a normal Python interpreter without requiring a mandatory TypePython runtime.
- The external type contract is expressed in ordinary Python typing forms so downstream tools consume the published package without needing TypePython-specific semantics.

TypePython is not a replacement interpreter. Its success criterion is publication and tooling interoperability, not ownership of the runtime.

### 1.2 Package Identity

The following identifiers are normative for the TypePython project:

| Aspect                        | Value                                   |
| ----------------------------- | --------------------------------------- |
| PyPI Package Name             | `type-python`                           |
| Python Import Root            | `typepython`                            |
| Source File Suffix            | `.tpy`                                  |
| Emitted Python Suffix         | `.py`                                   |
| Optional Cache Suffix         | `.pyc`                                  |
| Standalone Configuration File | `typepython.toml`                       |
| Embedded Configuration Table  | `[tool.typepython]` in `pyproject.toml` |

### 1.3 Terminology

| Term                     | Definition                                                                                |
| ------------------------ | ----------------------------------------------------------------------------------------- |
| **Source module**        | A `.tpy` file compiled by TypePython                                                      |
| **Pass-through module**  | A `.py` file copied unchanged into the output tree                                        |
| **Stub module**          | A `.pyi` file used for type information input                                             |
| **Public summary**       | The exported type surface of a module used for dependency invalidation                    |
| **Lowering**             | Syntactic translation from TypePython source to valid Python source                       |
| **Semantic elaboration** | Type-directed expansion that requires binding or declaration typing before final emission |
| **Dynamic boundary**     | A place where precise static typing cannot be guaranteed                                  |
| **Unsafe block**         | A syntactic region explicitly marked as opting into unsafely typed behavior               |
| **Authority**            | The artifact (`.py`, `.pyi`, summary, metadata) that is definitive for a given purpose    |
| **Feature tier**         | One of Core v1, DX v1, or Experimental v1                                                 |

---

## 2. Normative Conventions

The words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are used as defined in [RFC 2119](https://tools.ietf.org/html/rfc2119):

- **MUST** indicates a hard interoperability or correctness requirement. Violation constitutes a specification error.
- **MUST NOT** indicates a prohibition. Violation constitutes a specification error.
- **SHOULD** indicates a strong recommendation that may be deferred only with a documented reason and implementation notes.
- **SHOULD NOT** indicates a strong recommendation against a particular practice.
- **MAY** indicates an optional feature. Implementations MAY choose to support or omit.

### 2.1 Normative and Informative Content

Unless explicitly labeled otherwise:

- rule prose, numbered algorithms, decision procedures, and closed schemas in this document are normative
- examples, notes, rationale paragraphs, and compatibility commentary are informative
- Appendix A is informative and serves as a consolidated grammar cross-reference
- Appendix B, Appendix D, and Appendix E are normative where they define reserved feature boundaries, unsafe-boundary semantics, or compatibility constraints

### 2.2 Conflict Resolution and Precedence

If two normative statements in this document appear to conflict, precedence is resolved in the following order:

1. Explicit stepwise algorithms and closed schemas
2. Specific rule text governing the narrowest construct
3. General rule text
4. Summary tables
5. Examples and explanatory notes

If a remaining conflict appears between this document and the suite-level index, the suite-level boundary rules determine which document controls, but language acceptance and typing semantics are governed by this document.

### 2.3 Defined Specification Terms

This document uses the following additional terms:

- **implementation-defined**: the implementation MUST choose and document a behavior
- **host-defined**: behavior depends on the surrounding environment, such as filesystem, platform, or interpreter configuration, and the implementation MUST document the dependency when observable
- **unspecified**: an implementation may choose any behavior within the broader constraints of the specification and need not document that choice
- **undefined**: a program or tool interaction is outside the specification's guarantees; implementations MAY reject it, accept it, or behave arbitrarily

### 2.4 Conformance Boundary

This document constrains externally observable language behavior:

- whether a program is accepted or rejected
- the typed interpretation of declarations and expressions
- the narrowing and assignability results used to decide diagnostics

Internal implementation choices such as pass structure, caching layout, helper naming, or crate/module organization are non-normative unless another normative document explicitly states otherwise.

---

## 3. Core Design Goals and Non-Goals

### 3.1 Core Design Goals

TypePython v1 adopts the following design goals:

1. **Preserve Python runtime behavior.** Emitted `.py` MUST execute identically to the source intent.
2. **Static error detection.** Statically detect likely programmer errors before execution.
3. **Readable and stable emission.** Generated Python SHOULD be readable and formatter-stable. Emission quality is a quality-of-implementation goal, not a standalone conformance gate.
4. **Compile-time types.** Keep type information primarily in the compile-time domain.
5. **Gradual adoption.** Support incremental migration with explicit unsafe boundaries.
6. **Python ecosystem interoperability.** Interoperate directly with Python packages, stubs, and the standard library.
7. **Scalability.** Make large projects practical through deterministic builds, summaries, and incremental checking.
8. **Pluginless framework interop.** Common dataclass-like and metadata-driven library patterns SHOULD work through standard typing metadata rather than checker-specific plugins.
9. **Packaging-native workflow.** Fit ordinary Python packaging practice, including `pyproject.toml`, `py.typed`, and installed typed packages.
10. **Trustworthy library surfaces.** Make it practical to publish generated `.py` and `.pyi` with complete public API typing.
11. **Predictable migration profiles.** Provide a small number of deterministic adoption modes instead of an open-ended matrix of checker quirks.
12. **Authoring sugar, standard artifacts.** Source-level conveniences MUST lower to ordinary `.py` and standard `.pyi` forms so downstream Python tools do not need TypePython-specific semantics.

### 3.2 Explicit Non-Goals for v1

The following are explicitly outside Core v1 scope unless reintroduced in a later specification version:

- A new Python interpreter or alternative runtime
- Mandatory runtime type enforcement
- Full soundness with Python metaprogramming and reflection
- Complete checking for arbitrary existing `.py` source files
- Conditional types, mapped types, template literal types
- Intersection type syntax
- Emit targets below Python 3.10
- Project references (TypeScript composite builds)
- Checker-specific plugins as the required mechanism for mainstream framework typing

---

## 7. Lexical Structure and Syntax Extensions

### 7.1 Compatibility Baseline

TypePython inherits Python 3.10 lexical structure, indentation rules, comments, string literals, expression grammar, and statement grammar, except where this specification adds or reinterprets syntax.

TypePython MUST NOT change the runtime meaning of ordinary Python syntax.

### 7.2 Target Python Version

Core v1 targets Python 3.10, 3.11, or 3.12. The `target_python` configuration key MUST accept only these values.

This decision relies on:

- `match` statements
- `X | Y` union syntax
- Modern typing behavior expected by current tooling

### 7.3 Soft Keywords

TypePython introduces the following soft keywords, recognized only in specific syntactic positions:

| Keyword     | Position                                       |
| ----------- | ---------------------------------------------- |
| `typealias` | Statement start: `typealias Name = ...`        |
| `interface` | Statement start: `interface Name: ...`         |
| `data`      | Statement start: `data class Name: ...`        |
| `sealed`    | Statement start: `sealed class Name: ...`      |
| `overload`  | Declaration modifier: `overload def Name(...)` |
| `unsafe`    | Block start: `unsafe: ...`                     |

In all other positions, they remain ordinary identifiers.

### 7.4 Type Parameters

Core v1 introduces bracketed type parameter lists:

```python
def first[T](xs: Sequence[T]) -> T: ...
class Box[T]: ...
typealias Pair[T] = tuple[T, T]
```

**Grammar:**

```text
type_params       ::= '[' type_param (',' type_param)* ']'
type_param        ::= ('**')? NAME (':' type_param_restriction)? ('=' type_expr)?
type_param_restriction ::= type_expr | '(' type_expr (',' type_expr)+ ')'
type_param_list   ::= type_params
```

**Constraints:**

- A `TypeVar`-like parameter supports either one upper bound (`T: Base`) or a parenthesized constraint list (`T: (A, B)`).
- A `ParamSpec` uses `**P` syntax. It MAY declare a default, but it MUST NOT declare bounds or constraint lists.
- Once a type parameter with a default appears in a parameter list, every following type parameter in that list MUST also declare a default.

### 7.5 Grammar Extensions Summary

The following productions extend Python 3.10 grammar:

```text
// Type aliases
typealias_stmt    ::= 'typealias' NAME type_params? '=' type_expr

// Interfaces (structural protocols)
interface_stmt    ::= 'interface' NAME type_params? ('(' base_list? ')')? ':' suite

// Data classes
data_class_stmt   ::= 'data' 'class' NAME type_params? ('(' base_list? ')')? ':' suite

// Sealed classes
sealed_class_stmt ::= 'sealed' 'class' NAME type_params? ('(' base_list? ')')? ':' suite

// Overload declarations
overload_decl     ::= 'overload' 'def' NAME type_params? parameters ('->' type_expr)? ':' simple_suite

// Experimental conditional return type (not required for Core v1 parser conformance)
cond_return_type  ::= '->' 'match' NAME ':' NEWLINE INDENT cond_arm+ DEDENT
cond_arm          ::= 'case' type_expr ':' type_expr NEWLINE

// Unsafe blocks
unsafe_stmt       ::= 'unsafe' ':' suite

// Built-in type transforms
transform_expr    ::= ('Partial' | 'Required_' | 'Readonly' | 'Mutable') '[' type_expr ']'
                    | ('Pick' | 'Omit') '[' type_expr (',' STRING)+ ']'

// Generic functions (in parameters)
parameters        ::= '(' param_list? ')' (':' type_expr)?
param_list        ::= param (',' param)* (',' '*' param)? (',' '**' param)?
param             ::= NAME ('=' expression)?
```

### 7.6 Annotation Expressions

Within type annotations, TypePython supports:

- All valid Python typing expressions accepted by the target version
- TypePython intrinsic types: `dynamic`, `unknown`, `Never`
- Bracketed type parameter declarations on functions, classes, and type aliases

**Core v1 does not introduce `T?` shorthand.** Optional types MUST be written as `T | None`.

For parsing purposes, a Core v1 implementation MUST support at least the following type-expression grammar:

```text
type_expr           ::= union_type_expr
union_type_expr     ::= primary_type_expr ('|' primary_type_expr)*
primary_type_expr   ::= intrinsic_type_expr
                    | qualified_type_name
                    | string_literal
                    | '(' type_expr ')'
                    | tuple_type_expr
                    | subscript_type_expr

intrinsic_type_expr ::= 'dynamic' | 'unknown' | 'Never' | 'None'
qualified_type_name ::= NAME ('.' NAME)*
subscript_type_expr ::= qualified_type_name '[' type_arg_list ']'
type_arg_list       ::= type_expr (',' type_expr)* ','?
tuple_type_expr     ::= '(' type_expr ',' type_expr (',' type_expr)* ','? ')'
string_literal      ::= STRING
```

Notes:

- `Callable[[A, B], R]`, `type[T]`, `Literal[X]`, `Sequence[T]`, and similar forms are ordinary `subscript_type_expr` values.
- Arbitrary Python runtime expressions are not automatically valid `type_expr` values merely because they parse as ordinary expressions.
- A Core v1 implementation MAY accept a larger set of typing forms, but it MUST at least accept the grammar above.

### 7.7 Names

A **name** in TypePython is an identifier that designates a declared entity within a scope. Names are used to reference values, types, and modules.

#### 7.7.1 Identifier Production

TypePython inherits the Python identifier rules of the configured `target_python` lexer.

```
identifier ::= any token accepted as an identifier by the target Python version
```

A name MUST NOT shadow a hard keyword (see Section 7.7.2). A name MAY shadow a soft keyword when used in a non-keyword context.

#### 7.7.2 Reserved Words

TypePython distinguishes between hard keywords and soft keywords.

**Hard Keywords** (cannot be used as identifiers):

| Keyword    | Description                               |
| ---------- | ----------------------------------------- |
| `False`    | Boolean false literal                     |
| `None`     | None literal / null type                  |
| `True`     | Boolean true literal                      |
| `and`      | Logical conjunction                       |
| `as`       | Import alias                              |
| `assert`   | Assertion statement                       |
| `async`    | Async function modifier (future reserved) |
| `await`    | Await expression (future reserved)        |
| `break`    | Break statement                           |
| `class`    | Class definition                          |
| `continue` | Continue statement                        |
| `def`      | Function definition                       |
| `del`      | Del statement                             |
| `elif`     | Else-if clause                            |
| `else`     | Else clause                               |
| `except`   | Exception handler                         |
| `finally`  | Finally block                             |
| `for`      | For loop                                  |
| `from`     | Import from                               |
| `global`   | Global statement                          |
| `if`       | If statement                              |
| `import`   | Import statement                          |
| `in`       | Membership test                           |
| `is`       | Identity test                             |
| `lambda`   | Lambda expression                         |
| `nonlocal` | Nonlocal statement                        |
| `not`      | Logical negation                          |
| `or`       | Logical disjunction                       |
| `pass`     | Pass statement                            |
| `raise`    | Raise statement                           |
| `return`   | Return statement                          |
| `try`      | Try statement                             |
| `while`    | While loop                                |
| `with`     | With statement                            |
| `yield`    | Yield expression                          |

**Soft Keywords** (valid as identifiers outside their keyword contexts):

| Keyword     | Keyword Context                  | Non-Keyword Context |
| ----------- | -------------------------------- | ------------------- |
| `typealias` | Statement start                  | Ordinary identifier |
| `interface` | Statement start                  | Ordinary identifier |
| `data`      | Statement start (`data class`)   | Ordinary identifier |
| `sealed`    | Statement start (`sealed class`) | Ordinary identifier |
| `overload`  | Declaration modifier             | Ordinary identifier |
| `unsafe`    | Block start                      | Ordinary identifier |

#### 7.7.3 Property Names

A **property name** is a name that designates a member of a type, class, interface, or object. Property names appear after a dot (`.`) in member access expressions and in type member declarations.

A property name MAY be:

- Any valid identifier
- A string literal (e.g., `obj["method"]`)
- A valid Python special method name (`__init__`, `__str__`, etc.)

**Well-known special properties:**

| Property   | Context | Meaning               |
| ---------- | ------- | --------------------- |
| `__init__` | Class   | Constructor method    |
| `__str__`  | Class   | String representation |
| `__repr__` | Class   | Debug representation  |
| `__eq__`   | Class   | Equality comparison   |
| `__hash__` | Class   | Hash value            |
| `__call__` | Class   | Callable instances    |

A property name that is not a valid identifier (e.g., contains spaces) MUST be accessed via subscript notation `obj["property name"]` in the generated Python, but this form is not supported directly in TypePython type expressions.

---

## 8. Types

### 8.1 Intrinsic Types

#### 8.1.1 `dynamic`

`dynamic` is the escape hatch equivalent to TypeScript `any`.

**Rules:**

- `dynamic` is assignable to and from every type.
- Member access, calls, indexing, and arithmetic on `dynamic` are permitted without restriction.
- Operations involving `dynamic` generally produce `dynamic`.
- `no_implicit_dynamic = true` MUST diagnose any fallback to `dynamic` that was not explicit.

#### 8.1.2 `unknown`

`unknown` is the safe top-like boundary type.

**Rules:**

- Any value may be assigned to `unknown`.
- `unknown` may be assigned only to `unknown`, `dynamic`, and `object`, unless narrowed or cast.
- Member access, calls, indexing, and arithmetic on `unknown` are **errors** until narrowed.
- `unknown` SHOULD lower to `object` in emitted `.pyi` when no better external representation exists.

#### 8.1.3 `Never`

`Never` represents unreachable code or values that cannot exist.

**Use cases:**

- Functions that never return normally (e.g., `def fail() -> Never: raise ...`)
- Failed exhaustiveness checks
- Impossible branches after narrowing

#### 8.1.4 `None`

Represents the Python `None` value. In `strict_nulls = true` mode, `None` is not included in `T` unless explicitly written as `T | None`.

### 8.2 Type Forms Supported in Core v1

Core v1 MUST support:

- Named nominal class types
- Interface types (structural protocols)
- Type aliases
- Generic types with one upper bound per type parameter
- Union types
- Literal types based on Python literal values
- Tuples (fixed-length and variable-length)
- Callables
- Class-object types (`type[T]`)
- `Self` (from typing_extensions or Python 3.11+)
- Enum types (Section 8.10)
- `NewType` declarations and consumption
- `TypedDict` types (Section 9.2.2, Section 14.2)
- `Annotated`, `ClassVar`, `Required`, and `NotRequired` in their supported declaration positions
- `Final` bindings (Section 8.11)
- Abstract classes and methods (Section 8.12)

### 8.3 Nominal Typing

Ordinary classes are nominal. `class A` and `class B` are not mutually assignable solely because their fields match.

Classes derived from stdlib enum bases remain nominal class types. Their detailed typing and exhaustiveness behavior is defined in Section 8.10.

### 8.4 Structural Typing (Interfaces)

Interfaces are structural and lower to `typing.Protocol`.

If a concrete type has all required members with compatible types, it satisfies the interface **even without an explicit declaration**.

A class instance type MAY satisfy an interface structurally. Core v1 does not introduce an `implements` keyword.

### 8.5 Generic Types

#### 8.5.1 Supported Generic Declarations

Core v1 MUST support generic:

- Functions
- Classes
- Interfaces
- Type aliases

#### 8.5.2 Type Parameter Bounds

A type parameter MAY include one upper bound:

```python
def close_all[T: SupportsClose](xs: Sequence[T]) -> None: ...
```

#### 8.5.3 Unsupported Generic Features

Deferred beyond v1 features:

- Variance annotations in source syntax
- Higher-kinded types

#### 8.5.4 Generic Inference

The checker MUST perform call-site inference for generic functions.

Inference proceeds per call expression as follows:

1. Create one inference variable for each type parameter in the target signature.
2. Establish contextual types for argument expressions before inference. This includes lambda parameter contextual typing and literal contextual typing from the target parameter type.
3. For each argument/parameter pair, recursively infer from source type `S` (the argument type) to target type `T` (the parameter type):
   - If `T` is a type parameter `P`, record `S` as a candidate for `P`.
   - If `S` and `T` are instantiations of the same generic origin, recurse pairwise on corresponding type arguments.
   - If `S` and `T` are fixed-length tuples of the same arity, recurse elementwise.
   - If `T` is a union, infer against each branch that can accept `S`; if exactly one branch succeeds, use that branch; otherwise combine successful branch candidates.
   - If `S` is a union, infer each constituent against `T` and combine the resulting candidates.
   - If `S` and `T` are both callable types, recurse on corresponding parameter and return positions using the callable compatibility rules in Section 14.2.
   - If no rule applies, inference contributes no new candidates for that pair.
4. After collecting candidates for each type parameter `P`:
   - Discard candidates that violate the declared upper bound of `P`.
   - If no candidates remain for `P`, `P` is unresolved.
   - If exactly one candidate remains, that candidate is the inferred argument.
   - If multiple candidates remain, compute their normalized join if one exists deterministically; otherwise infer the normalized union of the candidates.
   - Literal candidates for the same primitive family MUST be widened before finalization unless the corresponding target position explicitly requires a `Literal[...]` type or an enum-member singleton.
5. Substitute the inferred type arguments into the target signature and re-check call applicability.
6. If any type parameter remains unresolved, or if substitution still leaves the call inapplicable, inference fails.

Inference MUST NOT invent a type argument from result context alone when no argument position constrains that parameter. Contextual typing may refine an inference site, but it does not replace a missing inference site.

If inference fails:

- Strict mode SHOULD diagnose
- Non-strict mode MAY fall back according to `no_implicit_dynamic` and `imports`

#### 8.5.5 `ParamSpec` and `Concatenate`

TypePython Core v1 supports both consuming and authoring `ParamSpec` and `Concatenate` in callable typing positions, including source-authored forwarding signatures that use `P.args` and `P.kwargs`.

A `ParamSpec` denotes an ordered callable parameter list preserving:

- parameter kind (positional-only, positional-or-keyword, keyword-only, variadic positional, variadic keyword)
- parameter name where Python call matching depends on it
- parameter type
- whether the parameter is required or optional for call applicability

`Callable[P, R]` denotes a callable with parameter list `P` and return type `R`.

`Callable[Concatenate[H1, ..., Hn, P], R]` denotes a callable whose parameter list begins with the fixed prefix `H1, ..., Hn`, followed by the parameter list `P`.

Core v1 consumption rules are:

- When generic inference matches a concrete callable source type against `Callable[P, R]`, `P` binds to the source callable's full parameter list and `R` binds from its return type.
- When generic inference matches a concrete callable source type against `Callable[Concatenate[H1, ..., Hn, P], R]`, the source callable MUST begin with parameters compatible with `H1, ..., Hn`; `P` then binds to the remaining source parameter list.
- After substitution, all ordinary call, assignability, and overload rules operate on the resulting concrete parameter list.
- A callable type whose parameter list still contains an unresolved `ParamSpec` or unresolved `Concatenate` tail is not directly callable in `.tpy` source. The checker MUST diagnose such a call (`TPY4014`) rather than guessing which arguments are accepted.
- Assignment compatibility between callable types with unresolved parameter-list variables succeeds only when the full parameter-list expressions are structurally identical after substitution, or when they derive from the same originating `ParamSpec` binding with identical fixed `Concatenate` prefixes.
- In source-authored `.tpy`, `*args: P.args` and `**kwargs: P.kwargs` are valid only when they refer to the same in-scope `ParamSpec`; they contribute that parameter-list variable to callable typing, inference, and forwarding.
- If an emitted `.pyi` surface still contains unresolved `ParamSpec` or `Concatenate`, the emitter MUST preserve those forms rather than erasing them to `dynamic`.

### 8.6 Type Aliases

Type aliases are compile-time names for types.

**Rules:**

- Aliases do not create new nominal types.
- Recursive type aliases are supported in Core v1.
- Implementations MUST preserve recursive alias structure during normalization and comparison rather than silently widening recursive aliases to `dynamic`.

### 8.7 Member Categories

Types in TypePython consist of members drawn from the following categories:

#### 8.7.1 Property Members

A **property member** declares a named attribute on a type:

```python
class Point:
    x: float
    y: float
```

Property members have:

- A name (identifier)
- A type
- Optionality (required vs. optional via `= None` default)

#### 8.7.2 Method Members (Call Signatures)

A **method member** declares a callable on a type:

```python
class Calculator:
    def add(self, a: int, b: int) -> int: ...
```

Method members have:

- A name
- A signature (parameter types + return type)
- An implicit receiver type for instance and class methods

#### 8.7.3 Index Signatures

An **index signature** declares a wildcard member access pattern:

```python
class StringDict:
    def __getitem__(self, key: str) -> str: ...
    def __setitem__(self, key: str, value: str) -> None: ...
```

In interface syntax:

```python
interface StringDict:
    def __getitem__(self, key: str) -> str: ...
    def __setitem__(self, key: str, value: str) -> None: ...
```

Core v1 does not support Python-style `[key: str]: str` index signature shorthand; explicit `__getitem__`/`__setitem__` methods MUST be used.

#### 8.7.4 Constructor Types

A **constructor type** represents the type of a class that can be instantiated. In TypePython, constructor types are expressed via `type[T]`:

```python
def create(factory: type[Point]) -> Point:
    return factory()
```

The `type[T]` form represents:

- A class object that can be called to construct instances of `T`
- The constructor's signature (parameter types + return type of the constructed instance)

**Constructor type relationships:**

- `type[A]` is assignable to `type[B]` if `A` is a subtype of `B` (covariant in the instance type).
- A class `C` has type `type[C]`.
- An interface does not have a constructor type unless explicitly modeled.
- When `T` is a generic instantiation (e.g., `type[list[int]]`), `type[list[int]]` denotes the class object `list` constrained such that construction produces `list[int]`. However, because Python class objects are not themselves parameterized at runtime, `type[list[int]]` is assignable from `type[list]` and vice versa; the generic argument serves only as documentation of intent. Stricter checking of generic `type[...]` is deferred beyond v1.
- `type[T]` where `T` is a type parameter with bound `B` denotes a class object that, when called, returns an instance of `T`. Member access on `type[T]` resolves class-level (static and class method) members of `B`.

#### 8.7.5 `Self` and Receiver Types

TypePython Core v1 treats the implicit receiver of a method as a polymorphic self type:

- In an instance method declared inside class `C`, an unannotated first parameter named `self` has the `Self` type of `C`, meaning "the most-derived instance type at the call site".
- In a class method, an unannotated first parameter named `cls` has type `type[Self]` for the enclosing class.
- In a static method, there is no implicit receiver.
- Within a class body, the annotation `Self` refers to the receiver-preserving type of the enclosing class.
- A method declared in base class `A` with return type `Self` is inherited by subclass `B` as returning `B`, unless an override declares a different but override-compatible type.
- If the first receiver parameter is explicitly annotated, that annotation MUST be compatible with the implicit receiver type; an incompatible receiver annotation is an error.

Core v1 MUST support `Self` in parameter, return, and attribute annotations in `.tpy`, `.py`, and `.pyi` inputs.

### 8.8 Class-Object Distinction

TypePython maintains a distinction between:

1. **Instance type** (`C`): The type of instances of class `C`
   - Members are instance methods and properties
   - Accessed via `C` in type positions

2. **Constructor type** (`type[C]`): The type of the class object itself
   - Represents the callable that constructs instances
   - Accessed via `type[C]` in type positions

```python
class Builder:
    def build(self) -> Product: ...

# Instance type
b: Builder

# Constructor type
factory: type[Builder]
```

This distinction is important for factory patterns and dependency injection.

### 8.9 Recursive Types

A **recursive type** is a type that references itself in its definition:

```python
class Node:
    value: int
    next: Node | None  # Recursive reference
```

**Rules for recursive types:**

1. Recursive types are permitted in:
   - Class definitions
   - Interface definitions
   - Type aliases
2. Recursive comparison proceeds coinductively: once the checker is comparing the same pair of recursive shapes again, it MAY assume that recursive relationship holds for the purpose of the current comparison.
3. Implementations MUST preserve recursive structure during normalization instead of widening recursive occurrences to `dynamic`.

**Recursive type normalization:**

When comparing recursive types `S` and `T`:

- The checker assumes the relationship holds for recursive occurrences
- Normalization proceeds by unwrapping one level at a time

```python
# Recursive aliases are supported in Core v1
type LinkedList[T] = T | list[LinkedList[T]]
```

Core v1 supports recursive alias expansion for structural types. Implementations MUST preserve those recursive relationships during comparison and inference rather than fabricating `dynamic` placeholders.

### 8.10 Enum Types

Python `enum.Enum` subclasses are a common pattern for defining closed sets of named constants. TypePython Core v1 MUST support them as follows:

#### 8.10.1 Enum Class Typing

A class whose direct or transitive base includes `enum.Enum`, `enum.IntEnum`, `enum.StrEnum`, `enum.Flag`, or `enum.IntFlag` (or their equivalents from the bundled typing source) is an **enum type**.

```python
import enum

class Color(enum.Enum):
    RED = 1
    GREEN = 2
    BLUE = 3
```

**Typing rules:**

- Each enum member (e.g., `Color.RED`) has a singleton literal type `Literal[Color.RED]` that widens to the enum class type `Color` per Section 8.15.
- The enum class object itself has type `type[Color]`.
- Enum members are not assignable to `int` or `str` unless the enum inherits from `IntEnum` or `StrEnum` respectively.
- Enum classes are nominal: two distinct enum classes with identical members are NOT mutually assignable.
- Classes derived from `enum.Flag` or `enum.IntFlag` are **flag enums**. Their declared members are singleton values, but bitwise combinations may produce additional runtime values beyond the declared member list.

#### 8.10.2 Enum Exhaustiveness

When a `match` statement or chain of `if`/`elif` comparisons covers all members of a known non-flag enum class:

- The checker SHOULD treat the domain as finite and perform exhaustiveness analysis.
- Missing members SHOULD be named in the diagnostic.
- Enum exhaustiveness follows the same rules as sealed-class exhaustiveness in Section 10.6.

For `enum.Flag` and `enum.IntFlag`, the checker MUST NOT assume that the declared members form a finite exhaustive domain unless a later specification version introduces an explicit restricted-mode rule for flag enums.

#### 8.10.3 Enum Limitations in Core v1

- Enum declaration via TypePython-specific syntax (e.g., `enum class`) is deferred beyond v1; users MUST use standard Python `class Color(enum.Enum)`.
- `enum.auto()` values are treated as opaque; the checker does not infer their runtime values.
- Dynamic enum creation via `Enum("Color", ...)` functional form is treated as a `dynamic` boundary in strict mode.
- Flag combination (`Color.RED | Color.BLUE`) typing follows the dunder protocol rules for `__or__` on the enum class and does not imply exhaustiveness over the declared member set.

### 8.11 `Final` Declarations

A declaration annotated with `Final` (from `typing` or `typing_extensions`) indicates a binding that MUST NOT be reassigned after initialization.

```python
from typing import Final

MAX_SIZE: Final = 100
MAX_SIZE = 200  # Error: cannot reassign Final binding
```

**Rules:**

- `Final` may appear on module-level variables, class-level attributes, and local variables.
- A `Final` variable with a literal initializer and no explicit type annotation retains its literal type without widening (e.g., `MAX_SIZE` above has type `Literal[100]`, not `int`).
- A `Final` variable with an explicit type annotation uses that annotation: `x: Final[int] = 100` has type `int`.
- Reassignment to a `Final` binding MUST be diagnosed as an error (`TPY4006`).
- Subclass overrides of a `Final` class attribute MUST be diagnosed.
- `Final` in function parameter position is not supported in Core v1.

### 8.12 Abstract Classes and Methods

TypePython Core v1 MUST recognize `abc.ABC`, `abc.ABCMeta`, and `abc.abstractmethod` from the standard library:

- A class using `ABCMeta` as metaclass or inheriting from `ABC` is **abstract-capable**.
- A method decorated with `@abstractmethod` is an **abstract method**.
- A class is an **abstract class** if, after collecting inherited abstract methods and removing methods concretely implemented in the class body or earlier in the MRO, its effective abstract-member set is non-empty.
- Direct instantiation of an abstract class (calling its constructor) MUST be diagnosed as an error.
- A subclass that does not implement all inherited abstract methods remains abstract. Implementations MAY diagnose this at the class declaration site; construction of such a class MUST be diagnosed.
- Abstract methods participate in override compatibility checking per Section 10.5.
- Abstract classes MAY be used as type annotations normally; only construction is restricted.

### 8.12.1 Standard Typing Wrappers

TypePython Core v1 MUST consume the following standard wrapper forms when they appear in `.tpy`, `.py`, or `.pyi` annotations:

- `Annotated[T, m1, m2, ...]`
- `ClassVar[T]`
- `Required[T]`
- `NotRequired[T]`
- `ReadOnly[T]`

Their Core v1 meanings are:

- `Annotated[T, ...]` has the same core static type relation as `T` for assignability, inference, overload resolution, and flow analysis, unless another rule in this specification explicitly inspects its metadata.
- Metadata carried by `Annotated[...]` MUST be preserved in public summaries and emitted `.pyi` when that metadata is part of a public annotation surface and can be represented in standard Python typing syntax.
- `ClassVar[T]` is valid only for class-scope attribute declarations. It contributes to the class-object surface, not the instance surface, and it is not a dataclass field or dataclass-transform field.
- Use of `ClassVar[...]` outside a class-attribute declaration in `.tpy` source MUST be diagnosed.
- `Required[T]` and `NotRequired[T]` are valid only in `TypedDict` item declarations. They determine requiredness of the containing key and otherwise contribute the value type `T`.
- Contradictory requiredness wrappers for the same `TypedDict` item in `.tpy` source MUST be diagnosed.
- `ReadOnly[T]` is valid only in `TypedDict` item declarations in Core v1. It marks the key as non-writable after construction and otherwise contributes the value type `T`.
- `ReadOnly[...]` MAY be nested with `Required[...]`, `NotRequired[...]`, and `Annotated[...]` in either order.

`TypedDict` supports `closed=True` and `extra_items=` in Core v1.

- `closed=True` means keys not declared in the `TypedDict` body are rejected unless accepted through `extra_items=`.
- `extra_items=T` admits undeclared string keys whose values have type `T`.
- `ReadOnly[...]` MAY wrap the `extra_items=` value type to make those undeclared-key writes non-writable after construction.

### 8.12.2 `NewType`

TypePython Core v1 MUST consume `typing.NewType` and `typing_extensions.NewType` declarations.

For static typing purposes:

- `UserId = NewType("UserId", int)` creates a distinct nominal type `UserId` whose runtime constructor is the ordinary `NewType` callable.
- The base of a `NewType` declaration in Core v1 MUST be a nominal class type or another `NewType`; declarations whose base is an arbitrary union, literal, protocol, or structural type MUST be diagnosed in `.tpy` source.
- A value of `UserId` is assignable to its base type `int`.
- A value of the base type `int` is not assignable to `UserId` without an explicit `UserId(...)` construction or `cast`.
- A call `UserId(x)` is well-typed when `x` is assignable to the declared base type, and the result type is `UserId`.
- Public summaries and emitted `.pyi` MUST preserve the `NewType` declaration rather than erase it to the base type.

### 8.13 Nullability

**If `strict_nulls = true`:**

- `T` does NOT include `None`
- Optionality MUST be expressed as `T | None`
- `None` flow narrowing MUST be enforced

**If `strict_nulls = false`:**

- `None` is assignment-compatible with any type except `Never` and explicit `Literal[...]` singleton domains.
- A declaration of type `T` and a declaration of type `T | None` are treated as assignment-compatible in both directions unless another rule explicitly inspects `None` as a distinct case.
- The checker MUST NOT require `None` guards before ordinary member access solely because a value might be `None`.
- Flow analysis MAY still narrow explicit `is None` or `is not None` tests, but those narrowings MUST NOT be required for acceptance of code that is otherwise valid under non-strict nulls.

### 8.14 Built-in Type Transforms (Utility Types)

TypePython v1 Core provides a set of **compiler-built-in type transforms** that operate on known `TypedDict` types at compile time and are elaborated to fully expanded standard typing forms in emitted `.pyi`. Downstream tools consume the expanded output without needing to understand the transforms.

These transforms are authoring-time sugar only. The authoritative downstream contract is the expanded emitted `.pyi` surface, not preservation of the source transform syntax.

#### 8.14.1 Supported Transforms

The following transforms are normative for Core v1:

| Transform              | Input       | Meaning                                          |
| ---------------------- | ----------- | ------------------------------------------------ |
| `Partial[T]`           | `TypedDict` | All items become optional via `NotRequired[...]` |
| `Required_[T]`         | `TypedDict` | All items become required                        |
| `Readonly[T]`          | `TypedDict` | All items become read-only via `ReadOnly[...]`   |
| `Mutable[T]`           | `TypedDict` | Removes read-only constraint from all items      |
| `Pick[T, K1, K2, ...]` | `TypedDict` | Retains only the named items                     |
| `Omit[T, K1, K2, ...]` | `TypedDict` | Removes the named items                          |

The name `Required_` (trailing underscore) avoids collision with `typing.Required` which operates on individual `TypedDict` items. An implementation MAY additionally accept `AllRequired` as a synonym.

```python
# .tpy source
from typing import TypedDict

class User(TypedDict):
    id: int
    name: str
    email: str

typealias UserCreate = Omit[User, "id"]
typealias UserUpdate = Partial[Omit[User, "id"]]
typealias UserPublic = Pick[User, "id", "name"]
```

#### 8.14.2 Transform Rules

1. Each key argument to `Pick` and `Omit` MUST be a string literal naming a field that exists on `T`. Unknown field names MUST be diagnosed (`TPY4017`).
2. Transforms compose left-to-right: `Partial[Omit[T, "x"]]` first removes `"x"`, then makes the remainder optional.
3. The result of a transform is a new anonymous `TypedDict`-compatible structural type. It is NOT a subtype of `T` unless the resulting item set is compatible under normal `TypedDict` assignability rules.
4. Transforms MAY be applied to generic `TypedDict` aliases; the transform is applied after generic substitution at usage sites.
5. Applying a transform to a type that is not a known `TypedDict` type or `TypedDict` alias MUST be diagnosed (`TPY4017`).
6. Transforms MUST NOT be used as base classes or in `isinstance` checks — they are compile-time-only type operators.
7. Applying transforms directly to `data class`, `interface`, nominal class, or protocol declarations is deferred until a later specification version defines a first-class record/shape model for those constructs.

#### 8.14.3 Lowering of Transforms

Transforms are expanded during lowering. The emitted `.pyi` contains the fully materialized result:

```python
# Emitted .pyi for UserCreate = Omit[User, "id"]
class UserCreate(TypedDict):
    name: str
    email: str

# Emitted .pyi for UserUpdate = Partial[Omit[User, "id"]]
class UserUpdate(TypedDict):
    name: NotRequired[str]
    email: NotRequired[str]
```

The expanded form MUST be deterministic: identical transform input produces identical `.pyi` output.

#### 8.14.4 Transform Application to Inherited Types

When `T` is a `TypedDict` that inherits from other `TypedDict` declarations:

- `Pick` and `Omit` operate on the flattened item set, including inherited items.
- `Partial`, `Required_`, `Readonly`, and `Mutable` apply to all flattened items.
- The result is a flat `TypedDict`-compatible type and does NOT preserve the original inheritance chain.

### 8.15 Widened Types

Literal expressions have immediate literal types per Section 9.2, but inference does not preserve literal precision in every binding site.

Unless a more specific contextual type is already present, TypePython Core v1 MUST widen literals as follows when inferring a durable variable or container element type:

- `Literal[42]` widens to `int`
- `Literal["x"]` widens to `str`
- `Literal[True]` / `Literal[False]` widen to `bool`
- Enum-member singleton literals widen to their enum class type

Widening MUST occur for:

- Unannotated variable declarations and rebindings
- Inference of list, set, and dict element types without a contextual target type
- Finalization of generic inference candidate sets unless the target position explicitly requires `Literal[...]`

Widening MUST NOT occur for:

- Explicitly annotated `Literal[...]` positions
- Pattern matching against literal or enum-member cases
- The immediate type of a literal expression before it is captured into a wider inferred type
- Overload and interface member declarations that explicitly mention literal types

---

## 9. Expressions

### 9.1 Expression Typing Overview

Every expression has a static type determined by the checker. The type is used for assignability checking, flow analysis, and code generation.

### 9.2 Literal Expressions

| Literal          | Type                               |
| ---------------- | ---------------------------------- |
| `42`             | `Literal[42]`                      |
| `"hello"`        | `Literal["hello"]`                 |
| `True` / `False` | `Literal[True]` / `Literal[False]` |
| `None`           | `None`                             |

Each literal expression first receives the type above. Any later widening follows Section 8.15.

#### 9.2.1 Collection Literal Inference

Without a contextual target type:

- A list literal `[e1, ..., en]` has type `list[J]`, where `J` is the normalized join of the widened element types.
- A set literal `{e1, ..., en}` has type `set[J]`, where `J` is the normalized join of the widened element types.
- A tuple literal `(e1, ..., en)` has fixed-length type `tuple[T1, ..., Tn]`, where each `Ti` is inferred for the corresponding element and widened only as required by Section 8.15.
- Empty list, set, and dict literals without contextual type are inference failures in strict mode.

With a contextual target type, the elements MUST be checked against that target before falling back to unconstrained literal inference.

#### 9.2.2 Dict and `TypedDict` Literals

Without a contextual `TypedDict` target, a dict literal `{k1: v1, ..., kn: vn}` has type `dict[K, V]`, where:

- `K` is the normalized join of the widened key types
- `V` is the normalized join of the widened value types

TypePython Core v1 does not invent anonymous object-literal types for dict literals.

If the contextual target type is a known `TypedDict`, the checker MUST validate the literal keywise:

- Requiredness of each target key is determined after applying `Required[...]` and `NotRequired[...]` wrappers from the authoritative `TypedDict` declaration surface.
- Read-only status of each target key is determined after applying `ReadOnly[...]` wrappers from the authoritative `TypedDict` declaration surface.
- Each statically known key MUST be a string literal matching a declared `TypedDict` key.
- Every required key MUST be present.
- Omitted optional keys are permitted.
- Unknown keys are errors (`TPY4013`).
- Each provided value MUST be assignable to the declared value type for that key.

Failure of any of the contextual `TypedDict` literal rules above MUST be diagnosed as `TPY4013`.

An ordinary `dict[K, V]` value is not treated as a `TypedDict` solely because its key and value joins happen to match.

### 9.3 Name References

A name reference resolves to a declaration. The type of a name reference is the declared or inferred type of that declaration.

If a name cannot be resolved:

- In strict mode: **error**
- In non-strict mode: MAY be `dynamic`

### 9.4 Member Access

**`attr_expr.member`** has type determined by:

1. The type of `attr_expr`
2. The declared type of `member` in that type
3. Flow narrowing applied to `attr_expr`

**Errors:**

- Accessing a non-existent member on a known type is an error.
- Accessing members on `unknown` is an error until narrowed.

### 9.5 Call Expressions

A call `f(args)` is well-typed if the callee exposes one or more call signatures and at least one applicable signature accepts the argument list.

#### 9.5.1 Signature Applicability Algorithm

To determine whether a concrete signature is applicable to a call site, the checker MUST perform the following procedure:

1. Resolve the callee to a concrete signature or overload candidate.
2. Establish contextual types for argument expressions before final applicability checks. This includes lambda parameter contextual typing, literal contextual typing, and generic inference inputs from Section 8.5.4.
3. Check Python arity and parameter-kind compatibility.
4. Match each supplied positional and keyword argument against the corresponding parameter.
5. Apply generic inference and substitute inferred arguments into the candidate signature.
6. Re-check assignability using the instantiated signature. The candidate is applicable only if every supplied argument is assignable and no required parameter remains unsatisfied.

At minimum, applicability requires all of the following:

1. The call satisfies Python arity and parameter-kind rules.
2. Each supplied argument is assignable to the corresponding parameter type after contextual typing and generic inference are applied.
3. Extra positional arguments are permitted only if the callable accepts `*args`.
4. Extra keyword arguments are permitted only if the callable accepts `**kwargs` or `**kwargs: Unpack[TD]` for a known `TypedDict` `TD` that can accept the supplied names.
5. A keyword argument naming a declared parameter MUST match that parameter's kind and name.
6. A keyword argument matched through `**kwargs: Unpack[TD]` MUST name a declared `TypedDict` key and have a value assignable to that key's value type.

For parameter declarations:

- `*args: T` accepts additional positional arguments of type `T` and has body type `tuple[T, ...]`.
- `**kwargs: T` accepts additional keyword arguments whose values have type `T` and has body type `dict[str, T]`.
- `**kwargs: Unpack[TD]`, where `TD` is a known `TypedDict`, accepts keyword arguments according to the declared keys and requiredness of `TD` and has body type `TD`.

#### 9.5.2 Starred Argument Expansion at Call Sites

When a caller passes starred arguments, the checker MUST unpack them:

- `f(*xs)` where `xs: tuple[A, B, C]` contributes three positional arguments of types `A`, `B`, `C` respectively.
- `f(*xs)` where `xs` has variadic tuple type `tuple[T, ...]`, `Sequence[T]`, or `list[T]` contributes an unknown-length positional suffix of type `T`. Such an expansion is well-typed only if the remaining positional acceptance is through `*args`, through an unresolved-but-preserved `ParamSpec`, or through a `dynamic` callable boundary.
- `f(**kw)` where `kw: dict[str, T]` does not deterministically satisfy named parameters, because the key set is unknown. Such an expansion is well-typed only if the callee accepts `**kwargs` whose value type can accept `T`, or if the callee is a `dynamic` callable boundary.
- `f(**kw)` where `kw` is a `TypedDict` contributes named keyword arguments for each declared key:
  - a required `TypedDict` key may satisfy a required or optional keyword-acceptable parameter of the same name
  - an optional `TypedDict` key may satisfy only an optional parameter, unless flow analysis has already proven the key is present
  - any declared `TypedDict` key not accepted by the callee MUST be rejected unless the callee has `**kwargs`
  - required keyword-only or positional-or-keyword parameters of the callee are satisfied by `**kw` only when some expansion source guarantees the key is present
  - `ReadOnly[...]` on a `TypedDict` item does not change whether that keyword may be supplied at the call site; it constrains later mutation of the `TypedDict` value, not call applicability

Invalid `**TypedDict` expansion MUST be diagnosed as `TPY4013`.

#### 9.5.3 Overload Resolution Algorithm

Overload resolution proceeds after applicability filtering:

1. Normalize each overload by applying generic inference from Section 8.5.4 and rejecting signatures whose inference or arity checks fail.
2. Collect the remaining applicable overloads. This includes overloads from the `overload def` soft keyword AND overloads consumed from `.py`/`.pyi` sources decorated with `@typing.overload`. Both forms produce the same call-signature set.
3. Signature `A` is more specific than signature `B` if each corresponding non-variadic parameter type in `A` is a subtype of the corresponding parameter type in `B`, at least one comparison is strict, and `A` does not rely on a more permissive variadic or `dynamic`/`unknown` parameter where `B` has a concrete parameter.
4. If a unique most specific candidate exists, select it and use its instantiated return type.
5. If no applicable overload exists, diagnose ordinary call incompatibility (`TPY4001`).
6. Otherwise, diagnose ambiguity (`TPY4012`).

### 9.6 Binary and Unary Expressions

| Operator                      | Operand Types        | Result Type                                                                                     |
| ----------------------------- | -------------------- | ----------------------------------------------------------------------------------------------- |
| `+`, `-`, `*`, `/`, `//`, `%` | numeric              | numeric (widest operand)                                                                        |
| `+`                           | strings              | `str`                                                                                           |
| `+`                           | sequences            | sequence type                                                                                   |
| `==`, `!=`                    | any                  | `bool`                                                                                          |
| `<`, `>`, `<=`, `>=`          | comparable           | `bool`                                                                                          |
| `and`, `or`, `not`            | any                  | `bool` (for `not`), union (for `and`/`or` with truthiness narrowing)                            |
| `in`                          | `x in container`     | `bool`; the checker MUST verify `container` supports membership via `__contains__` or iteration |
| `not in`                      | `x not in container` | `bool`                                                                                          |
| `is`, `is not`                | any                  | `bool`; participate in narrowing (Section 15)                                                   |

Operations involving `dynamic` produce `dynamic`. Operations involving `unknown` produce `unknown` unless narrowed.

Short-circuit boolean operators participate in flow analysis. The right-hand operand of `and` and `or` MUST be checked in the branch environment induced by the left-hand operand as defined in Section 15.

Dunder-protocol dispatch: For operator expressions on user-defined types, the checker resolves the corresponding dunder method (e.g., `__add__`, `__eq__`, `__lt__`, `__contains__`) and uses its declared return type. If both a forward dunder (e.g., `__add__`) and a reflected dunder (e.g., `__radd__`) exist, the checker follows Python's standard resolution order: forward method first, reflected method of the right operand if the forward method returns `NotImplemented` or is absent.

### 9.7 Indexing

`xs[i]` has type:

- The element type of `xs` if `xs` is a generic subscriptable type
- `dynamic` if `xs` is `dynamic`
- `unknown` if indexing `unknown` or with an unknown key

For a known `TypedDict` type `TD` indexed by a string-literal key:

- `td["k"]` has the declared value type of key `"k"` if `"k"` is a declared key of `TD`
- indexing by a statically known undeclared key MUST be diagnosed
- indexing by a non-literal key is permitted only under the conservative rules already implied by `unknown` or `dynamic` boundaries

### 9.8 Ternary Expressions

`x if cond else y` has type:

- The union of the types of `x` and `y`
- `Never` if `cond` is provably `True` or `False` at analysis time

The true and false branches MUST be checked under the narrowed environments `EnvTrue(cond)` and `EnvFalse(cond)` respectively, following the compositional narrowing rules in Section 15.

### 9.9 Lambda Expressions

A lambda expression `lambda params: body` produces a callable type.

- If the lambda is contextually typed by a callable target (Section 9.12.3), unannotated parameters receive their types from the contextual signature.
- If no contextual type is available, unannotated parameters are typed as `dynamic` (when `no_implicit_dynamic = false`) or diagnosed (when `no_implicit_dynamic = true`).
- The return type of a lambda is inferred from the body expression.
- `.tpy` source MAY spell lambda-local parameter annotations with a parenthesized parameter list:

```python
lambda (x: int, y: str): f(x, y)
```

- When a parenthesized lambda parameter list is used, each authored annotation contributes that parameter's type directly.
- Unannotated parameters inside the same parenthesized lambda parameter list continue to use contextual typing when available and otherwise fall back to the ordinary unannotated-lambda rules above.
- The parenthesized lambda parameter list uses the ordinary Python lambda parameter grammar for defaults and `*` / `**` markers; the extra parentheses are required whenever source-authored lambda parameter annotations are present.

### 9.10 Type Assertions (`cast`)

TypePython Core v1 supports type assertions via `typing.cast`:

```python
from typing import cast

x: object = get_value()
y = cast(int, x)  # y has type int
```

**Rules:**

- `cast(T, expr)` has type `T` regardless of the type of `expr`.
- `cast` has no runtime effect; the emitted `.py` preserves the `cast` call, which returns its argument unchanged.
- `cast` does NOT validate the assertion at compile time. It is the programmer's responsibility to ensure correctness.
- If `warn_unsafe = true`, the checker SHOULD emit a warning when `cast` is used outside an `unsafe:` block and the source type is not plausibly related to the target type (e.g., `cast(int, "hello")`).
- `cast` is the only type assertion mechanism in Core v1. TypePython does NOT introduce an `as` type-assertion syntax.

### 9.11 Walrus Operator (`:=`)

The assignment expression `target := expr` (Python 3.8+) is supported in TypePython Core v1:

- The type of the overall expression is the type of `expr`.
- The binding `target` receives that type in subsequent flow.
- `target` MUST be a simple name; attribute access or subscript targets are not permitted.
- Walrus operators within type expressions are NOT supported (see Appendix E).

### 9.12 Contextual Typing

**Contextual typing** is the process by which the type of an expression is determined by its surrounding context rather than solely from the expression itself.

TypePython implements contextual typing in the following scenarios:

#### 9.12.1 Argument Typing

When a function is called, the parameter types provide a contextual type for the argument expressions:

```python
def process(items: list[int]) -> None: ...

# Contextual type for [1, 2, 3] is list[int]
process([1, 2, 3])
```

The argument `[1, 2, 3]` is contextually typed as `list[int]`, so the literal elements are inferred as `int`.

#### 9.12.2 Return Type Context

When a function body contains a bare expression in a return statement, the function's declared return type provides contextual type:

```python
def get_value() -> int:
    return 42  # Contextual type is int

def get_list() -> list[str]:
    return ["a", "b"]  # Contextual type is list[str]
```

#### 9.12.3 Lambda and Callable Context

When a lambda is passed as an argument, the target parameter type provides contextual typing for the lambda parameters:

```python
def map_func(fn: Callable[[int], str]) -> list[str]: ...

# Parameter 'x' is contextually typed as int
map_func(lambda x: str(x))
```

Contextual signature instantiation works as follows:

1. If the target callable type is non-generic, unannotated lambda parameters receive their types directly from the target signature.
2. If the target callable type is generic, the checker MUST first instantiate that signature from surrounding call-site information that does not depend on the lambda body.
3. If that instantiation is unique, the instantiated parameter types become the contextual types for the lambda parameters.
4. If no unique instantiation exists, the checker MUST NOT infer outer generic arguments solely from the lambda body. In strict mode, explicit outer type arguments or explicit lambda parameter annotations are required.

#### 9.12.4 Assignment Contextual Typing

In assignment statements, the target variable's type provides contextual type for the right-hand side:

```python
point: tuple[int, int] = (10, 20)
# Both 10 and 20 are contextually typed as int
```

#### 9.12.5 Type Inference Interaction

Contextual typing interacts with type inference:

1. **Inference from context first**: The contextual type is established before inference runs
2. **Inference can refine**: If the contextual type is a union or generic, inference refines within that context
3. **Conflict detection**: If inference produces a type incompatible with context, diagnose type mismatch

```python
# Contextual typing with generic
def first[T](items: Sequence[T], default: T) -> T: ...

# T is inferred as int from the contextual type of default
result = first([1, 2], 0)  # T = int
```

**Not Supported in Core v1:**

- Full bidirectional type checking
- Contextual typing for object literals with implicit property types (TypeScript-style)
- Automatic inference of generic type arguments from context alone (must have explicit bounds or inference site)

---

## 10. Statements

### 10.1 Statement Typing Overview

Statements do not have types directly but create bindings, affect control flow, or produce types through narrowing.

### 10.2 Import Statements

**Resolution priority for external modules:**

1. Explicit local `.tpy` source in the current project
2. Explicit local `.pyi` in the current project
3. Explicit local pass-through `.py` with usable annotations
4. Installed package stubs
5. Installed packages marked `py.typed`
6. Fallback according to `typing.imports`

**Untyped imports:**

- `imports = "unknown"`: imported values are typed as `unknown`
- `imports = "dynamic"`: imported values are typed as `dynamic`

`unknown` is the Core v1 default.

Imported `typing.Any` is treated as `dynamic` by the checker.

### 10.3 Assignment Statements

**Type inference for assignments:**

- If annotated: use the annotation
- If unannotated: infer from the right-hand side expression after applying contextual typing and widening rules
- If inference fails and `no_implicit_dynamic = true`: **diagnose**
- Otherwise: MAY fall back to `dynamic`

**Assignment compatibility:**

- RHS type MUST be assignable to LHS type
- `Never` is assignable to any type
- Any type is assignable to `dynamic`
- Any type is assignable to `unknown`

**Valid assignment targets (references):**

- A simple name
- An attribute access expression
- An index expression
- A tuple or list destructuring pattern composed of valid targets, with at most one starred target

All other left-hand-side expressions are invalid assignment targets and MUST be diagnosed (`TPY4011`).

**Tuple and list unpacking:**

- If the RHS has a fixed-length tuple type, arity MUST match exactly unless one starred target captures the remaining elements.
- A non-starred unpack target receives the corresponding element type.
- A starred target receives `list[T]`, where `T` is the normalized join of the captured element types.
- If the RHS is only known as `Sequence[T]` or `Iterable[T]`, each bound non-starred target receives `T`; arity is diagnosed only when it is statically knowable.

**Augmented assignment:**

- `x op= y` requires `x` to be both readable and writable.
- The checker MUST type `x op y` using the current type of `x` and require the result to be assignable back to `x`.
- If `x` is a known `TypedDict` item access whose target key is read-only, augmented assignment MUST be diagnosed as `TPY4016`.

**`del` targets:**

- `del` may be applied only to valid reference targets.
- Deleting a local name removes the binding from subsequent flow analysis until the name is rebound.
- Deleting a known read-only `TypedDict` item MUST be diagnosed as `TPY4016`.

**Direct `TypedDict` item mutation:**

- Assigning through `td["k"] = value` is valid only if `td` is not known to declare `"k"` as read-only, and `value` is assignable to the declared item type.
- Assignment to a known read-only `TypedDict` item MUST be diagnosed as `TPY4016`.

### 10.4 Function Definitions

**Return type inference:**

- If annotated: use the annotation
- If unannotated: infer from all return paths using the following rules:
  - A bare `return` or `return None` contributes `None` to the set of return types.
  - A function body that falls through without a `return` statement contributes `None`.
  - If the function body consists solely of a single `raise` statement or an unconditional call to a `Never`-returning function, the inferred return type is `Never`.
  - Otherwise, the inferred return type is the union of all contributed return types, after widening per Section 8.15.
  - If the function directly or indirectly references itself (mutual recursion) through unannotated return paths, the inferred return type for the cycle is `dynamic`. Adding an explicit return annotation to any function in the cycle breaks it.
- If inferred return type contains `dynamic` and `no_implicit_dynamic = true`: **diagnose**

**Generic functions:**

- Type parameters are in scope within the function signature and body
- Type parameters must be bound at each call site

**Parameter forms:**

- An annotated default value is checked against the declared parameter type.
- `*args: T` has body type `tuple[T, ...]` and accepts additional positional arguments of type `T`.
- `**kwargs: T` has body type `dict[str, T]` and accepts additional keyword argument values of type `T`.
- `**kwargs: Unpack[TD]`, where `TD` is a known `TypedDict`, has body type `TD` and accepts keyword arguments according to the declared key set of `TD`.
- Other source-authored `Unpack[...]` parameter forms remain deferred beyond v1 unless they arise only through imported typing surfaces whose semantics are already fixed by Section 8.5.5.

**Async and generator constructs in Core v1 bodies:**

- User-authored `async def`, `await`, `yield`, `yield from`, `async for`, and `async with` are part of Core v1 in `.tpy` source.
- Their typing follows the standard `Awaitable`, `Coroutine`, `Generator`, `AsyncIterator`, and async context-manager protocols from the bundled typing surface.
- Imported `.py` and `.pyi` declarations using `Awaitable`, `Coroutine`, `Generator`, `Iterator`, and related typing forms remain consumable.

### 10.5 Class Definitions

- Create a nominal type
- Class body establishes the type's members
- A class-body attribute annotated with `ClassVar[T]` contributes only to the class-object surface unless later sections give it additional descriptor semantics
- Declared base classes contribute inherited members according to Python's C3 MRO
- If incompatible inherited members with the same name are visible and no explicit override resolves the conflict, the checker MUST diagnose
- `data class` lowers to `@dataclass`
- `sealed class` marks the class as the root of a sealed hierarchy whose closure is the set of subclasses defined in the same module as the sealed root
- `interface` lowers to `Protocol`

**Sealed closure boundary in Core v1:**

- A class that directly or indirectly inherits from a sealed root MUST be declared in the same module as that sealed root.
- Subclassing a sealed root from another module MUST be diagnosed using the same inheritance-constraint category as an invalid override or forbidden subclass relation (`TPY4005`).
- Exhaustiveness over a sealed root is computed from the sealed root plus the transitive subclass set found in the defining module's final public summary.

**Override compatibility:**

- An overriding member MUST preserve member kind: instance method, class method, static method, property, and data attribute are distinct categories.
- An overriding callable member is valid only if the override is assignable to the overridden callable type under the callable compatibility rules in Section 14.2.
- This implies parameter checking is contravariant and return checking is covariant for override purposes.
- A readable property MAY narrow its getter result covariantly.
- A writable property setter MUST accept at least the base property's writable type.

**`super()` typing:**

- Zero-argument `super()` is valid only inside instance methods and class methods.
- Member lookup begins at the next class in the current class's MRO after the enclosing class.
- The resulting member is then bound using the current receiver type (`Self` for instance methods, `type[Self]` for class methods).

**Builtin decorator typing in Core v1:**

- `@property` converts a zero-argument instance method into a readable property whose type is the getter return type.
- `@name.setter` introduces a writable property; the setter parameter type defines the accepted write type and MUST be compatible with the getter type.
- `@classmethod` removes the implicit instance receiver and binds the first parameter as `type[Self]`.
- `@staticmethod` removes all implicit receiver binding.
- `@final` from `typing` or `typing_extensions` MUST be enforced for finality diagnostics:
  - a method decorated with `@final` MUST NOT be overridden in a subclass
  - a class decorated with `@final` MUST NOT be subclassed
  - violations use the same diagnostic category as incompatible override or inheritance constraints (`TPY4005`)
- `@override` from `typing` or `typing_extensions` MUST be accepted on overriding methods, class methods, static methods, and properties:
  - it is an error if the decorated member does not override a compatible member from a base class
  - violations use the same diagnostic category as incompatible override or inheritance constraints (`TPY4005`)
- If `typing.require_explicit_overrides = true`, any overriding method, class method, static method, or property in `.tpy` source MUST be marked with `@override`.
- `@deprecated` from `warnings` and `typing_extensions` MUST be consumed as a static deprecation marker on functions, methods, classes, properties, and overload items:
  - use of a deprecated declaration MUST produce diagnostic `TPY4101` with severity controlled by `typing.report_deprecated`
  - if overload resolution selects a deprecated overload item, that selected item triggers the deprecation diagnostic
  - direct `from x import deprecated_name` in `.tpy` source counts as a use for this purpose
  - the deprecation message, when statically known, SHOULD be included in the diagnostic text or notes
- `typing.dataclass_transform` and `typing_extensions.dataclass_transform` define the standard pluginless mechanism for dataclass-like framework typing in Core v1.
- Core v1 additionally recognizes **typed callable decorator transforms** for ordinary functions and methods.
- A decorator contributes a callable transform when its statically resolved callable surface is a single-argument callable whose first parameter is `Callable[...]` and whose return type is also `Callable[...]`.
- Decorator application order is Python's normal bottom-up order: for `@a` above `@b`, the effective callable surface is `a(b(fn))`.
- The effective callable surface of the decorated declaration is the return callable surface after generic substitution from the undecorated callable argument.
- In `.tpy` source, such generic decorator transforms MAY use ordinary source-authored type parameters, including `ParamSpec`, on the decorator declaration itself.
- If a decorator's effect cannot be reduced to a statically known callable-to-callable mapping, strict mode SHOULD diagnose and non-strict mode MAY treat the decorated declaration as an undecorated or `dynamic` boundary.

**Dataclass-like transforms (`dataclass_transform`) in Core v1:**

- A decorator function, base class, or metaclass marked with `dataclass_transform` defines a dataclass-like transform surface.
- When a class is decorated by such a decorator, derives from such a transformed base, or uses such a transformed metaclass, the checker MUST synthesize dataclass-like field and initializer semantics from statically known transform metadata.
- At minimum, synthesis MUST:
  - collect annotated instance fields from the class body and transformed bases in declaration order
  - exclude methods, descriptors without dataclass-transform field metadata, and `ClassVar` attributes from the field set
  - determine whether a field is required from an explicit default value, a recognized field-specifier default, or transform defaults that are statically known
  - synthesize an `__init__` signature using the discovered field order and any statically known `kw_only` or `init` controls
  - treat statically known `frozen=True` as prohibiting instance-field assignment after initialization
  - when `eq=True` (or when the transform default is `eq=True`), the checker SHOULD synthesize `__eq__` and `__hash__` members consistent with the standard `@dataclass` behavior for assignability and protocol-satisfaction purposes
  - when `order=True`, the checker SHOULD synthesize comparison-method members (`__lt__`, `__le__`, `__gt__`, `__ge__`)
- Recognized field-specifier effects in Core v1 are limited to statically known `default`, `default_factory`, `init`, `kw_only`, and `alias`.
- If transform semantics depend on runtime-computed metadata or framework-specific behavior beyond the standardized `dataclass_transform` contract, the checker MUST NOT guess. Strict mode SHOULD diagnose or mark only the affected synthesized behavior as a `dynamic` boundary; non-strict mode MAY degrade at that boundary.
- `data class` remains the built-in TypePython sugar for the standard-library `@dataclass` case.

### 10.6 Match Statements

**Supported patterns for exhaustiveness:**

- Wildcard `_`
- Literal patterns
- Class patterns without guards
- OR-patterns composed of supported patterns

Guard expressions do not contribute to exhaustiveness proof in Core v1.

**Coverage checking:**

- If coverage is incomplete, diagnostic MUST name the missing subclasses or literal values
- Sealed hierarchies are checked if `enable_sealed_exhaustiveness = true`
- Enum classes with statically known members SHOULD be treated as finite domains for exhaustiveness checking and diagnostics SHOULD name the missing members

### 10.7 `with` Statements

A `with` item `expr as target` is well-typed if `expr` has `__enter__` and `__exit__` members compatible with the standard context-manager protocol from the bundled typing source.

- The type of `target`, when present, is the return type of `expr.__enter__()`.
- If no `as target` clause is present, the `__enter__` result is ignored.
- Multiple `with` items are processed left-to-right, and each item contributes bindings independently.
- An `async with` item is well-typed if `expr` has `__aenter__` and `__aexit__` members compatible with the standard async context-manager protocol; the type of `target`, when present, is the awaited result of `expr.__aenter__()`.

### 10.8 `for` Statements

A `for` loop `for target in expr` is well-typed if `expr` has an `__iter__` method returning an iterator with a `__next__` method, or if `expr` is a known `Iterable[T]` or `Sequence[T]`.

- The type of `target` is the element type `T` yielded by the iterator.
- If `target` is a destructuring pattern (e.g., `for a, b in pairs`), the element type is unpacked per the tuple unpacking rules in Section 10.3.
- The optional `else` clause of a `for` loop is reachable only when the loop terminates normally (without `break`). The checker MUST NOT narrow types differently in the `else` clause unless it can prove that the loop body executed at least once.
- An `async for` loop is well-typed if `expr` has `__aiter__` returning an async iterator whose `__anext__` result is awaitable; the loop target receives the yielded element type of that async iterator.

### 10.9 `try` / `except` Statements

- In an `except` clause, the exception variable (if present) is bound to the caught exception type.
- `except ExcType as e` binds `e` with type `ExcType`.
- `except (ExcA, ExcB) as e` binds `e` with type `ExcA | ExcB`.
- A bare `except:` clause binds no variable; the implicit exception type is `BaseException`.
- The `else` clause is checked in the environment where no exception was raised from the `try` block.
- The `finally` clause is checked in the joined environment of all preceding branches.
- Exception variables bound in `except` clauses are implicitly deleted at the end of the clause (per Python runtime semantics); the checker MUST treat them as unbound after the clause exits.

### 10.10 Comprehensions

List, set, dict, and generator comprehensions follow expression-level typing:

- `[expr for x in iterable]` has type `list[T]` where `T` is the type of `expr` in the loop body context.
- `{expr for x in iterable}` has type `set[T]`.
- `{k: v for x in iterable}` has type `dict[K, V]`.
- `(expr for x in iterable)` has type `Generator[T, None, None]`.
- Nested `for` clauses and `if` filters are processed left-to-right, and each filter applies narrowing to subsequent clauses and the output expression.
- Comprehension variables are scoped to the comprehension and do not leak into the enclosing scope.

---

## 11. Declarations

### 11.1 Declaration Types

| Declaration                     | Syntax                             | Lowers To                          |
| ------------------------------- | ---------------------------------- | ---------------------------------- |
| Type alias                      | `typealias Name = Type`            | `Name: TypeAlias = Type`           |
| Interface                       | `interface Name: ...`              | `class Name(Protocol): ...`        |
| Data class                      | `data class Name: ...`             | `@dataclass class Name: ...`       |
| Sealed class                    | `sealed class Name: ...`           | `class Name: ...` (with metadata)  |
| Overload def                    | `overload def f(...): ...`         | `@overload def f(...): ...`        |
| Experimental conditional return | `def f(x: A \| B) -> match x: ...` | Multiple `@overload` declarations  |
| Type transform                  | `Partial[T]`, `Pick[T, ...]`, etc. | Expanded structural type           |
| Unsafe block                    | `unsafe: ...`                      | `if True: ...` (no runtime effect) |

### 11.1.1 Experimental Conditional Return Types (Overload Sugar)

Conditional return syntax is an Experimental v1 feature. A Core v1 implementation MAY reject it entirely. If an implementation enables it explicitly, it MUST follow the rules in this subsection.

Like the built-in type transforms, conditional return syntax is an authoring-time surface only. Interoperability with downstream Python tooling is defined by the lowered overload set that appears in emitted `.pyi`, not by the source `-> match` notation.

**Syntax:**

```python
def decode(x: str | bytes | None) -> match x:
    case str: str
    case bytes: str
    case None: None
```

**Semantics:**

1. The `match` target MUST name a parameter of the enclosing function.
2. Each `case` arm specifies a type pattern and a corresponding return type.
3. The union of all case arm input types MUST cover the declared parameter type. If it does not, the checker MUST diagnose incomplete conditional-return coverage (`TPY4018`).
4. Case arms are evaluated in declaration order for overload specificity.

**Lowering:**

The compiler MUST lower a conditional return type to a sequence of `@overload` declarations followed by the implementation:

```python
# Emitted .py and .pyi
from typing import overload

@overload
def decode(x: str) -> str: ...
@overload
def decode(x: bytes) -> str: ...
@overload
def decode(x: None) -> None: ...

def decode(x: str | bytes | None) -> str | None:
    ...  # original body
```

**Constraints:**

- Conditional return types are permitted only at the top level of a function return-type annotation, not nested inside other type expressions.
- Each case arm pattern MUST be a type expression, not a value pattern.
- The implementation body is type-checked against the union of all return types.
- Generic functions MAY use conditional return types; type parameters are in scope within the case arms.
- A function with a conditional return type MUST NOT also have separate `overload def` declarations; the two forms are mutually exclusive.

```python
# Generic conditional return
def first_or_none[T](xs: Sequence[T] | None) -> match xs:
    case Sequence[T]: T
    case None: None
```

### 11.2 Declaration Scope

TypePython uses Python-like lexical scoping with additional compile-time scopes for type parameters.

- **Module scope**: top-level declarations participate in the module namespace; names beginning with `_` are non-public unless exported via `__all__`
- **Class scope**: class members participate in the class type's member map; member lookup follows Python MRO at runtime and declared member sets at type-check time
- **Function scope**: parameters and local variables are function-local
- **Block scope**: control-flow blocks do not create a new runtime scope, but the checker MAY track block-local narrowing environments
- **Type-parameter scope**: type parameters are in scope within their declaring function, class, interface, or type alias signature and body where applicable

### 11.2.1 Declaration Spaces

A **declaration space** is a region of the program in which names are uniquely associated with declarations. TypePython defines the following declaration spaces:

1. **Module Declaration Space**: Contains all top-level declarations (classes, functions, type aliases, interfaces) in a module. Names in this space are accessible via import.

2. **Class Declaration Space**: Contains all instance and static members declared within a class body. Nested classes, interfaces, and type aliases within a class belong to the class's declaration space but are not considered instance members.

3. **Interface Declaration Space**: Contains all members declared within an interface body. Interface members are purely structural.

4. **Function Declaration Space**: Contains parameters and local declarations within a function body. Type parameters from generic functions are in the function declaration space.

5. **Type Parameter Declaration Space**: Each generic declaration (function, class, interface, type alias) introduces a type parameter declaration space scoped to that declaration's signature and body.

6. **Stub Declaration Space (`.pyi`)**: Declarations in `.pyi` files are in the stub declaration space. Stub declarations are not executable but contribute type information.

### 11.2.2 Name Conflicts Within Declaration Spaces

If a name resolves to multiple declarations in the same declaration space, the compiler MUST diagnose a duplicate declaration error (TPY4004) unless a later section explicitly permits merging.

**Rules for name conflicts:**

| Declaration Space | Permitted Duplicates                                         | Conflict Conditions                       |
| ----------------- | ------------------------------------------------------------ | ----------------------------------------- |
| Module            | None (except via explicit export merging deferred beyond v1) | Two top-level declarations with same name |
| Class             | Methods with different overloads                             | Two instance variables with same name     |
| Interface         | None                                                         | Two members with same name                |
| Function          | Parameters with same name                                    | Duplicate parameter names                 |
| Type Parameter    | None                                                         | Two type parameters with same name        |

**Cross-space name independence:**

In TypePython Core v1, declaration spaces are primarily an analysis tool. Module, class, and function bodies still correspond to ordinary Python runtime namespaces, so TypePython does NOT support TypeScript-style declaration merging across those spaces.

- A module MUST NOT declare both a function `foo` and a class `foo` in the same body.
- A class MUST NOT declare both a method `bar` and a nested class `bar` in the same body.
- Type parameter names form a compile-time-only declaration space, but they still shadow outer names within their lexical scope.

### 11.2.3 Import Namespace

Imported names occupy the module declaration space. If an import introduces a name that already exists in the module declaration space:

- **Explicit import alias**: `from x import foo as bar` creates name `bar`, does not conflict with `foo`
- **Star import**: `from x import *` imports all exported names; if conflict exists, the local declaration takes precedence
- **Named import**: `from x import foo` conflicts with existing local `foo`; compiler MUST diagnose (TPY4004)

### 11.3 Public API Detection

For `.pyi` emission and static star-import expansion, public names are determined as:

1. If `__all__` is present as a statically known literal sequence, it is authoritative
2. Otherwise, top-level names beginning without `_` are public

A checker processing `from x import *` MUST apply the same rule to the authoritative type surface for `x` (`.tpy` public summary, `.pyi`, or annotated pass-through `.py`).

### 11.4 Package Re-Exports (`__init__` Modules)

An `__init__.tpy` (or `__init__.py`) file defines the public surface of its containing package. Names imported in `__init__.tpy` are considered **re-exports** under the following rules:

1. If `__all__` is defined in `__init__.tpy`, it is authoritative: only the listed names are public re-exports.
2. If `__all__` is absent, a name imported via `from .sub import Foo` or `from .sub import Foo as Foo` is a public re-export (the explicit-alias-to-same-name pattern signals re-export intent, following PEP 484 convention).
3. A name imported via `from .sub import _Bar` (leading underscore) is NOT a public re-export unless listed in `__all__`.
4. The public summary of `__init__.tpy` MUST include all re-exported names and their types so that downstream `from pkg import Foo` resolves correctly.
5. These rules also apply to `__init__.py` pass-through files when consumed for type information.

### 11.5 Public API Type Completeness

TypePython Core v1 distinguishes between an internal type that is sufficient to continue checking and a public type surface that is safe to publish to downstream users.

A public declaration surface is **type-complete** when every exported function signature, overload item, class member type, base-type reference, type alias target, and module-level value annotation is expressed without:

- `dynamic`
- unresolved `unknown` that escaped inference or cycle stabilization
- erased `Annotated[...]` metadata that was present on the public source annotation

The following do NOT by themselves make a public surface incomplete:

- references to external named types imported from authoritative `.pyi` or typed-package surfaces
- opaque runtime implementation details in private names
- internal use of `dynamic` inside a function body that does not leak into the exported type surface

If `typing.require_known_public_types = true`, any non-complete exported type surface MUST be diagnosed as `TPY4015`.

---

## 12. Modules and Imports

### 12.1 Module Identity

A module is identified by its logical import path. For a file `src/pkg/sub/mod.tpy` with `root_dir = "src"` and `src = ["src"]`, the module identity is `pkg.sub.mod`.

### 12.2 Relative Imports

Relative imports follow Python package semantics:

- `.` refers to the current package
- `..` refers to the parent package

Relative import spellings MUST be emitted unchanged into generated `.py`.

### 12.3 Import Cycles

Import cycles are permitted if Python runtime semantics would permit them.

Type checking within a strongly connected component MUST use a deterministic provisional-summary rule:

- Each module in the cycle contributes the exported declarations whose signatures can be read without recursively requiring a not-yet-final summary from another module in the same cycle.
- Any exported type that still depends on unresolved cyclic information during that pass is treated as `unknown` within the cycle.
- Once the strongly connected component reaches a stable public summary, that stable summary replaces the provisional one for subsequent checking.

The compiler MUST avoid infinite recursion and SHOULD produce targeted diagnostics when an import cycle forces provisional `unknown` typing for an exported surface.

### 12.4 Resolution Order

For a project-local import, the resolver searches in this order:

1. Local `.tpy` module
2. Local `.pyi` module
3. Local `.py` module
4. Configured `type_roots`
5. Installed packages and bundled stdlib typing sources

The resolver MUST ignore `out_dir` as an input root.
The resolver MUST also ignore `cache_dir` as an input root.

### 12.5 Third-Party Resolution Priority

For any imported external module, type information MUST be resolved in this order:

1. explicit local `.tpy` source in the current project
2. explicit local `.pyi` in the current project
3. explicit local pass-through `.py` with usable annotations
4. installed package stubs
5. installed packages marked `py.typed`
6. fallback according to `typing.imports`

If no type information is available:

- `imports = "unknown"` means imported values are typed as `unknown`
- `imports = "dynamic"` means imported values are typed as `dynamic`

`unknown` is the default for Core v1.

### 12.5.1 Installed Typed Packages, Stub Packages, and Partial Stubs

TypePython Core v1 MUST follow PEP 561-style installed typing metadata closely enough for interoperable package consumption.

At minimum:

- An installed package containing `py.typed` is treated as an inline-typed package.
- An installed distribution that provides importable `.pyi` modules for a package is treated as a stub package and takes precedence over the corresponding runtime `.py` modules for the modules it defines.
- If a stub package is explicitly marked partial (for example, via a `py.typed` marker indicating partial coverage), the resolver MUST merge the stub package and runtime package trees, preferring `.pyi` where present and falling through to the runtime package where absent.
- If no partial-stub marker is present, provided stub modules are authoritative for the modules they define.
- Installed-package resolution MUST use `resolution.python_executable` when configured, except in an explicitly documented safe structural verification mode that avoids executing a project-controlled interpreter. In that safe mode, the implementation MUST use a deterministic default interpreter and SHOULD surface that interpreter or override in verbose output, logs, command notes, or environment diagnostics.

### 12.6 Standard Library and Typeshed

TypePython Core v1 MUST ship with a pinned typeshed snapshot or an equivalent bundled standard-library type source.

- The bundled snapshot is the source of truth for stdlib typing.
- The snapshot MUST be filtered by `target_python`.
- Namespace packages in third-party ecosystems and in the current project MUST participate in deterministic module discovery and import resolution.

A Core v1 checker MUST understand the semantics needed to consume at least the following standard typing constructs when they appear in imported `.py`/`.pyi` inputs, and in ordinary annotation positions of `.tpy` source that do not require features deferred beyond v1:

- `Annotated`
- `ClassVar`
- `NewType`
- `TypedDict`
- `Required` and `NotRequired`
- `ReadOnly`
- `Unpack` when used for `**kwargs: Unpack[TypedDict]`
- `Enum` and related stdlib enum bases
- `Final`
- `override`
- `deprecated`
- `TypeGuard` and `TypeIs`
- `ParamSpec` and `Concatenate`
- `Iterator`, `Generator`, `Awaitable`, and `Coroutine`
- `ContextManager` and `AbstractContextManager`
- `property`, `classmethod`, and `staticmethod`
- `dataclass_transform`

TypePython Core v1 does not introduce separate source syntax for those constructs; they are used through ordinary Python imports and annotations. Their imported meanings are determined by the bundled typing source and the additional rules in this specification.

### 12.6.1 `typing` and `typing_extensions` Equivalence

For every construct in Section 12.6 that exists in both `typing` and `typing_extensions`, the checker MUST treat the two spellings as semantically equivalent for the configured `target_python`.

For the deprecation decorator, the checker MUST likewise treat `warnings.deprecated` and `typing_extensions.deprecated` as semantically equivalent static deprecation markers.

Emission rules for target-version compatibility:

- If a required emitted typing construct exists in `typing` for the configured `target_python`, the emitter SHOULD prefer `typing`.
- If the construct is not available from `typing` for the configured `target_python` but is available from `typing_extensions`, the emitter MUST use `typing_extensions` in emitted `.pyi` when that construct must appear in the public surface.
- For the deprecation decorator, the emitter SHOULD prefer `warnings.deprecated` when available for the configured `target_python`, and otherwise MUST use `typing_extensions.deprecated` if available.
- If neither import location is available for the configured `target_python`, the emitter MUST diagnose `TPY5001` rather than silently erasing the construct.

### 12.6.2 Target-Version Compatibility Matrix

Core v1 targets Python 3.10, 3.11, and 3.12 runtimes, but it also consumes newer typing constructs through `typing_extensions` and other standardized backport locations. Emitters MUST therefore follow a deterministic compatibility matrix.

| Construct                                        | Preferred source when available                              | Backport or fallback for Core v1 targets | Core v1 emit rule                                                                                                                             |
| ------------------------------------------------ | ------------------------------------------------------------ | ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `TypeAlias`                                      | `typing.TypeAlias`                                           | none required for supported targets      | Emit assignment-style aliases using `TypeAlias`. Core v1 does not emit the Python 3.12 `type` statement.                                      |
| `Self`                                           | `typing.Self` (3.11+)                                        | `typing_extensions.Self`                 | For target 3.10 emit `typing_extensions.Self`; otherwise prefer `typing.Self`.                                                                |
| `Required`, `NotRequired`, `dataclass_transform` | `typing` (3.11+)                                             | `typing_extensions`                      | For target 3.10 emit `typing_extensions`; for 3.11+ prefer `typing`.                                                                          |
| `override`                                       | `typing.override` (3.12+)                                    | `typing_extensions.override`             | For targets 3.10 and 3.11 emit `typing_extensions.override`; for 3.12 prefer `typing.override`.                                               |
| `ReadOnly`, `TypeIs`                             | stdlib spellings exist only after the supported target range | `typing_extensions`                      | For all Core v1 targets, emit `typing_extensions.ReadOnly` and `typing_extensions.TypeIs` when these constructs appear in the public surface. |
| `deprecated` decorator                           | `warnings.deprecated` (outside the supported target range)   | `typing_extensions.deprecated`           | For all Core v1 targets emit `typing_extensions.deprecated` when a deprecation decorator must appear in emitted code or stubs.                |

Additional deterministic rules:

- Core v1 `.py` and `.pyi` emit MUST use legacy-compatible generic materialization (`TypeVar`, `Generic`, and ordinary annotation syntax) rather than Python 3.12 native type-parameter syntax.
- A Core v1 implementation MAY parse or consume newer syntax in external inputs only when another tool in the environment has already lowered or normalized it to a form compatible with the configured `target_python`.
- If multiple compatible spellings are available for a target, the emitter MUST choose one deterministically and use the same choice for equivalent declarations within a build.

---

## 14. Type Relationships and Assignability

### 14.1 Assignability Rules

A type `S` is assignable to type `T` if one of the following holds:

| Condition                                      | Description                          |
| ---------------------------------------------- | ------------------------------------ |
| `S == T`                                       | Identical types                      |
| `T == dynamic`                                 | Any type assignable to `dynamic`     |
| `S == Never`                                   | `Never` is bottom, assignable to any |
| `S == None` and `T` includes `None`            | `None` in union or optional          |
| `S` is subtype of `T`                          | Nominal or structural subtyping      |
| `S` is union and all members assignable to `T` | Union assignability                  |
| `T` is union and `S` assignable to any member  | Union assignability                  |
| `S` is `Literal[X]` and `T` is base type       | Literal to base                      |

Assignment compatibility is used for variable binding, parameter passing, and return checking. It is distinct from subtyping:

- `dynamic` is assignable to and from every type, but is not thereby a subtype of every type.
- `unknown` accepts assignment from every type, but is assignable out only under the rules in Section 8.1.2 unless narrowed.
- Overload specificity, declared variance, and structural subtype comparisons MUST use subtyping rules rather than mere assignability.
- `Annotated[T, ...]` participates in core type relations as `T` unless another section explicitly gives meaning to its metadata.

### 14.2 Subtyping Rules

**Nominal subtyping:** `class B(A)` makes `B` a subtype of `A`.

**`NewType` subtyping:** A `NewType` declaration creates a distinct nominal subtype of its declared base for static assignability purposes, subject to the construction rule in Section 8.12.2.

**Structural subtyping:** If `T` is an interface/protocol, `S` is a subtype if it has all required members with compatible types.

**Generic subtyping:** `Generic[T]` is a subtype of `Generic[U]` if `T` is assignable to `U` and the variance permits.

In Core v1, generic type parameters declared directly by TypePython source are invariant by default. No source-level covariance or contravariance syntax is supported.

When consuming `.py` or `.pyi` declarations, declared variance from the authoritative typing surface (for example `TypeVar(..., covariant=True)` or `TypeVar(..., contravariant=True)`) MUST be respected. The bundled typeshed snapshot is therefore normative for variance of stdlib generics such as `Sequence`, `Mapping`, and `Callable`.

**Callable compatibility:** A source callable type `S` is compatible with target callable type `T` if the following ordered procedure succeeds:

1. Every call accepted by `T` is also accepted by `S`.
2. Parameter types are checked contravariantly.
3. Return types are checked covariantly.
4. Positional-only, positional-or-keyword, and keyword-only parameters are matched using Python's ordinary call rules.
5. Optional parameters in the target type may correspond only to parameters the source callable can also accept when provided.
6. `*args` element types and `**kwargs` value types are checked contravariantly.
7. The source callable MUST NOT require arguments that callers of the target callable are not guaranteed to provide.

**`TypedDict` compatibility:** `TypedDict` compatibility in Core v1 is intentionally conservative and mutation-aware.

For a source `TypedDict` type `S` to be assignable to a target `TypedDict` type `T`, all of the following MUST hold:

- Every key declared in `T` is also declared in `S`.
- For every key shared by `S` and `T`, requiredness MUST match exactly.
- For every key shared by `S` and `T`:
  - if the target key is writable, the source key MUST also be writable, and the value types MUST be mutually assignable after alias normalization
  - if the target key is read-only, the source key MAY be writable or read-only, and the source value type MUST be assignable to the target value type
- `S` MAY declare additional keys beyond those in `T`.

This rule applies equally to contextual `TypedDict` literal checking and to assignment between named `TypedDict` aliases.

An ordinary `dict[K, V]` is not automatically a subtype of a `TypedDict`, and a `TypedDict` is not automatically a subtype of `dict[str, V]`.

### 14.3 Type Equality

Two types are equal if:

- They are the same nominal type
- They are the same literal value
- They are unions with identical members
- They are generic instances with equal type arguments

### 14.4 Apparent Members

The **apparent members** of a type are the members considered during subtype checking, assignability, and member access. The apparent members of a type are determined as follows:

**For nominal class types:**

- Declared instance members in the class and inherited members from base classes
- Members from `object` (e.g., `__str__`, `__eq__`) that are not overridden
- If the type has call/construct capabilities, members from callable interface

**For interface types (structural protocols):**

- All declared members
- Members from base interfaces
- Standard `object` members unless explicitly overridden

**For union types:**

- A member is an apparent member of a union if it is present in ALL constituent types
- The type of the apparent member is the union of the corresponding member types from each constituent

```python
# Example: apparent member in union
type A = int | str

# A has __add__ only if both int and str have __add__
# Since both have __add__ but with different signatures,
# the apparent member is the union of the signatures
```

**For type parameters:**

- Apparent members are derived from the constraint type
- If unconstrained, behaves as having no apparent members until narrowed

**For `dynamic`:**

- Has all members (access is permitted without static checking)

**For `unknown`:**

- Has no apparent members until narrowed (any access is an error)

### 14.5 Member Identity

Two members are considered **identical** when:

1. **Property members** if they have:
   - The same name
   - The same type (or covariantly assignable types)
   - Same optionality (both required or both optional with same default type)

2. **Method members** (call signatures) if they have:
   - The same name
   - Identical parameter types in the same order
   - Identical return types
   - Same generic parameters (if applicable)

3. **Construct signatures** if they have:
   - Identical parameter types
   - Identical return types

**Note:** Parameter names are NOT significant for member identity—only types matter.

### 14.6 Excess Property Checking

**Background:** Excess property checking detects extra properties in object literals that are not expected by the target type. This helps catch typos and misspelled property names.

**TypePython Core v1 Rule:**

TypePython Core v1 does **not** define a distinct TypeScript-style excess-property-checking phase for ordinary class construction or mapping literals. Instead:

- constructor calls are checked using normal call and parameter compatibility rules
- structural compatibility is checked using apparent members and assignability rules
- types that explicitly model arbitrary keyed access do so through `__getitem__` and `__setitem__` members

```python
# Constructor compatibility, not excess-property checking
data class Options:
    strict: bool

opts = Options(strict=True)          # OK
# opts = Options(strict=True, x=1)   # Error by normal call checking
```

**Rationale for Core v1 Rule:**

Python's object model does not have a direct equivalent of TypeScript object-literal freshness. Core v1 therefore keeps the rule simple and ties it to existing call and structural-typing behavior instead of inventing a separate excess-property phase.

**Deferred Beyond v1 Consideration:**

TypeScript's full excess property checking (with fresh object literal types and widening) is NOT supported in Core v1. A future version MAY introduce more granular checks.

### 14.7 Union Normalization

Unions are normalized as follows:

- `A | A` becomes `A`
- `A | Never` becomes `A`
- `A | Any` becomes `Any` (treated as `dynamic`)
- Member order is canonicalized for deterministic comparison

---

## 15. Flow Analysis and Narrowing

### 15.1 Required Narrowing Operations

Core v1 MUST support narrowing for:

| Narrowing                           | Condition Type                                                                          |
| ----------------------------------- | --------------------------------------------------------------------------------------- |
| `x is None`                         | Removes `None` from type                                                                |
| `x is not None`                     | Removes `None` from type                                                                |
| `isinstance(x, T)`                  | Narrows to `T` or union                                                                 |
| `guard(x)` returning `TypeGuard[T]` | Narrows `x` to `T` on the true branch                                                   |
| `guard(x)` returning `TypeIs[T]`    | Narrows `x` to `T` on the true branch and removes `T` on the false branch when possible |
| `assert x`                          | Applies the when-true environment to subsequent flow                                    |
| `match` patterns                    | Exhaustiveness and narrowing                                                            |

Core v1 MUST also support `isinstance(x, (A, B, ...))` narrowing, producing the union `A | B | ...`.

### 15.2 Branch Environments

For a guard expression `G`, the checker computes two environments:

- `EnvTrue(G)`: bindings known when `G` evaluates truthily
- `EnvFalse(G)`: bindings known when `G` evaluates falsily

At minimum, the following rules apply:

- `x is None`: `EnvTrue` narrows `x` to `None`; `EnvFalse` removes `None` from `x`.
- `x is not None`: `EnvTrue` removes `None`; `EnvFalse` narrows to `None`.
- `isinstance(x, T)`: `EnvTrue` intersects `x` with `T`; `EnvFalse` removes `T` from `x` when `x` is a known union or other finite domain from which `T` can be removed deterministically. Otherwise the false branch may leave `x` unchanged.
- `guard(x)` with `TypeGuard[T]`: `EnvTrue` narrows the first guarded argument to `T`; `EnvFalse` leaves it unchanged.
- `guard(x)` with `TypeIs[T]`: `EnvTrue` narrows the first guarded argument to `T`; `EnvFalse` removes `T` when deterministically possible.
- `assert G`: subsequent statements are checked under `EnvTrue(G)`; the false branch is unreachable.

Persistent narrowing in Core v1 is guaranteed only for simple local names.

For attribute and index expressions such as `obj.attr` or `obj[i]`:

- narrowing MAY be used within the same enclosing guard expression for local reasoning
- acceptance of a program MUST NOT depend on longer-lived narrowing of such access paths across statement boundaries
- after the enclosing expression ends, those access paths are treated as having their original pre-guard type unless they are first assigned into a simple local name

### 15.3 Boolean Composition

Guard composition is compositional:

- `not G`: `EnvTrue(not G) = EnvFalse(G)` and `EnvFalse(not G) = EnvTrue(G)`.
- `G1 and G2`: `G2` is checked under `EnvTrue(G1)`. The true environment is `EnvTrue(G2)` under that refined input. The false environment is the join of `EnvFalse(G1)` and `EnvFalse(G2)` under `EnvTrue(G1)`.
- `G1 or G2`: `G2` is checked under `EnvFalse(G1)`. The true environment is the join of `EnvTrue(G1)` and `EnvTrue(G2)` under `EnvFalse(G1)`. The false environment is `EnvFalse(G2)` under `EnvFalse(G1)`.

This rule permits right-hand expressions such as `isinstance(x, str) and x.upper()` to observe the narrowing induced by the left operand.

### 15.4 Truthiness Narrowing

General truthiness narrowing is intentionally limited in Core v1.

`if x:` MUST NOT silently erase `None` or falsy literal possibilities except in cases the checker can prove safely and deterministically.

Guaranteed truthiness-based narrowing in Core v1 is limited to:

- `bool`, `Literal[True]`, and `Literal[False]`
- `T | None` where every non-`None` constituent is definitely truthy
- Cases proven by prior explicit guards such as `x is not None`

### 15.5 Narrowing Lifetime and Assignment Invalidation

A narrowing for binding `x` remains in effect only while the checker can prove `x` still denotes the same runtime value.

At minimum, narrowing for `x` MUST be invalidated by:

- Direct assignment to `x`
- Augmented assignment to `x`
- `del x`
- Rebinding through `global x` or `nonlocal x`

For attribute and index expressions such as `obj.attr` or `obj[i]`, any write through the same root object invalidates prior narrowing of that access path immediately. Implementations MAY invalidate such access-path narrowings even earlier, but they MUST NOT preserve them across statement boundaries in a way that changes program acceptance.

### 15.6 Control-Flow Join

After a conditional branch, the environment is rejoined:

- For a variable present in both branches, the joined type is the union of branch-local types
- `dynamic` joined with any type becomes `dynamic`
- `unknown` joined with `T` becomes `unknown | T`

---

## 21. Appendices

### Appendix A (Informative): Consolidated Grammar

This appendix provides a complete grammar reference for TypePython v1 source syntax. Grammar productions extend Python 3.10 where noted.

#### I.1 Lexical Tokens

```
token              ::= identifier | keyword | literal | operator | delimiter

identifier         ::= any identifier token accepted by the configured target Python lexer

keyword            ::= 'False' | 'None' | 'True' | 'and' | 'as' | 'assert' |
                       'async' | 'await' | 'break' | 'class' | 'continue' |
                       'def' | 'del' | 'elif' | 'else' | 'except' | 'finally' |
                       'for' | 'from' | 'global' | 'if' | 'import' | 'in' |
                       'is' | 'lambda' | 'nonlocal' | 'not' | 'or' | 'pass' |
                       'raise' | 'return' | 'try' | 'while' | 'with' | 'yield'

soft_keyword       ::= 'typealias' | 'interface' | 'data' | 'sealed' |
                       'overload' | 'unsafe'

literal            ::= integer | float | string | bytes | bool | None

integer            ::= decimal_integer | hex_integer | oct_integer | bin_integer
decimal_integer    ::= digit+
hex_integer        ::= '0x' hexdigit+
oct_integer        ::= '0o' octdigit+
bin_integer        ::= '0b' bindigit+

float              ::= point_float | exponent_float
point_float        ::= digit* '.' digit+
exponent_float     ::= digit+ exponent

string             ::= stringliteral | bytesliteral
stringliteral      ::= "'" stringitem* "'" | '"' stringitem* '"'
stringitem         ::= char | escapeseq
char               ::= any character except '\', newline
escapeseq          ::= '\' any character

operator           ::= '+' | '-' | '*' | '**' | '/' | '//' | '%' | '@' |
                       '<<' | '>>' | '&' | '|' | '^' | '~' |
                       '<' | '>' | '<=' | '>=' | '==' | '!=' |
                       '->'

delimiter          ::= '(' | ')' | '[' | ']' | '{' | '}' |
                       ',' | ':' | '.' | ';' | '=' | '+=' | '-=' |
                       '*=' | '/=' | '//=' | '%=' | '**=' | '&=' | '|=' |
                       '^=' | '>>=' | '<<=' | '@='
```

#### I.2 Type Expressions

```
type_expr           ::= union_type_expr

union_type_expr     ::= primary_type_expr ('|' primary_type_expr)*

primary_type_expr   ::= intrinsic_type_expr
                    | qualified_type_name
                    | string_literal
                    | '(' type_expr ')'
                    | tuple_type_expr
                    | subscript_type_expr

intrinsic_type_expr ::= 'dynamic' | 'unknown' | 'Never' | 'None'

qualified_type_name ::= NAME ('.' NAME)*

subscript_type_expr ::= qualified_type_name '[' type_arg_list ']'

type_arg_list       ::= type_expr (',' type_expr)* ','?

tuple_type_expr     ::= '(' type_expr? ',' type_expr (',' type_expr)* ','? ')'

string_literal      ::= STRING
```

#### I.3 Statements and Declarations

```
module             ::= statement*

statement          ::= simple_stmt | compound_stmt

simple_stmt        ::= expr_stmt | assign_stmt | import_stmt | return_stmt |
                       raise_stmt | pass_stmt | flow_stmt

expr_stmt          ::= expression

assign_stmt        ::= target_list '=' expression
                     | target_list ('+=' | '-=' | '*=' | '/=' | etc.) expression

target_list        ::= target (',' target)* ','?

target             ::= identifier
                    | '(' target_list ')'
                    | '[' target_list ']'
                    | attributeref | subscription

import_stmt        ::= 'import' module ['as' name]
                    | 'from' module 'import' import_spec

import_spec        ::= name ['as' alias]
                    | '*'

return_stmt        ::= 'return' expression?

raise_stmt         ::= 'raise' [expression]

pass_stmt          ::= 'pass'

flow_stmt          ::= 'break' | 'continue' | 'return' [expression]

compound_stmt       ::= if_stmt | while_stmt | for_stmt | try_stmt |
                       with_stmt | func_def | class_def

if_stmt            ::= 'if' expression ':' suite
                     ('elif' expression ':' suite)*
                     ['else' ':' suite]

while_stmt         ::= 'while' expression ':' suite

for_stmt           ::= 'for' target_list 'in' expression_list ':' suite
                     ['else' ':' suite]

try_stmt           ::= 'try' ':' suite
                     ('except' [expression ['as' name]] ':' suite)+
                     ['else' ':' suite]
                     ['finally' ':' suite]

with_stmt          ::= 'with' with_item (',' with_item)* ':' suite

with_item          ::= expression ['as' target]

func_def           ::= 'def' NAME parameters ['->' type_expr] ':' suite
                    | 'overload' 'def' NAME parameters ['->' type_expr] ':' simple_suite

parameters         ::= '(' param_list? ')' (':' type_expr)?

param_list         ::= param (',' param)* (',' '*' param)? (',' '**' param)?

param              ::= NAME ('=' expression)?

suite              ::= NEWINDENT statement+ DEDENT

simple_suite       ::= '...'

class_def          ::= 'class' NAME type_params? ('(' base_list? ')')? ':' suite
                    | 'data' 'class' NAME type_params? ('(' base_list? ')')? ':' suite
                    | 'sealed' 'class' NAME type_params? ('(' base_list? ')')? ':' suite

base_list          ::= qualified_type_name (',' qualified_type_name)*

type_params        ::= '[' type_param (',' type_param)* ']'

type_param         ::= NAME (':' type_expr)?
```

#### I.4 TypeScript-Style Extensions

```
// Type alias declaration
typealias_stmt     ::= 'typealias' NAME type_params? '=' type_expr

// Interface declaration (structural protocol)
interface_stmt     ::= 'interface' NAME type_params? ('(' base_list? ')')? ':' suite

// Data class (lowered to @dataclass)
data_class_stmt    ::= 'data' 'class' NAME type_params? ('(' base_list? ')')? ':' suite

// Sealed class (for exhaustiveness checking)
sealed_class_stmt  ::= 'sealed' 'class' NAME type_params? ('(' base_list? ')')? ':' suite

// Overload declaration
overload_decl      ::= 'overload' 'def' NAME type_params? parameters ('->' type_expr)? ':' simple_suite

// Unsafe block
unsafe_stmt        ::= 'unsafe' ':' suite
```

#### I.5 Expressions

```
expression          ::= or_expr

or_expr            ::= and_expr ('or' and_expr)*

and_expr           ::= not_expr ('and' not_expr)*

not_expr           ::= 'not' not_expr | comparison

comparison          ::= sum (('==' | '!=' | '<' | '>' | '<=' | '>=' | 'in' | 'not' 'in' | 'is' | 'is' 'not') sum)*

sum                ::= term (('+' | '-') term)*

term               ::= factor (('*' | '/' | '//' | '%' | '@') factor)*

factor             ::= ('+' | '-' | '~') factor | power

power              ::= await_expr ['**' factor]

await_expr         ::= 'await' unary_expr | unary_expr

unary_expr         ::= 'not' unary_expr | '-' unary_expr | '~' unary_expr | primary

primary            ::= atom (trailer)*

trailer            ::= '(' arglist? ')' | '[' subscriptlist ']' | '.' NAME

atom               ::= identifier | literal | '(' expression ')' | '[' listmaker ']' |
                       '{' dictorsetmaker '}' | '...' | NAME ':' expression

listmaker          ::= expression (',' expression)* ','?

dictorsetmaker     ::= (expression ':' expression (',' expression ':' expression)*) ','?
                    | expression (',' expression)* ','?

arglist            ::= (argument ',')* argument ','? '*' argument? ',' '**' argument?

argument           ::= [test] ('=' test | '**' test | '*' test)?
```

#### I.6 Notes on Grammar

1. **Soft keywords** (`typealias`, `interface`, `data`, `sealed`, `overload`, `unsafe`) are valid identifiers in most contexts but become keywords when appearing at statement start.

2. **Type parameters** use bracket notation `[T, U]` rather than angle brackets to avoid conflict with comparison operators in Python source.

3. **Ellipsis** (`...`) is used for:
   - Function body stubs
   - Interface method stubs
   - Type expression ellipsis (deferred beyond v1)

4. **Stub files (`.pyi`)** use standard Python stub grammar rather than `.tpy`-only authoring syntax, with additional constraints:
   - Function bodies MUST be `...` (not executable code)
   - No runtime statements (assignments, control flow) except type annotations
   - Public names are determined by the rules in Section 11.3

#### I.7 Grammar Ambiguities and Resolutions

| Ambiguity                             | Resolution                                                         |
| ------------------------------------- | ------------------------------------------------------------------ |
| `data class` vs `data` identifier     | `data` followed by `class` keyword triggers data class grammar     |
| `sealed class` vs `sealed` identifier | `sealed` followed by `class` keyword triggers sealed class grammar |
| `type[T]` in subscript vs less-than   | Subscript requires closing `]`, comparison does not                |
| Soft keyword vs identifier            | Context determines interpretation; error if ambiguous              |

### Appendix B: Reserved Features

The following are reserved for deferred beyond v1:

- `extends` config key
- Project references
- Plugins
- Multiple emit profiles
- Custom resolver hooks
- Declaration sourcemap files as user-facing artifact

### Appendix D: Unsafe Boundary Operations

In strict mode, the following operations MUST either appear in an `unsafe:` block or be diagnosed:

- `eval(...)`
- `exec(...)`
- Writes through `globals()` or `locals()`
- Writes through `__dict__`
- `setattr(obj, name, value)` where `name` is not a string literal
- `delattr(obj, name)` where `name` is not a string literal

### Appendix E: CPython Grammar Compatibility Notes

TypePython source MUST be compatible with Python 3.10+ grammar except for the explicit extensions defined in Section 7. The following Python constructs are explicitly supported as-is:

- All expression forms except those whose v1 semantics are explicitly deferred elsewhere in this specification
- All statement forms except those whose v1 semantics are explicitly deferred elsewhere in this specification
- All existing typing constructs (`Union`, `Optional`, `Callable`, etc.)
- `match` statements
- `from __future__ import annotations`

Parser/checker boundary for inherited Python syntax:

- The parser MUST accept inherited Python 3.10+ grammar forms even when their `.tpy` typing semantics are deferred beyond Core v1.
- For such constructs, rejection happens during binding or type checking, not parsing, unless the source is invalid Python syntax outright.
- In `.py` and `.pyi` inputs, the presence of such constructs MUST NOT by itself cause a deferred-feature source error; the deferred restriction applies to user-authored `.tpy` semantics.

The following are NOT supported in Core v1:

- Walrus operator in type expressions (Python 3.8+)
- Pattern matching beyond basic exhaustiveness
