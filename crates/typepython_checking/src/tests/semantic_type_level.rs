use super::*;

#[test]
fn check_accepts_supported_restricted_type_level_shape_aliases() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    id: int\n",
        "    name: str\n\n",
        "typealias Names = RequiredKeys[User]\n",
        "typealias PublicUser = Pick[User, Literal[\"id\"]]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(!rendered.contains("TPY4027"), "{rendered}");
}

#[test]
fn check_reports_unsupported_restricted_type_level_alias() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    name: str\n\n",
        "typealias Names = MapValues[User, Callable]\n",
    ));

    let rendered = result.diagnostics.as_text();
    assert!(rendered.contains("TPY4027"), "{rendered}");
    assert!(rendered.contains("unsupported form `MapValues[Callable]`"), "{rendered}");
}

#[test]
fn check_uses_projected_shape_alias_for_typed_dict_literals() {
    let result = check_temp_typepython_source(concat!(
        "class User(TypedDict):\n",
        "    id: int\n",
        "    name: str\n\n",
        "typealias PublicUser = Pick[User, Literal[\"id\"]]\n\n",
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
        "typealias PublicUser = Pick[User, Literal[\"id\"]]\n\n",
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
