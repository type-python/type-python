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
