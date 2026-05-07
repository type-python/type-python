#[cfg(test)]
mod flow_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn validator_witness_trust_uses_explicit_metadata_arg() {
        assert!(!validator_witness_return_is_trusted(&[SemanticType::Name(String::from("User"))]));
        assert!(validator_witness_trust_marker(&SemanticType::Generic {
            head: String::from("Literal"),
            args: vec![SemanticType::Name(String::from("\"trusted\""))],
        }));
        assert!(!validator_witness_trust_marker(&SemanticType::Name(String::from("User"))));
    }

    fn validator_test_node(return_type: &str) -> typepython_graph::ModuleNode {
        typepython_graph::ModuleNode {
            module_path: PathBuf::from("validator-flow.tpy"),
            module_key: String::from("validator_flow"),
            module_kind: SourceKind::TypePython,
            declarations: vec![Declaration {
                name: String::from("validate_user"),
                kind: DeclarationKind::Function,
                metadata: typepython_binding::DeclarationMetadata::Callable {
                    signature: typepython_binding::BoundCallableSignature {
                        params: vec![typepython_syntax::DirectFunctionParamSite {
                            name: String::from("value"),
                            annotation: Some(String::from("unknown")),
                            annotation_expr: typepython_syntax::TypeExpr::parse("unknown"),
                            has_default: false,
                            positional_only: false,
                            keyword_only: false,
                            variadic: false,
                            keyword_variadic: false,
                        }],
                        returns: Some(typepython_binding::BoundTypeExpr::new(return_type)),
                    },
                },
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
            }],
            calls: Vec::new(),
            method_calls: Vec::new(),
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
            summary_fingerprint: 0,
        }
    }

    fn validator_context<'a>(
        nodes: &'a [typepython_graph::ModuleNode],
    ) -> CheckerContext<'a> {
        CheckerContext::new_with_bound_surface_facts_and_options(
            nodes,
            None,
            None,
            CheckerOptions::permissive_test_default()
                .with_experimental_features(false, false, true)
                .with_framework_adapters(true),
        )
    }

    #[test]
    fn validator_witness_requires_explicit_trust_to_narrow() {
        let node = validator_test_node("ValidatorWitness[User]");
        let nodes = vec![node.clone()];
        let context = validator_context(&nodes);
        let narrowed = apply_predicate_guard_semantic_with_context(
            &context,
            &node,
            &nodes,
            &SemanticType::Name(String::from("unknown")),
            "validate_user",
            true,
        );

        assert_eq!(narrowed, SemanticType::Name(String::from("unknown")));
    }

    #[test]
    fn validator_witness_with_explicit_trust_narrows_unknown() {
        let node = validator_test_node("ValidatorWitness[User, Literal[\"trusted\"]]");
        let nodes = vec![node.clone()];
        let context = validator_context(&nodes);
        let narrowed = apply_predicate_guard_semantic_with_context(
            &context,
            &node,
            &nodes,
            &SemanticType::Name(String::from("unknown")),
            "validate_user",
            true,
        );

        assert_eq!(narrowed, SemanticType::Name(String::from("User")));
    }
}
