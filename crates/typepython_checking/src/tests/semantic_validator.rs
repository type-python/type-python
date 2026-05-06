use super::*;

fn check_validator_source(source: &str) -> crate::CheckResult {
    check_temp_typepython_source_with_checker_options(
        source,
        ParseOptions::default(),
        crate::CheckerOptions::default()
            .with_experimental_features(false, false, true)
            .with_framework_adapters(true),
    )
}

#[test]
fn check_validator_witness_is_disabled_without_experimental_acceptance() {
    let result = check_temp_typepython_source(concat!(
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "from typing import Literal\n\n",
        "def validate_user(value: unknown) -> ValidatorWitness[User, Literal[\"trusted\"]]:\n",
        "    ...\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(!rendered.contains("validator witness"), "{rendered}");
}

#[test]
fn check_validator_witness_narrows_unknown_in_true_branch() {
    let result = check_validator_source(concat!(
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "from typing import Literal\n\n",
        "def validate_user(value: unknown) -> ValidatorWitness[User, Literal[\"trusted\"]]:\n",
        "    ...\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_trust_metadata_decorator_produces_generated_witness() {
    let result = check_validator_source(concat!(
        "from typing import Callable\n\n",
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "def validator[T](target: type[T], trust: str):\n",
        "    def wrap[**P](fn: Callable[P, bool]) -> Callable[P, bool]:\n",
        "        return fn\n",
        "    return wrap\n\n",
        "@validator(User, trust=\"generated\")\n",
        "def validate_user(value: unknown) -> bool:\n",
        "    ...\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_trust_metadata_decorator_uses_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_experimental_overrides(
        concat!(
            "from typing import Callable\n\n",
            "class User:\n",
            "    name: str\n",
            "    def greet(self) -> str:\n",
            "        return self.name\n\n",
            "def validator[T](target: type[T], trust: str):\n",
            "    def wrap[**P](fn: Callable[P, bool]) -> Callable[P, bool]:\n",
            "        return fn\n",
            "    return wrap\n\n",
            "@validator(User, trust=\"generated\")\n",
            "def validate_user(value: unknown) -> bool:\n",
            "    ...\n\n",
            "def handle(value: unknown) -> str:\n",
            "    if validate_user(value):\n",
            "        return value.greet()\n",
            "    return \"\"\n",
        ),
        ParseOptions::default(),
        false,
        false,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_imported_trust_metadata_validator_produces_witness() {
    let root = create_temp_typepython_root();
    let lib_path = root.join("lib.tpy");
    let app_path = root.join("app.tpy");
    let lib_source = concat!(
        "from typing import Callable\n\n",
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "def validator[T](target: type[T], trust: str):\n",
        "    def wrap[**P](fn: Callable[P, bool]) -> Callable[P, bool]:\n",
        "        return fn\n",
        "    return wrap\n\n",
        "@validator(User, trust=\"generated\")\n",
        "def validate_user(value: unknown) -> bool:\n",
        "    ...\n",
    );
    let app_source = concat!(
        "from lib import User, validate_user\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    );
    fs::write(&lib_path, lib_source).expect("temp source should be written");
    fs::write(&app_path, app_source).expect("temp source should be written");

    let trees = [
        parse_with_options(
            SourceFile {
                path: lib_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("lib"),
                text: lib_source.to_owned(),
            },
            ParseOptions::default(),
        ),
        parse_with_options(
            SourceFile {
                path: app_path,
                kind: SourceKind::TypePython,
                logical_module: String::from("app"),
                text: app_source.to_owned(),
            },
            ParseOptions::default(),
        ),
    ];
    let bindings = trees.iter().map(bind).collect::<Vec<_>>();
    let graph = build(&bindings);
    let result = check_with_experimental_binding_metadata(
        &graph,
        &bindings,
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
        ImportFallback::Unknown,
        None,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn check_adapter_declared_validator_witness_trusts_one_arg_witness() {
    let result = check_validator_source(concat!(
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "def framework_transform(*args, **kwargs):\n",
        "    def wrap(obj):\n",
        "        return obj\n",
        "    return wrap\n\n",
        "@framework_transform(kind=\"function_decorator\", capabilities=(\"validator_witness\",))\n",
        "def trusted_boundary(fn):\n",
        "    return fn\n\n",
        "@trusted_boundary\n",
        "def validate_user(value: unknown) -> ValidatorWitness[User]:\n",
        "    ...\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_adapter_declared_validator_witness_uses_source_overrides_without_backing_file() {
    let result = check_virtual_source_with_experimental_overrides(
        concat!(
            "class User:\n",
            "    name: str\n",
            "    def greet(self) -> str:\n",
            "        return self.name\n\n",
            "def framework_transform(*args, **kwargs):\n",
            "    def wrap(obj):\n",
            "        return obj\n",
            "    return wrap\n\n",
            "@framework_transform(kind=\"function_decorator\", capabilities=(\"validator_witness\",))\n",
            "def trusted_boundary(fn):\n",
            "    return fn\n\n",
            "@trusted_boundary\n",
            "def validate_user(value: unknown) -> ValidatorWitness[User]:\n",
            "    ...\n\n",
            "def handle(value: unknown) -> str:\n",
            "    if validate_user(value):\n",
            "        return value.greet()\n",
            "    return \"\"\n",
        ),
        ParseOptions::default(),
        false,
        false,
    );

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_validator_witness_does_not_narrow_after_reassignment() {
    let result = check_validator_source(concat!(
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "from typing import Literal\n\n",
        "def validate_user(value: unknown) -> ValidatorWitness[User, Literal[\"trusted\"]]:\n",
        "    ...\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        value = get_unknown()\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("invalidated by assignment or mutation"), "{rendered}");
}

#[test]
fn check_validator_witness_does_not_narrow_after_subscript_mutation() {
    let result = check_validator_source(concat!(
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "from typing import Literal\n\n",
        "def validate_user(value: unknown) -> ValidatorWitness[User, Literal[\"trusted\"]]:\n",
        "    ...\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        value[\"name\"] = get_unknown()\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("invalidated by assignment or mutation"), "{rendered}");
}

#[test]
fn check_validator_witness_does_not_narrow_after_attribute_mutation() {
    let result = check_validator_source(concat!(
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "from typing import Literal\n\n",
        "def validate_user(value: unknown) -> ValidatorWitness[User, Literal[\"trusted\"]]:\n",
        "    ...\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        value.name = get_unknown()\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("invalidated by assignment or mutation"), "{rendered}");
}

#[test]
fn check_validator_witness_does_not_narrow_after_alias_mutation() {
    let result = check_validator_source(concat!(
        "class User:\n",
        "    name: str\n",
        "    def greet(self) -> str:\n",
        "        return self.name\n\n",
        "from typing import Literal\n\n",
        "def validate_user(value: unknown) -> ValidatorWitness[User, Literal[\"trusted\"]]:\n",
        "    ...\n\n",
        "def handle(value: unknown) -> str:\n",
        "    if validate_user(value):\n",
        "        alias = value\n",
        "        alias.name = get_unknown()\n",
        "        return value.greet()\n",
        "    return \"\"\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("invalidated by assignment or mutation"), "{rendered}");
}

#[test]
fn check_validator_witness_without_trust_metadata_does_not_narrow() {
    let result = check_temp_typepython_source_with_experimental_check_options(
        concat!(
            "class User:\n",
            "    name: str\n",
            "    def greet(self) -> str:\n",
            "        return self.name\n\n",
            "def validate_user(value: unknown) -> ValidatorWitness[User]:\n",
            "    ...\n\n",
            "def handle(value: unknown) -> str:\n",
            "    if validate_user(value):\n",
            "        return value.greet()\n",
            "    return \"\"\n",
        ),
        ParseOptions::default(),
        false,
        true,
        DiagnosticLevel::Warning,
        true,
        false,
    );

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4003"), "{rendered}");
    assert!(rendered.contains("crossed an untrusted boundary"), "{rendered}");
}
