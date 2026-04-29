# TypePython 研究路线评审

日期：2026-04-29

状态：设计评审，非正式承诺

范围：本文评估一组关于 TypePython 未来研究路线的建议，重点放在当前仓库已经实现的能力、建议中的事实偏差、各路线的实际工程成本、研究价值和六个月内的优先级。

## 摘要

这组建议最有价值的判断是：TypePython 如果长期只停留在“把已有类型系统思想高质量 Python 化”的层面，确实会碰到创新天花板。要形成更强差异化，必须在 TypePython 自己的 author-time 语义里建立比标准 Python typing 更强的能力。

但我不认为要简单地“打破 portability / no runtime / no enforcement”这些约束。当前 repo 的核心优势恰恰是双层结构：

1. TypePython 内部可以拥有更强语义、更严格诊断和更丰富的 metadata。
2. 对外发布面仍然是标准 `.py` 和 `.pyi`。
3. 下游 checker 可以继续把输出当作普通 Python。
4. 更强能力由 TypePython checker、LSP、adapter、validator 和 sidecar metadata 消费。

所以更准确的战略不是放弃保守姿态，而是把它改写成：

> 内部语义可以更强，对外表面必须标准；强语义在 TypePython 工具链内闭环，发布产物保持可移植。

在这个框架下，我会把优先级调整为：

| 优先级 | 路线 | 判断 |
| --- | --- | --- |
| P0 | Effect / Capability Rows | 最值得先做，是 `unsafe:`、lifecycle、taint、boundary validator 的共同语义底座。 |
| P1 | Shape 作为内部代数基底 | 当前 repo 已经有 Shape 模型，契合度比建议中评估的更高。 |
| P2 | Restricted Conditional + Mapped Types | 有展示价值，但必须先补类型 IR 和 evaluator，不能只是扩展 `typeddict.rs`。 |
| P3 | Taint as Real Type | 研究价值高，但应建立在 effect/capability 与 framework source/sink 基础上。 |
| P4 | Runtime Boundary Witnesses | 学术价值最高，但当前 validator 还不适合直接承诺 soundness。 |
| 暂缓 | CPython specializing interpreter 协同 | 技术和政治成本都太早，不应进入六个月主线。 |
| 暂缓 | Tensor shape dependent algebra | 当前 metadata-only 路线是合理产品选择，不应在核心路线里强行升级为 dependent typing。 |

如果只能做一件事，我赞同原建议最后的判断：把 `unsafe:` 从特殊语法泛化为 effect/capability 原语。但我会避免一开始就引入重语法或 Koka 风格表面，而是先从内部 effect summary、capability scope 和局部诊断开始。

## 当前 repo 的事实基础

### 1. TypePython 的核心承诺是标准输出面

仓库文档反复强调：TypePython 编译 `.tpy` 到标准 `.py` 和 `.pyi`，没有自定义 runtime，没有 checker plugin，没有 vendor lock-in。

相关锚点：

1. `README.md`：项目定位、lowering 表、portable output by design。
2. `docs/interop.md`：明确要求输出只包含标准 Python typing 构造。
3. `docs/architecture.md`：pipeline 是 syntax、binding、graph、checking、lowering、emit、incremental、CLI/LSP。

这意味着任何“硬核创新”如果直接要求下游 `.pyi` 出现非标准类型构造，都会和项目最核心的产品承诺冲突。

但这不等于 TypePython 不能创新。当前已经存在大量只在 TypePython author-time 生效的能力：

1. `unknown` 的更严格处理。
2. sealed class 的 exhaustiveness 检查。
3. `unsafe:` 的 author-time 边界诊断。
4. TypedDict transforms 的 author-time 展开。
5. lifecycle diagnostics。
6. framework transform 的 synthetic constructor 和 field diagnostics。
7. runtime validator generation 的 opt-in emit 阶段。

所以原建议中“no enforcement 是问题根源”的说法要修正：TypePython 并不是没有 enforcement，而是 enforcement 主要留在 TypePython 工具链内，不转嫁给下游标准 checker。

### 2. 类型 IR 当前很窄

当前类型表达式 IR 主要包括：

1. `Name`
2. `Generic`
3. `Callable`
4. `Union`
5. `Annotated`
6. `Unpack`

相关锚点：

1. `crates/typepython_syntax/src/syntax_parts/type_expr.rs`
2. `crates/typepython_checking/src/type_core.rs`

这对 conditional/mapped types 的影响很大。原建议把路线 3 说成“直接扩展 typeddict.rs”，这个评估偏乐观。真正做通用 conditional/mapped types，需要至少增加：

1. 新的 type-level AST。
2. 新的 `SemanticType` 或 type-function IR。
3. 归约器和终止性约束。
4. 对 `Union`、`Literal`、`TypedDict`、`Shape`、`Callable` 等结构的匹配规则。
5. lowering 阶段的完全求值。
6. stub generation 与 hover/LSP 展示。

也就是说，这不是几个 transform 的扩展，而是一层 type-level evaluator。

### 3. TypedDict transforms 现在是特例机制

当前 lowering 支持的 transform 包括：

1. `Partial`
2. `Required_`
3. `Readonly`
4. `Mutable`
5. `Pick`
6. `Omit`

相关锚点：

1. `crates/typepython_lowering/src/typeddict.rs`
2. `crates/typepython_lowering/src/core.rs`

这个实现非常适合作为产品能力，但不应继续无限扩展成一堆硬编码 helper。它暴露出的真正方向是：TypePython 需要一个可复用的 Shape IR 和有限 type-level evaluator。

### 4. Shape 模型比建议中描述的更接近落地

原建议把 Shape Algebra 列为暂缓，并说契合度低。我不同意。当前 repo 已经有内部 Shape：

1. `ShapeSourceKind`
2. `ShapeFieldSourceKind`
3. `ShapeField`
4. `ShapeExtraItems`
5. `Shape`
6. `partial`
7. `required_fields`
8. `readonly_fields`
9. `mutable_fields`
10. `pick`
11. `omit`

相关锚点：

1. `crates/typepython_checking/src/assignments.rs`
2. `crates/typepython_checking/src/type_system/contextual.rs`
3. `docs/rfcs/shape-model-phase-1.md`

Shape 现在主要作为内部统一表示，不是公开语言构造。这是正确状态。下一步不应该立刻公开 `Shape[...]` 语法，而应该先让更多现有能力真正通过 Shape 模型闭环，包括：

1. TypedDict transforms。
2. dataclass / dataclass_transform。
3. framework transforms。
4. synthetic constructor surface。
5. hover 展示。
6. incremental public summary。

等内部路径稳定后，再考虑公开 `keyof`、shape merge、shape subtraction 等语法。

### 5. Lifecycle diagnostics 不是纯 deferred

原建议把 lifecycle-diagnostics 放在 deferred 列表里，但当前 repo 已经有实现：

1. ignored lifecycle result diagnostics。
2. unclosed lifecycle resource diagnostics。
3. `@must_use`、`@must_await`、`@must_close`、`@must_consume` 风格的局部诊断。

相关锚点：

1. `crates/typepython_checking/src/semantic.rs`

这说明 TypePython 已经有 effect-like 诊断的雏形。它不是完整 effect system，但它证明了“author-time enforcement + standard output”这条路线是可行的。

### 6. `unsafe:` 已经是 effect/capability 的雏形

当前 `unsafe:` 在 strict/warn_unsafe 模式下会检查 unsafe operation site，不在 unsafe block 内会产生诊断。lowering 时它会被擦除或重写成普通 Python 结构。

相关锚点：

1. `crates/typepython_checking/src/semantic.rs`
2. `crates/typepython_lowering/src/core.rs`
3. `crates/typepython_lsp` 中已有 unsafe wrapper quickfix。

这非常适合作为 effect/capability system 的入口。关键不是“再发明一个 effect 语法”，而是把 `unsafe:` 的语义从单一布尔标记升级为：

1. 作用域 capability。
2. 调用点 effect 检查。
3. 函数 effect summary。
4. adapter 提供的外部 capability metadata。
5. LSP 可解释的诊断和 quickfix。

### 7. Boundary validators 目前是 emit-time 能力，不是 soundness 证明

runtime validators 已经存在，但它们现在主要是 opt-in emit 阶段能力：

1. 内建 validator。
2. Pydantic / msgspec / cattrs delegate。
3. 多种 boundary kind。
4. 配置驱动启用。

相关锚点：

1. `docs/rfcs/boundary-validator-generation.md`
2. `crates/typepython_emit/src/runtime.rs`

这条路有研究潜力，但当前不能直接推出“boundary witness 证明”。原因是：

1. validator 是否 faithful 需要严格定义。
2. 泛型容器、嵌套结构、Protocol、Callable、TypedDict extra items 都需要规则。
3. validator 成功后的事实需要进入 checker flow graph。
4. witness 失效条件需要和 assignment、mutation、aliasing、attribute write 绑定。
5. delegate 到第三方库时，TypePython 无法天然证明其 validator 与类型语义一致。

所以 runtime witnesses 应该作为研究里程碑，而不是第一批主线产品。

## 对原建议核心判断的修正

### 修正 1：问题不是“metadata-only”，而是 metadata 缺少统一语义底座

metadata-only 本身不是问题。TypePython 的优势就在于能产生比标准 Python 更丰富的 author-time metadata，同时输出标准代码。

真正的问题是：多个 metadata 方向目前相互独立：

1. `unsafe` 是一套机制。
2. lifecycle 是一套机制。
3. taint RFC 是一套机制。
4. boundary validators 是一套机制。
5. framework transforms 是一套机制。
6. result/effect RFC 是另一套表达。

这些都可以被统一为 capability/effect/fact system。统一之后，metadata 不再是“注释”，而是 TypePython 内部语义的一部分。

### 修正 2：创新不一定要打破 portability

原建议说硬核创新需要打破至少一条约束。这个判断对 runtime witnesses 和 CPython specialization 成立，但对 effect rows、shape algebra、conditional/mapped types 不一定成立。

TypePython 可以在自己的 `.tpy` 源语言中做强语义，然后在 lowering 阶段完全归约或擦除：

1. effect rows 可以 lower 成 `Annotated` 或 sidecar metadata。
2. mapped types 可以在 build 时完全展开。
3. shape operations 可以 lower 成 TypedDict、Protocol、dataclass constructor stubs。
4. taint 类型可以在 TypePython checker 内非赋值兼容，对外 lower 成 `Annotated[T, ...]` 或 plain `T`。

这不是“没有创新”，而是 compiler front-end 的典型优势：前端语言比目标语言更强。

### 修正 3：Conditional/mapped types 的 ROI 高，但不是最好的第一步

路线 3 很适合展示差异化，因为用户能马上看到：

1. `Pick` / `Omit` 不再是硬编码集合。
2. 可以表达 schema 派生。
3. `.pyi` 仍然完全标准。
4. ty / pyright / mypy 不需要理解 TypePython 扩展。

但这条路线依赖 Type IR 和 evaluator。相比之下，effect/capability 可以从现有 `unsafe:` 和 lifecycle 直接演进，路径更短，战略联动更多。

所以我会把 effect/capability 放在 P0，把 conditional/mapped 放在 P2。

### 修正 4：Shape Algebra 不该暂缓太久

原建议把 Shape Algebra 放在暂缓，我认为这是错过了当前 repo 最有价值的内部资产。

Shape 的优势不是“公开一个漂亮语法”，而是统一以下问题：

1. TypedDict field sets。
2. dataclass fields。
3. Pydantic-like model fields。
4. SQLAlchemy / Django ORM field providers。
5. FastAPI request/response model surfaces。
6. framework-generated constructor signatures。
7. field alias、readonly、required、default、extra items。

如果没有 Shape，mapped types 会退化成 TypedDict 专用工具。如果有 Shape，mapped types 才能成为跨 framework 的 schema calculus。

## 推荐路线 1：Effect / Capability Rows

### 判断

这是最值得优先投入的路线。它不是单独 feature，而是可以统一多个现有和 deferred 方向的语义枢纽：

1. `unsafe:`。
2. lifecycle diagnostics。
3. result/effect RFC。
4. taint source/sink/sanitizer。
5. boundary validators。
6. framework adapter capabilities。
7. async、IO、network、filesystem、random、time、process 等 side effects。

如果做得好，TypePython 可以形成一个清晰定位：

> Python 的 portable type front-end，同时提供标准 typing 尚未覆盖的 capability/effect analysis。

### 不建议一开始做的事

不建议第一版就引入这种表面：

```python
def fetch(url: str) -> str / [io, network]: ...
```

原因：

1. 语法冲击大。
2. parser、formatter、LSP、lowering 全要改。
3. 和 Python 未来 PEP 对齐风险高。
4. 对早期用户来说可解释性不一定比 decorator 强。

第一版应优先使用 TypePython 现有风格：

1. decorator。
2. `Annotated` metadata。
3. `unsafe:` scope。
4. config opt-in。
5. framework adapter metadata。

### 建议的 MVP

第一版可以只做五件事。

#### 1. 内部数据模型

新增内部概念：

1. `EffectKind`
2. `EffectRow`
3. `EffectSummary`
4. `CapabilityScope`
5. `EffectSource`

初始 effect vocabulary 不要太多：

1. `unsafe`
2. `io.fs`
3. `io.net`
4. `io.proc`
5. `time`
6. `random`
7. `runtime.validation`
8. `taint.sanitize`

第一版重点不是完美 taxonomy，而是让模型支持组合、包含和诊断。

#### 2. Authoring surface

先支持 decorator 形式：

```python
@effect("io.net")
def fetch(url: str) -> str: ...

@effect_pure
def parse(text: str) -> dict[str, object]: ...
```

或者更 TypePython 风格地作为 framework metadata：

```python
@tpy.effect("io.net")
def fetch(url: str) -> str: ...
```

`unsafe:` 保持存在，但语义改为 capability scope：

```python
def load_plugin(path: str) -> object:
    unsafe:
        return eval(path)
```

后续可以支持：

```python
effect io.net:
    fetch(url)
```

但这不应该是 MVP。

#### 3. 检查规则

第一版规则保持非常保守：

1. 只有显式 `@effect_pure` 或 config 指定 pure default 时，才检查 pure 函数。
2. 函数调用者的 allowed effects 必须覆盖 callee effects。
3. `unsafe:` block 只授予局部 capability，不改变函数 public effect summary，除非函数显式声明。
4. 未标注函数默认 `unknown effect`，在 strict 模式下可 warning。
5. framework adapter 可以为特定 decorator、base class、method 注入 effect summary。

这样可以避免一开始就做复杂 inference。

#### 4. 输出策略

`.py` 和 `.pyi` 仍然标准。

可选输出：

1. `.pyi` 中保留 `Annotated[..., EffectRow(...)]`，但下游 checker 看到的主类型仍然兼容。
2. 生成 sidecar metadata，例如 `.typepython/effects.json`。
3. LSP 和 verify 命令消费 sidecar metadata。

如果担心 `Annotated` metadata 影响兼容性，第一版可以只走 sidecar。

#### 5. LSP 和 diagnostics

必须做 LSP 支持，否则 effect system 很难被用户理解：

1. hover 展示函数 effect summary。
2. diagnostic note 解释 effect 来源。
3. quickfix 提供“给当前函数添加 effect 声明”。
4. quickfix 提供“包裹到 `unsafe:` scope”。
5. code action 可以生成 suppress 或 capability annotation。

### 为什么这是 P0

Effect/capability 是唯一能同时支撑以下路线的设计：

1. Taint 类型需要 source/sink/sanitizer effect。
2. Runtime witness 需要 validation effect 和 trust boundary。
3. Lifecycle diagnostics 可以升级成 resource effect。
4. Framework adapters 可以声明 capability，而不是写一堆特例。
5. `unsafe:` 可以从单点 feature 升级为统一能力模型。

## 推荐路线 2：Shape 作为内部代数基底

### 判断

Shape 不应该先公开成用户语法。更重要的是把它变成 checker 和 lowering 的共享内部基底。

当前 repo 已经有 Shape，但仍有一些路径是 ad hoc 的。下一阶段目标应该是：

> 所有 field-bearing 类型都先进入 Shape IR，然后从 Shape 派生 diagnostics、constructor surface、stub output 和 transform result。

### 第一阶段目标

第一阶段不改变用户语法，只重构内部语义路线。

具体目标：

1. TypedDict collector 产出 Shape。
2. dataclass/dataclass_transform collector 产出 Shape。
3. framework transform provider 产出 Shape。
4. `Partial`、`Pick`、`Omit`、`Readonly`、`Mutable` 作用于 Shape。
5. lowering 从 Shape materialize 标准 TypedDict 或 synthetic stubs。
6. LSP hover 展示 projected Shape。
7. incremental summary 记录 Shape fingerprint。

### 第二阶段目标

等内部路径稳定后，再公开有限 Shape operations：

1. `keyof SomeShape`
2. `Pick[SomeShape, Literal["id", "name"]]`
3. `Omit[SomeShape, Literal["password"]]`
4. `MapValues[SomeShape, Optional]`
5. `Merge[ShapeA, ShapeB]`

不建议第一版支持任意表达式级 shape literal，例如：

```python
typealias UserShape = Shape[
    "id": int,
    "name": str,
]
```

这个语法设计成本高，也容易和 Python parser、formatter、stub compatibility 发生摩擦。

### 为什么 Shape 是 P1

原因很直接：没有 Shape，conditional/mapped types 只能覆盖 TypedDict。那会变成 TypeScript feature 的局部复制。

有 Shape 后，TypePython 可以讲更强的故事：

> Python 生态里的 dict、TypedDict、dataclass、Pydantic、ORM model 和 web framework schema 都可以通过同一个 Shape IR 做类型级投影。

这比单独做 `Pick` / `Omit` 更有原创性。

## 推荐路线 3：Restricted Conditional + Mapped Types

### 判断

这条路线值得做，但应该建立在 Shape IR 之后。它的目标不是复刻 TypeScript，而是提供一个可终止、可 lowering、checker-neutral 的 type-level evaluator。

### 不建议复刻 TypeScript 表面

不建议直接追求：

```typescript
T extends U ? A : B
{ [K in keyof T as `on_${K}`]: ... }
```

原因：

1. Python 语法没有天然容器。
2. `.tpy` parser 和 formatter 成本会很高。
3. 用户可能把它理解为完整 TypeScript type system，从而期待递归、template literal、distributive conditional 等高级特性。
4. 一旦暴露过强语法，后续收缩会很难。

### 建议的设计方向

第一版应更像“有限 type functions”：

```python
typealias NonNull[T] = TypeIf[IsSubtype[T, None], Never, T]
typealias PublicUser = Omit[User, Literal["password"]]
typealias UserKeys = KeyOf[User]
```

也可以在 TypePython 源语言里设计更好看的语法，但语义上仍然是有限 evaluator。

### Evaluator 必须具备的约束

为了避免走向 Turing-complete type-level computation，应明确：

1. 只允许有限展开。
2. 递归 typealias 必须有 structural decrease 或被禁止。
3. evaluator 有 step budget。
4. 所有 output 必须能 materialize 成标准 Python typing。
5. conditional 只对可判定的关系求值。
6. 不能求值时产生 TypePython diagnostic，而不是输出非标准 `.pyi`。

### 第一版支持集合

第一版可以支持：

1. `IsSubtype[T, U]`
2. `TypeIf[Cond, A, B]`
3. `KeyOf[ShapeLike]`
4. `Pick[ShapeLike, Keys]`
5. `Omit[ShapeLike, Keys]`
6. `RequiredKeys[ShapeLike]`
7. `OptionalKeys[ShapeLike]`
8. `MapValues[ShapeLike, F]`，其中 `F` 初始只支持内建 wrapper，例如 `Optional`、`Readonly`。

不要第一版就支持：

1. template literal type。
2. arbitrary key remapping。
3. recursive deep transforms。
4. distributive conditional 的完整 TypeScript 语义。
5. type-level string manipulation。

### 为什么它是 P2

它的展示价值很高，但它依赖：

1. 更强 Type IR。
2. Shape substrate。
3. evaluator。
4. LSP 展示。
5. lowering 完全归约。

因此它不应该抢在 effect/capability 和 Shape 前面。

## 推荐路线 4：Taint as Real Type

### 判断

Taint as real type 是很有价值的研究方向，但不能孤立实现。它应该被建模为：

1. type qualifier。
2. effect/capability fact。
3. framework boundary fact。
4. sanitizer transform。
5. sink constraint。

如果只做 `Tainted[str]` 不能赋给 `str`，很容易产生两个问题：

1. 用户以为得到了完整信息流安全，但实际只覆盖局部。
2. framework source/sink 不足时，诊断会非常不稳定。

### 建议的 MVP

第一版只做四件事：

1. `Tainted[T, Context]` 在 TypePython checker 内不是 `T` 的 subtype。
2. 显式 sanitizer 可以把 `Tainted[T, Context]` 转为 `T` 或另一种 context。
3. 显式 sink 可以要求未污染类型或已 sanitizer 类型。
4. framework adapter 可以标记 source 和 sink。

例子：

```python
@source("http.request", taint="html")
def body() -> Tainted[str, "html"]: ...

@sanitizer(from_taint="html", to="str")
def escape_html(x: Tainted[str, "html"]) -> str: ...

@sink("html.response")
def render(x: str) -> Response: ...
```

### 和 effect/capability 的关系

Taint 不是单纯类型问题。Sanitizer 通常有 context，sink 也有 context。某些 sanitizer 可能需要 runtime validation 或 escaping policy。Source 来自 framework adapter 或 trust boundary。

所以它应该共用 effect/capability 的基础设施：

1. source 是 taint-introducing effect。
2. sanitizer 是 taint-transforming effect。
3. sink 是 capability requirement。
4. boundary validator 可以产生 taint-cleared witness。

### 推荐时间点

Taint 不应是 P0。更合理的是在 effect/capability MVP 和 Shape substrate 后做一个小型 vertical slice，例如：

1. FastAPI request body 标为 tainted。
2. Jinja2/HTML response sink 检查。
3. `escape_html` sanitizer。
4. LSP hover 展示 taint context。

## 推荐路线 5：Runtime Boundary Witnesses

### 判断

这是最有学术价值的方向，但也是最容易过早承诺的方向。

当前 repo 有 runtime validator generation，这很好。但它目前更像 emit-time safety aid，不是类型系统里的 witness calculus。

### 为什么不能直接做完整 soundness

要让 validator 成功后产生静态 witness，需要解决：

1. validator 对类型语义的 faithful 性。
2. 泛型容器元素是否递归验证。
3. TypedDict required/optional/extra item 是否完整验证。
4. Protocol、Callable、NewType、Annotated 的 runtime 语义。
5. mutation 后 witness 失效。
6. aliasing 后 witness 是否仍然成立。
7. delegation 到 Pydantic/msgspec/cattrs 时如何建模 trust。
8. witness 在函数边界、闭包、属性、容器中的生命周期。

其中任一项处理不清，都会让 “verified gradual typing” 的论文故事失去可信度。

### 建议的阶段化路线

#### 阶段 1：Unknown narrowing by validator

先做非常小的版本：

```python
def handle(x: unknown) -> User:
    if validate[User](x):
        reveal_type(x)  # User
        return x
```

这个阶段只要求：

1. validator 是 TypePython 认可的内建或 adapter-declared faithful validator。
2. flow narrowing 在 `if` true branch 内有效。
3. mutation 或 assignment 后失效。

这类似 `TypeIs`，但 validator 可以由 TypePython 生成。

#### 阶段 2：Boundary witness

把 witness 绑定到 trust boundary：

1. HTTP request。
2. CLI parameter。
3. config file。
4. message payload。
5. plugin entrypoint。

validator 通过后，在 boundary 内形成局部 type fact。

#### 阶段 3：Formal subset

只对一个小语言子集写 theorem：

1. immutable values。
2. first-order functions。
3. simple containers。
4. TypedDict closed shapes。
5. no arbitrary mutation aliasing。

这才适合写 OOPSLA/POPL/ICFP 风格材料。

### 推荐时间点

这条应作为 P4 研究路线，不应在六个月主线前半段投入过深。

## 推荐路线 6：CPython specializing interpreter 协同

### 判断

暂缓。

这条路线一旦成立，影响力很大，但它要求：

1. CPython internals。
2. bytecode metadata 设计。
3. PEP 写作。
4. core dev 讨论。
5. benchmark suite。
6. 长期兼容承诺。

当前 TypePython 仍处于语言、checker、lowering、LSP、adapter 和 publishing pipeline 快速演进阶段。过早进入 CPython 协同，会把项目从产品和类型系统问题拖进治理与解释器实现问题。

建议 12 到 18 个月后再重新评估。

## 对 Tensor Shape Types 的判断

我基本同意原建议：不要在当前阶段追求 tensor dependent types。

原因：

1. 真正有价值的 tensor shape typing 很快会需要 symbolic algebra。
2. symbolic algebra 又会导向 dependent typing 或 SMT。
3. Python ML 生态高度动态，framework 版本差异巨大。
4. 单人维护项目很难同时承担 checker、lowering、LSP、adapter、solver 和 tensor algebra。

当前 metadata-only 的 tensor shape RFC 是合理产品姿态，但不应作为创新主线。

如果未来 effect/Shape/type-evaluator 都成熟，可以再把 tensor shape 作为特定 domain adapter 加回来，而不是现在强行把它做成核心类型系统。

## 六个月路线图建议

### 第 1 阶段：Effect/capability facts

目标：

1. 定义内部 `EffectKind` / `EffectRow` / `EffectSummary`。
2. 把 `unsafe:` 从布尔标记升级为 capability scope。
3. 把 lifecycle diagnostics 映射到 effect/resource facts。
4. 支持显式 `@effect` / `@effect_pure`。
5. LSP hover 能展示 effect summary。
6. strict 模式下能报告 pure 函数调用 effectful 函数。

输出：

1. RFC。
2. 实现。
3. 10 到 20 个 focused tests。
4. 一篇面向用户的文档。

### 第 2 阶段：Shape substrate hardening

目标：

1. 收敛所有 field-bearing source 到 Shape。
2. 降低 TypedDict transform 的 ad hoc 程度。
3. framework transform 使用 Shape 产出 synthetic constructor。
4. incremental summary 记录 Shape fingerprint。
5. LSP hover 展示 shape projection。

输出：

1. Shape internal design note。
2. Transform golden fixtures。
3. Framework adapter fixture 扩展。
4. Downstream checker matrix 更新。

### 第 3 阶段：Restricted type-level evaluator

目标：

1. 增加 type-function IR。
2. 支持 `TypeIf`、`IsSubtype`、`KeyOf`、`Pick`、`Omit`。
3. 所有 evaluator output 在 lowering 前完全归约。
4. 未能归约时报告 TypePython diagnostic。
5. `.pyi` 不出现 TypePython-only type function。

输出：

1. evaluator tests。
2. conformance examples。
3. stub snapshot tests。
4. LSP hover 展示“展开前”和“展开后”。

### 第 4 阶段：Taint 或 witness 的小型 vertical slice

二选一，不建议两条同时做。

如果选择 taint：

1. FastAPI-like request source。
2. HTML response sink。
3. sanitizer declaration。
4. intra-procedural taint checking。

如果选择 witness：

1. TypePython-generated validator。
2. `unknown` narrowing。
3. assignment/mutation invalidation。
4. trust-boundary note。

输出：

1. 一组 demo。
2. 一篇长文。
3. 明确列出 non-goals。

## 风险清单

### 1. 语法债

TypePython 一旦引入复杂新语法，formatter、parser、LSP、lowering、documentation 都会跟着变重。

缓解：

1. 第一版尽量用 decorator、typealias helper、`Annotated`、sidecar metadata。
2. 新语法只在语义稳定后引入。
3. 所有新语法必须有明确 lowering 策略。

### 2. 下游兼容面污染

如果 `.pyi` 输出非标准构造，项目核心承诺会被破坏。

缓解：

1. TypePython-only construct 必须在 emit 前完全擦除或归约。
2. verify 阶段加入 portability gate。
3. downstream checker matrix 持续覆盖 mypy、pyright、ty 等目标。

### 3. False security

Taint 和 runtime witness 都容易制造虚假的安全感。

缓解：

1. 文档明确 coverage。
2. diagnostic wording 保守。
3. 不把局部检查宣传成全程序 soundness。
4. 第三方 validator 必须标注 trust level。

### 4. Adapter sprawl

不断写 framework adapter 会消耗大量维护精力。

缓解：

1. adapter 只提供 facts。
2. facts 必须落到通用 capability/Shape/taint/boundary vocabulary。
3. 不为每个 framework 写专用 checker 逻辑。

### 5. Solver 复杂度失控

Effect rows、Shape algebra、conditional types 都可能推高 solver 复杂度。

缓解：

1. 保持 evaluator 有 step budget。
2. 不做 SMT。
3. 不做任意递归 type-level computation。
4. 所有复杂能力先 opt-in。
5. 性能 benchmark 必须纳入合并门槛。

## 成功指标

六个月内比较实际的成功指标不是“发表论文”，而是：

1. 用户可以在 TypePython 中声明 pure/effectful 函数，并得到稳定诊断。
2. `unsafe:` 不再是孤立 feature，而是 capability scope 的一个实例。
3. Shape 成为 field-bearing 类型的共享内部表示。
4. 至少一个 framework adapter 通过 Shape 和 effect facts 提供更强诊断。
5. `Pick` / `Omit` 之类 transform 不再只是硬编码 TypedDict 特例。
6. 所有新能力输出的 `.py` / `.pyi` 仍能通过 downstream checker matrix。
7. LSP 能解释新语义，而不是只在 CLI 中报错。
8. 文档能清楚说明 TypePython 内部强语义和标准输出面的边界。

## 推荐的对外叙事

我不建议把 TypePython 定位成“Python 版 TypeScript”或“更强 mypy”。这两个方向都会让项目落入已有工具的比较框架。

更好的叙事是：

> TypePython is a portable typed front-end for Python that adds stronger author-time semantics while emitting standard Python.

中文可以说：

> TypePython 是一个可移植的 Python 类型前端：它在作者侧提供更强的类型、effect、schema 和 boundary 语义，同时发布标准 Python 产物。

这个叙事能同时容纳：

1. portable output。
2. no runtime by default。
3. stronger author-time checking。
4. framework-aware schema。
5. capability/effect analysis。
6. future boundary witnesses。

## 最终建议

我会按下面顺序推进：

1. 先把 `unsafe:` 泛化为 effect/capability system。
2. 再把 Shape 从内部辅助模型升级为所有 field-bearing 类型的统一 substrate。
3. 然后做 restricted conditional/mapped evaluator，让 shape projection 成为通用 type-level 能力。
4. 最后在 effect + Shape 的基础上选择 taint 或 runtime witness 做研究型 vertical slice。

这条路线的核心优点是：它不需要 TypePython 放弃当前最强的产品承诺，同时能逐步建立真正原创的语义层。

真正要避免的是两种极端：

1. 只做标准 typing 语法糖，最后变成已有工具的前端包装。
2. 过早追求 dependent types、SMT、CPython PEP 或完整 soundness，导致项目工程面失控。

TypePython 当前最有希望的创新空间在中间地带：强 author-time semantics，标准 consumer-time Python。
