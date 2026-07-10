use super::*;

#[test]
fn check_accepts_supported_restricted_type_level_shape_aliases() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    id: int\n",
        "    name: str\n\n",
        "typealias Names = RequiredKeys[User]\n",
        "typealias PublicUser = Pick[User, \"id\"]\n",
        "typealias PublicProfile = Pick[User, \"id\", \"name\"]\n",
        "typealias AnonymousUser = Omit[User, \"id\", \"name\"]\n",
        "typealias LiteralCompat = Pick[User, Literal[\"id\"]]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4027"), "{rendered}");
}

#[test]
fn check_reports_unsupported_restricted_type_level_alias() {
    let result = check_temp_typepython_source(concat!(
        "# module prelude\n",
        "\n",
        "class User(TypedDict):\n",
        "    name: str\n\n",
        "typealias Names = MapValues[User, Callable]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4027"), "{rendered}");
    assert!(rendered.contains("unsupported form `MapValues[Callable]`"), "{rendered}");
    let diagnostic = result
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPY4027")
        .expect("type-level diagnostic should be present");
    assert_eq!(diagnostic.span.as_ref().map(|span| span.line), Some(6));
}

#[test]
fn check_uses_projected_shape_alias_for_typed_dict_literals() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    id: int\n",
        "    name: str\n\n",
        "typealias PublicUser = Pick[User, \"id\"]\n\n",
        "good: PublicUser = {\"id\": 1}\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_projected_shape_alias_literal_mismatch() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    id: int\n",
        "    name: str\n\n",
        "typealias PublicUser = Pick[User, \"id\"]\n\n",
        "bad: PublicUser = {\"name\": \"Ada\"}\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4013"), "{rendered}");
    assert!(rendered.contains("missing required key `id`"), "{rendered}");
}

#[test]
fn check_uses_map_values_shape_alias_for_typed_dict_literals() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    id: int\n\n",
        "typealias OptionalUser = MapValues[User, Optional]\n\n",
        "good: OptionalUser = {\"id\": None}\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_accepts_transforms_of_multilevel_inherited_typed_dicts() {
    let result = check_temp_typepython_source(concat!(
        "class Base(TypedDict, total=False):\n",
        "    id: int\n",
        "    frozen: ReadOnly[bytes]\n\n",
        "class Mid(Base):\n",
        "    count: int\n\n",
        "class Leaf(Mid):\n",
        "    active: bool\n\n",
        "typealias MidPublic = Pick[Mid, \"id\", \"count\"]\n",
        "typealias Patch = Partial[Leaf]\n",
        "typealias Public = Pick[Leaf, \"id\", \"frozen\", \"active\"]\n",
        "typealias WithoutCount = Omit[Leaf, \"count\"]\n\n",
        "mid_public: MidPublic = {\"count\": 1}\n",
        "patch: Patch = {}\n",
        "public: Public = {\"active\": True}\n",
        "without_count: WithoutCount = {\"active\": True}\n",
    ));

    assert!(!result.diagnostics.has_errors(), "{}", result.diagnostics.as_text());
}

#[test]
fn check_reports_unknown_and_cyclic_shape_bases_without_panicking() {
    let result = check_temp_typepython_source(concat!(
        "class Broken(MissingBase):\n",
        "    value: int\n\n",
        "class A(B):\n",
        "    a: int\n\n",
        "class B(A):\n",
        "    b: int\n\n",
        "typealias BrokenPublic = Pick[Broken, \"value\"]\n",
        "typealias CyclicPublic = Pick[A, \"a\"]\n",
    ));

    let diagnostics = result
        .diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "TPY4027")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 2, "{}", result.diagnostics.as_text());
    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("shape source `Broken` is not known"), "{rendered}");
    assert!(rendered.contains("shape source `A` is not known"), "{rendered}");
}
