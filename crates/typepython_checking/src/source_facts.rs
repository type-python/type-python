use std::{cell::RefCell, collections::BTreeMap};

use typepython_binding::{Declaration, ModuleSurfaceFacts};
use typepython_graph::ModuleNode;
use typepython_syntax::{
    ConditionalReturnSite, DataclassTransformModuleInfo, DecoratorTransformModuleInfo,
    DirectFunctionParamSite, DirectMethodSignatureSite, FrozenFieldMutationSite,
    ModuleSurfaceMetadata, SourceFile, TypedDictClassMetadata, TypedDictLiteralSite,
    TypedDictMutationSite, UnsafeOperationSite,
};

use crate::{SemanticDeclarationFacts, declaration_semantic_facts};

pub(super) type TypedDictClassMetadataByName = BTreeMap<String, TypedDictClassMetadata>;
pub(super) type DirectFunctionSignaturesByName = BTreeMap<String, Vec<DirectFunctionParamSite>>;
pub(super) type DirectMethodSignaturesByName =
    BTreeMap<(String, String), Vec<DirectFunctionParamSite>>;

#[derive(Debug, Default)]
struct FallbackModuleSourceFacts {
    source_loaded: bool,
    source_text: Option<String>,
    surface_metadata_loaded: bool,
    surface_metadata: Option<ModuleSurfaceMetadata>,
    typed_dict_literal_sites: Option<Vec<TypedDictLiteralSite>>,
    typed_dict_mutation_sites: Option<Vec<TypedDictMutationSite>>,
    frozen_field_mutation_sites: Option<Vec<FrozenFieldMutationSite>>,
    unsafe_operation_sites: Option<Vec<UnsafeOperationSite>>,
    conditional_return_sites: Option<Vec<ConditionalReturnSite>>,
}

impl FallbackModuleSourceFacts {
    fn source_text<'a>(
        &'a mut self,
        node: &ModuleNode,
        source_overrides: Option<&BTreeMap<String, String>>,
    ) -> Option<&'a str> {
        if !self.source_loaded {
            self.source_text = source_overrides
                .and_then(|overrides| {
                    overrides.get(&node.module_path.display().to_string()).cloned()
                })
                .or_else(|| {
                    (!is_virtual_module_path(node))
                        .then(|| SourceFile::from_path(&node.module_path).map(|source| source.text))
                        .and_then(Result::ok)
                });
            self.source_loaded = true;
        }

        self.source_text.as_deref()
    }

    fn module_surface_metadata<'a>(
        &'a mut self,
        node: &ModuleNode,
        source_overrides: Option<&BTreeMap<String, String>>,
    ) -> Option<&'a ModuleSurfaceMetadata> {
        if !self.surface_metadata_loaded {
            self.surface_metadata = self
                .source_text(node, source_overrides)
                .map(typepython_syntax::collect_module_surface_metadata);
            self.surface_metadata_loaded = true;
        }

        self.surface_metadata.as_ref()
    }

    fn typed_dict_literal_sites(
        &mut self,
        node: &ModuleNode,
        source_overrides: Option<&BTreeMap<String, String>>,
    ) -> &[TypedDictLiteralSite] {
        if self.typed_dict_literal_sites.is_none() {
            self.typed_dict_literal_sites = Some(
                self.source_text(node, source_overrides)
                    .map(typepython_syntax::collect_typed_dict_literal_sites)
                    .unwrap_or_default(),
            );
        }

        self.typed_dict_literal_sites.as_deref().unwrap_or(&[])
    }

    fn typed_dict_mutation_sites(
        &mut self,
        node: &ModuleNode,
        source_overrides: Option<&BTreeMap<String, String>>,
    ) -> &[TypedDictMutationSite] {
        if self.typed_dict_mutation_sites.is_none() {
            self.typed_dict_mutation_sites = Some(
                self.source_text(node, source_overrides)
                    .map(typepython_syntax::collect_typed_dict_mutation_sites)
                    .unwrap_or_default(),
            );
        }

        self.typed_dict_mutation_sites.as_deref().unwrap_or(&[])
    }

    fn frozen_field_mutation_sites(
        &mut self,
        node: &ModuleNode,
        source_overrides: Option<&BTreeMap<String, String>>,
    ) -> &[FrozenFieldMutationSite] {
        if self.frozen_field_mutation_sites.is_none() {
            self.frozen_field_mutation_sites = Some(
                self.source_text(node, source_overrides)
                    .map(typepython_syntax::collect_frozen_field_mutation_sites)
                    .unwrap_or_default(),
            );
        }

        self.frozen_field_mutation_sites.as_deref().unwrap_or(&[])
    }

    fn unsafe_operation_sites(
        &mut self,
        node: &ModuleNode,
        source_overrides: Option<&BTreeMap<String, String>>,
    ) -> &[UnsafeOperationSite] {
        if self.unsafe_operation_sites.is_none() {
            self.unsafe_operation_sites = Some(
                self.source_text(node, source_overrides)
                    .map(typepython_syntax::collect_unsafe_operation_sites)
                    .unwrap_or_default(),
            );
        }

        self.unsafe_operation_sites.as_deref().unwrap_or(&[])
    }

    fn conditional_return_sites(
        &mut self,
        node: &ModuleNode,
        source_overrides: Option<&BTreeMap<String, String>>,
    ) -> &[ConditionalReturnSite] {
        if self.conditional_return_sites.is_none() {
            self.conditional_return_sites = Some(
                self.source_text(node, source_overrides)
                    .map(typepython_syntax::collect_conditional_return_sites)
                    .unwrap_or_default(),
            );
        }

        self.conditional_return_sites.as_deref().unwrap_or(&[])
    }
}

fn is_virtual_module_path(node: &ModuleNode) -> bool {
    node.module_path.to_string_lossy().starts_with('<')
}

pub(super) fn surface_typed_dict_class_metadata(
    metadata: &ModuleSurfaceMetadata,
) -> TypedDictClassMetadataByName {
    metadata
        .typed_dict_classes
        .iter()
        .cloned()
        .map(|typed_dict| (typed_dict.name.clone(), typed_dict))
        .collect()
}

pub(super) fn surface_direct_function_signatures(
    metadata: &ModuleSurfaceMetadata,
) -> DirectFunctionSignaturesByName {
    metadata
        .direct_function_signatures
        .iter()
        .cloned()
        .map(|signature| (signature.name, signature.params))
        .collect()
}

pub(super) fn surface_direct_method_signatures(
    metadata: &ModuleSurfaceMetadata,
) -> DirectMethodSignaturesByName {
    metadata
        .direct_method_signatures
        .iter()
        .cloned()
        .map(|signature: DirectMethodSignatureSite| {
            let params = match signature.method_kind {
                typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => {
                    signature.params
                }
                typepython_syntax::MethodKind::Instance
                | typepython_syntax::MethodKind::Class
                | typepython_syntax::MethodKind::PropertySetter => {
                    signature.params.into_iter().skip(1).collect()
                }
            };
            ((signature.owner_type_name, signature.name), params)
        })
        .collect()
}

#[derive(Debug, Default)]
pub(super) struct CheckerSourceFactsProvider<'a> {
    bound_surface_facts: Option<&'a BTreeMap<String, ModuleSurfaceFacts>>,
    modules: RefCell<BTreeMap<String, FallbackModuleSourceFacts>>,
    source_overrides: Option<&'a BTreeMap<String, String>>,
}

impl<'a> CheckerSourceFactsProvider<'a> {
    pub(super) fn new(
        source_overrides: Option<&'a BTreeMap<String, String>>,
        bound_surface_facts: Option<&'a BTreeMap<String, ModuleSurfaceFacts>>,
    ) -> Self {
        Self { bound_surface_facts, modules: RefCell::new(BTreeMap::new()), source_overrides }
    }

    fn with_module_facts<T>(
        &self,
        node: &ModuleNode,
        action: impl FnOnce(&mut FallbackModuleSourceFacts) -> T,
    ) -> T {
        let cache_key = node.module_path.display().to_string();
        let mut modules = self.modules.borrow_mut();
        let facts = modules.entry(cache_key).or_default();
        action(facts)
    }

    fn bound_surface_facts(&self, node: &ModuleNode) -> Option<&ModuleSurfaceFacts> {
        self.bound_surface_facts.and_then(|facts| facts.get(&node.module_key))
    }

    pub(super) fn declaration_semantics(
        &self,
        declaration: &Declaration,
    ) -> SemanticDeclarationFacts {
        declaration_semantic_facts(declaration)
    }

    pub(super) fn typed_dict_class_metadata(
        &self,
        node: &ModuleNode,
    ) -> TypedDictClassMetadataByName {
        if let Some(bound) = self.bound_surface_facts(node) {
            return bound.typed_dict_class_metadata.clone();
        }

        self.with_module_facts(node, |facts| {
            facts
                .module_surface_metadata(node, self.source_overrides)
                .map(surface_typed_dict_class_metadata)
                .unwrap_or_default()
        })
    }

    pub(super) fn direct_function_signatures(
        &self,
        node: &ModuleNode,
    ) -> DirectFunctionSignaturesByName {
        if let Some(bound) = self.bound_surface_facts(node) {
            return bound.direct_function_signatures.clone();
        }

        self.with_module_facts(node, |facts| {
            facts
                .module_surface_metadata(node, self.source_overrides)
                .map(surface_direct_function_signatures)
                .unwrap_or_default()
        })
    }

    pub(super) fn direct_method_signatures(
        &self,
        node: &ModuleNode,
    ) -> DirectMethodSignaturesByName {
        if let Some(bound) = self.bound_surface_facts(node) {
            return bound.direct_method_signatures.clone();
        }

        self.with_module_facts(node, |facts| {
            facts
                .module_surface_metadata(node, self.source_overrides)
                .map(surface_direct_method_signatures)
                .unwrap_or_default()
        })
    }

    pub(super) fn decorator_transform_module_info(
        &self,
        node: &ModuleNode,
    ) -> Option<DecoratorTransformModuleInfo> {
        if let Some(bound) = self.bound_surface_facts(node) {
            return Some(bound.decorator_transform_module_info.clone());
        }

        self.with_module_facts(node, |facts| {
            facts
                .module_surface_metadata(node, self.source_overrides)
                .map(|metadata| metadata.decorator_transform.clone())
        })
    }

    pub(super) fn dataclass_transform_module_info(
        &self,
        node: &ModuleNode,
    ) -> Option<DataclassTransformModuleInfo> {
        if let Some(bound) = self.bound_surface_facts(node) {
            return Some(bound.dataclass_transform_module_info.clone());
        }

        self.with_module_facts(node, |facts| {
            facts
                .module_surface_metadata(node, self.source_overrides)
                .map(|metadata| metadata.dataclass_transform.clone())
        })
    }

    pub(super) fn typed_dict_literal_sites(&self, node: &ModuleNode) -> Vec<TypedDictLiteralSite> {
        self.with_module_facts(node, |facts| {
            facts.typed_dict_literal_sites(node, self.source_overrides).to_vec()
        })
    }

    pub(super) fn typed_dict_mutation_sites(
        &self,
        node: &ModuleNode,
    ) -> Vec<TypedDictMutationSite> {
        self.with_module_facts(node, |facts| {
            facts.typed_dict_mutation_sites(node, self.source_overrides).to_vec()
        })
    }

    pub(super) fn frozen_field_mutation_sites(
        &self,
        node: &ModuleNode,
    ) -> Vec<FrozenFieldMutationSite> {
        self.with_module_facts(node, |facts| {
            facts.frozen_field_mutation_sites(node, self.source_overrides).to_vec()
        })
    }

    pub(super) fn unsafe_operation_sites(&self, node: &ModuleNode) -> Vec<UnsafeOperationSite> {
        self.with_module_facts(node, |facts| {
            facts.unsafe_operation_sites(node, self.source_overrides).to_vec()
        })
    }

    pub(super) fn conditional_return_sites(&self, node: &ModuleNode) -> Vec<ConditionalReturnSite> {
        self.with_module_facts(node, |facts| {
            facts.conditional_return_sites(node, self.source_overrides).to_vec()
        })
    }
}
