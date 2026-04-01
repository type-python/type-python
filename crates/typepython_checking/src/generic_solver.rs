use std::collections::BTreeMap;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct GenericSolution {
    pub(crate) types: BTreeMap<String, String>,
    pub(crate) param_lists: BTreeMap<String, ParamListBinding>,
    pub(crate) type_packs: BTreeMap<String, TypePackBinding>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ParamListBinding {
    pub(crate) params: Vec<typepython_syntax::DirectFunctionParamSite>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct TypePackBinding {
    pub(crate) types: Vec<String>,
}
