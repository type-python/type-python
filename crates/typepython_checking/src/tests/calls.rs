use super::*;

#[test]
fn check_substitutes_source_authored_paramspec_in_return_type() {
    let result = check_temp_typepython_source(concat!(
        "from typing import Callable\n\n",
        "def identity[**P, R](cb: Callable[P, R]) -> Callable[P, R]:\n",
        "    return cb\n\n",
        "def greet(name: str) -> str:\n",
        "    return name\n\n",
        "handler: Callable[[str], str] = identity(greet)\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_keyword_and_default_arguments_in_direct_calls() {
    let result = check_temp_typepython_source(
        "def field(default=None, init=True, kw_only=False):\n    return default\n\nfield(default=\"Ada\", init=False)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_positional_only_parameter_passed_as_keyword() {
    let result =
        check_temp_typepython_source("def takes(x: int, /):\n    return x\n\ntakes(x=1)\n");

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("positional-only parameter `x`"));
}

#[test]
fn check_accepts_keyword_constructor_calls_for_explicit_init() {
    let result = check_temp_typepython_source(
        "class User:\n    def __init__(self, age: int, name: str = \"Ada\"):\n        self.age = age\n        self.name = name\n\nUser(age=1)\nUser(age=1, name=\"Grace\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_positional_only_constructor_parameter_passed_as_keyword() {
    let result = check_temp_typepython_source(
        "class User:\n    def __init__(self, age: int, /):\n        self.age = age\n\nUser(age=1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("positional-only parameter `age`"));
}

#[test]
fn check_reports_incomplete_conditional_return_coverage() {
    let result = check_temp_typepython_source_with_options(
        "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n",
        ParseOptions { enable_conditional_returns: true, ..ParseOptions::default() },
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4018"));
    assert!(rendered.contains("missing: None"));
}

#[test]
fn check_accepts_complete_conditional_return_coverage() {
    let result = check_temp_typepython_source_with_options(
        "def decode(x: str | bytes | None) -> match x:\n    case str: str\n    case bytes: str\n    case None: None\n",
        ParseOptions { enable_conditional_returns: true, ..ParseOptions::default() },
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4018"));
    assert!(!result.diagnostics.has_errors());
}

#[test]
fn check_accepts_dataclass_transform_decorator_constructor_call() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n    age: int\n\nuser: User = User(\"Ada\", 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_dataclass_transform_base_class_constructor_call() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\nclass ModelBase:\n    pass\n\nclass User(ModelBase):\n    name: str\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_dataclass_transform_metaclass_constructor_call() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\nclass ModelMeta:\n    pass\n\nclass User(metaclass=ModelMeta):\n    name: str\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_dataclass_transform_constructor_arity_mismatch() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n    age: int\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("missing required synthesized dataclass-transform field(s): age"));
}

#[test]
fn check_reports_dataclass_transform_constructor_type_mismatch() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("synthesized dataclass-transform field `age` expects `int`"));
}

#[test]
fn check_reports_dataclass_transform_constructor_keyword_type_mismatch() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(age=\"oops\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("synthesized keyword `age`"));
    assert!(rendered.contains("expects `int`"));
}

#[test]
fn check_reports_dataclass_transform_constructor_duplicate_binding() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    age: int\n\nuser: User = User(1, age=2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("binds synthesized field `age` both positionally and by keyword"));
}

#[test]
fn check_accepts_dataclass_transform_default_and_classvar_fields() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    role: ClassVar[str]\n    name: str\n    age: int = 1\n\nuser: User = User(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_plain_dataclass_constructor_arguments() {
    let result = check_temp_typepython_source(
        "@dataclass\nclass User:\n    name: str\n    age: int = 1\n\nUser(\"Ada\")\nUser(\"Ada\", 2)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_plain_frozen_dataclass_field_assignment_after_init() {
    let result = check_temp_typepython_source(
        "@dataclass(frozen=True)\nclass User:\n    name: str\n\nuser = User(\"Ada\")\nuser.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("frozen dataclass field `name`"));
}

#[test]
fn check_reports_plain_kw_only_dataclass_positional_call() {
    let result = check_temp_typepython_source(
        "@dataclass(kw_only=True)\nclass User:\n    name: str\n\nUser(\"Ada\")\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("expects at most 0 positional argument(s) but received 1"));
}

#[test]
fn check_accepts_dataclass_transform_inherited_fields() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass Base:\n    name: str\n\nclass User(Base):\n    age: int\n\nuser: User = User(\"Ada\", 1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_inherited_dataclass_transform_kw_only_defaults() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\ndef field(*, default=None, kw_only=False, init=True):\n    return default\n\n@dataclass_transform(field_specifiers=(field,), kw_only_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass Base:\n    age: int\n\nclass User(Base):\n    name: str\n\nUser(name=\"Ada\", age=1)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_excludes_descriptor_defaults_from_dataclass_transform_fields() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\nclass Descriptor:\n    def __get__(self, instance, owner):\n        return 0\n\n@dataclass_transform()\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: int = Descriptor()\n\nuser: User = User()\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_frozen_dataclass_transform_assignment_in_init() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\n    def __init__(self, name: str):\n        self.name = name\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("frozen dataclass-transform field"), "{rendered}");
}

#[test]
fn check_reports_frozen_dataclass_transform_field_assignment_after_init() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\nuser: User = User(\"Ada\")\nuser.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("frozen dataclass-transform field `name`"));
}

#[test]
fn check_reports_frozen_dataclass_transform_field_assignment_after_init_with_explicit_init() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    name: str\n\n    def __init__(self, name: str):\n        self.name = name\n\nuser: User = User(\"Ada\")\nuser.name = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("frozen dataclass-transform field `name`"));
}

#[test]
fn check_reports_frozen_dataclass_transform_augmented_assignment_after_init() {
    let result = check_temp_typepython_source(
        "def dataclass_transform(*args, **kwargs):\n    def wrap(obj):\n        return obj\n    return wrap\n\n@dataclass_transform(frozen_default=True)\ndef model(cls):\n    return cls\n\n@model\nclass User:\n    count: int\n\nuser: User = User(1)\nuser.count += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("augmented assignment after initialization"));
}

#[test]
fn check_reports_readonly_typed_dict_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be assigned"));
}

#[test]
fn check_reports_qualified_readonly_typed_dict_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nimport typing_extensions\n\nclass User(TypedDict):\n    name: typing_extensions.ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"), "{rendered}");
    assert!(rendered.contains("cannot be assigned"), "{rendered}");
}

#[test]
fn check_accepts_writable_typed_dict_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = \"Grace\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_accepts_writable_typed_dict_extra_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict, extra_items=int):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"age\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_writable_typed_dict_item_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"name\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("assigns `int` where `name` expects `str`"));
}

#[test]
fn check_reports_readonly_typed_dict_extra_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict, extra_items=ReadOnly[int]):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"age\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be assigned"));
}

#[test]
fn check_reports_qualified_readonly_typed_dict_extra_item_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nimport typing_extensions\n\nclass User(TypedDict, extra_items=typing_extensions.ReadOnly[int]):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"age\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"), "{rendered}");
    assert!(rendered.contains("cannot be assigned"), "{rendered}");
}

#[test]
fn check_accepts_qualified_notrequired_typed_dict_field() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nimport typing_extensions\n\nclass User(TypedDict):\n    name: str\n    age: typing_extensions.NotRequired[int]\n\nuser: User = {\"name\": \"Ada\"}\n",
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_treats_qualified_never_extra_items_as_closed_typed_dict() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nimport typing_extensions\n\nclass User(TypedDict, extra_items=typing_extensions.Never):\n    name: str\n\nuser: User = {\"name\": \"Ada\", \"age\": 1}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("unknown key `age`"), "{rendered}");
}

#[test]
fn check_accepts_contextual_writable_typed_dict_item_assignment_lambda() {
    let result = check_temp_typepython_source(
        "from typing import Callable, TypedDict\n\nclass User(TypedDict):\n    formatter: Callable[[int], str]\n\ndef mutate(user: User) -> None:\n    user[\"formatter\"] = lambda x: str(x)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_contextual_writable_typed_dict_item_assignment_nested_typed_dict_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass Child(TypedDict):\n    name: str\n\nclass User(TypedDict):\n    child: Child\n\ndef mutate(user: User) -> None:\n    user[\"child\"] = {}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_writable_typed_dict_item_augmented_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\ndef mutate(user: User) -> None:\n    user[\"name\"] += \"!\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_writable_typed_dict_item_augmented_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    age: int\n\ndef mutate(user: User) -> None:\n    user[\"age\"] += \"!\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("produces `str` where `age` expects `int`"));
}

#[test]
fn check_reports_readonly_typed_dict_item_augmented_assignment() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    user[\"name\"] += \"!\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("augmented assignment"));
}

#[test]
fn check_reports_readonly_typed_dict_item_delete() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\nfrom typing_extensions import ReadOnly\n\nclass User(TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    del user[\"name\"]\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be deleted"));
}

#[test]
fn check_reports_readonly_typed_dict_item_delete_for_qualified_base() {
    let result = check_temp_typepython_source(
        "import typing\nfrom typing_extensions import ReadOnly\n\nclass User(typing.TypedDict):\n    name: ReadOnly[str]\n\ndef mutate(user: User) -> None:\n    del user[\"name\"]\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4016"));
    assert!(rendered.contains("cannot be deleted"));
}

#[test]
fn check_accepts_nominal_setitem_subscript_assignment() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_nominal_setitem_subscript_value_mismatch() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] = \"bad\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes value `str` where `__setitem__` expects `int`"));
}

#[test]
fn check_reports_nominal_setitem_subscript_key_mismatch() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[1] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("passes key `int` where `__setitem__` expects `str`"));
}

#[test]
fn check_accepts_contextual_nominal_setitem_subscript_assignment_lambda() {
    let result = check_temp_typepython_source(
        "from typing import Callable\n\nclass Cache:\n    def __setitem__(self, key: str, value: Callable[[int], str]) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"fmt\"] = lambda x: str(x)\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_contextual_nominal_setitem_subscript_assignment_typed_dict_missing_key() {
    let result = check_temp_typepython_source(
        "from typing import TypedDict\n\nclass User(TypedDict):\n    name: str\n\nclass Cache:\n    def __setitem__(self, key: str, value: User) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"user\"] = {}\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"));
    assert!(rendered.contains("missing required key `name`"));
}

#[test]
fn check_accepts_nominal_setitem_subscript_augmented_assignment() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __getitem__(self, key: str) -> int:\n        return 0\n\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_nominal_setitem_subscript_augmented_assignment_type_mismatch() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __getitem__(self, key: str) -> int:\n        return 0\n\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] += \"bad\"\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("produces `str` where `__setitem__` expects `int`"));
}

#[test]
fn check_reports_unreadable_nominal_setitem_subscript_augmented_assignment() {
    let result = check_temp_typepython_source(
        "class Cache:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] += 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("not readable via `__getitem__`"));
}

#[test]
fn check_accepts_inherited_setitem_subscript_assignment() {
    let result = check_temp_typepython_source(
        "class Base:\n    def __setitem__(self, key: str, value: int) -> None:\n        return None\n\nclass Cache(Base):\n    pass\n\ndef mutate(cache: Cache) -> None:\n    cache[\"x\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(!result.diagnostics.has_errors(), "{rendered}");
}

#[test]
fn check_reports_readonly_nominal_subscript_assignment_without_setitem() {
    let result = check_temp_typepython_source(
        "class View:\n    def __getitem__(self, key: str) -> int:\n        return 1\n\ndef mutate(view: View) -> None:\n    view[\"x\"] = 1\n",
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4001"));
    assert!(rendered.contains("is not writable via `__setitem__`"));
}

#[test]
fn check_accepts_unique_module_symbols() {
    let result = check(&ModuleGraph {
        nodes: vec![ModuleNode {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                    metadata: Default::default(),
                    detail: String::new(),
                    value_type: None,
                    value_type_expr: None,
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
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    metadata: Default::default(),
                    detail: String::new(),
                    value_type: None,
                    value_type_expr: None,
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
            ],
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
            summary_fingerprint: 1,
            calls: Vec::new(),
            method_calls: Vec::new(),
        }],
    });

    assert!(result.diagnostics.is_empty(), "{}", result.diagnostics.as_text());
}
