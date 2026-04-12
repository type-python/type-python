use super::*;
use proptest::{prelude::*, test_runner::Config as ProptestConfig};

const RECURSIVE_JSON_SOURCE_PREFIX: &str = concat!(
    "typealias JsonObject = dict[str, JsonValue]\n",
    "typealias JsonArray = list[JsonValue]\n",
    "typealias JsonValue = None | bool | int | str | JsonObject | JsonArray\n\n",
);

#[derive(Debug, Clone)]
enum TreeValue {
    Int(i16),
    List(Vec<TreeValue>),
}

impl TreeValue {
    fn render(&self) -> String {
        match self {
            Self::Int(value) => value.to_string(),
            Self::List(items) => {
                format!("[{}]", items.iter().map(TreeValue::render).collect::<Vec<_>>().join(", "))
            }
        }
    }
}

#[derive(Debug, Clone)]
enum JsonValue {
    Null,
    Bool(bool),
    Int(i16),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    fn render(&self) -> String {
        match self {
            Self::Null => String::from("None"),
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::Str(value) => format!("\"{value}\""),
            Self::Array(items) => {
                format!("[{}]", items.iter().map(JsonValue::render).collect::<Vec<_>>().join(", "))
            }
            Self::Object(entries) => format!(
                "{{{}}}",
                entries
                    .iter()
                    .map(|(key, value)| format!("\"{key}\": {}", value.render()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[derive(Debug, Clone)]
enum JsonWithSet {
    Value(JsonValue),
    Set(Vec<JsonValue>),
    Array(Vec<JsonWithSet>),
    Object(Vec<(String, JsonWithSet)>),
}

impl JsonWithSet {
    fn contains_set(&self) -> bool {
        match self {
            Self::Value(_) => false,
            Self::Set(_) => true,
            Self::Array(items) => items.iter().any(JsonWithSet::contains_set),
            Self::Object(entries) => entries.iter().any(|(_, value)| value.contains_set()),
        }
    }

    fn render(&self) -> String {
        match self {
            Self::Value(value) => value.render(),
            Self::Set(items) => format!(
                "{{{}}}",
                items.iter().map(JsonValue::render).collect::<Vec<_>>().join(", ")
            ),
            Self::Array(items) => format!(
                "[{}]",
                items.iter().map(JsonWithSet::render).collect::<Vec<_>>().join(", ")
            ),
            Self::Object(entries) => format!(
                "{{{}}}",
                entries
                    .iter()
                    .map(|(key, value)| format!("\"{key}\": {}", value.render()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

fn identifier_fragment_strategy() -> impl Strategy<Value = String> {
    "[a-z]{1,8}"
}

fn json_scalar_strategy() -> impl Strategy<Value = JsonValue> {
    prop_oneof![
        Just(JsonValue::Null),
        any::<bool>().prop_map(JsonValue::Bool),
        (-8i16..=8).prop_map(JsonValue::Int),
        "[a-z]{0,8}".prop_map(JsonValue::Str),
    ]
}

fn tree_value_strategy() -> impl Strategy<Value = TreeValue> {
    (-8i16..=8).prop_map(TreeValue::Int).prop_recursive(4, 48, 6, |inner| {
        prop::collection::vec(inner, 0..=4).prop_map(TreeValue::List)
    })
}

fn json_value_strategy() -> impl Strategy<Value = JsonValue> {
    json_scalar_strategy().prop_recursive(4, 64, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..=4).prop_map(JsonValue::Array),
            prop::collection::btree_map(identifier_fragment_strategy(), inner, 0..=4)
                .prop_map(|entries| JsonValue::Object(entries.into_iter().collect())),
        ]
    })
}

fn json_with_set_strategy() -> impl Strategy<Value = JsonWithSet> {
    prop_oneof![
        json_scalar_strategy().prop_map(JsonWithSet::Value),
        prop::collection::vec(json_scalar_strategy(), 1..=3).prop_map(JsonWithSet::Set),
    ]
    .prop_recursive(4, 64, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..=4).prop_map(JsonWithSet::Array),
            prop::collection::btree_map(identifier_fragment_strategy(), inner, 0..=4)
                .prop_map(|entries| JsonWithSet::Object(entries.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn recursive_tree_alias_accepts_generated_values(value in tree_value_strategy()) {
        let source = format!(
            "typealias Tree = int | list[Tree]\n\nvalue: Tree = {}\n",
            value.render()
        );

        let result = check_temp_typepython_source(&source);
        prop_assert!(
            !result.diagnostics.has_errors(),
            "expected recursive Tree alias to accept source:\n{source}\n{}",
            result.diagnostics.as_text(),
        );
    }

    #[test]
    fn generic_recursive_alias_accepts_generated_values(value in tree_value_strategy()) {
        let source = format!(
            "typealias Nested[T] = T | list[Nested[T]]\n\nvalue: Nested[int] = {}\n",
            value.render()
        );

        let result = check_temp_typepython_source(&source);
        prop_assert!(
            !result.diagnostics.has_errors(),
            "expected generic recursive alias to accept source:\n{source}\n{}",
            result.diagnostics.as_text(),
        );
    }

    #[test]
    fn mutual_recursive_json_alias_accepts_generated_values(value in json_value_strategy()) {
        let source = format!(
            "{RECURSIVE_JSON_SOURCE_PREFIX}payload: JsonValue = {}\n",
            value.render()
        );

        let result = check_temp_typepython_source(&source);
        prop_assert!(
            !result.diagnostics.has_errors(),
            "expected recursive JsonValue alias to accept source:\n{source}\n{}",
            result.diagnostics.as_text(),
        );
    }

    #[test]
    fn mutual_recursive_json_alias_rejects_generated_set_values(
        value in json_with_set_strategy().prop_filter(
            "generated value must contain at least one set literal",
            JsonWithSet::contains_set,
        )
    ) {
        let source = format!(
            "{RECURSIVE_JSON_SOURCE_PREFIX}payload: JsonValue = {}\n",
            value.render()
        );

        let result = check_temp_typepython_source(&source);
        let rendered = result.diagnostics.as_text();
        prop_assert!(
            result.diagnostics.has_errors(),
            "expected recursive JsonValue alias to reject source:\n{source}\n{rendered}",
        );
        prop_assert!(
            rendered.contains("JsonValue") || rendered.contains("set["),
            "expected recursive JsonValue diagnostics for source:\n{source}\n{rendered}",
        );
    }
}
