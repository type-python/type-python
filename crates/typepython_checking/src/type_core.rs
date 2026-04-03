use std::collections::BTreeMap;

use super::{CallableParamExpr, TypeExpr};

pub(crate) type TypeId = usize;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum SemanticType {
    Name(String),
    Generic { head: String, args: Vec<SemanticType> },
    Callable { params: SemanticCallableParams, return_type: Box<SemanticType> },
    Union(Vec<SemanticType>),
    Annotated { value: Box<SemanticType>, metadata: Vec<String> },
    Unpack(Box<SemanticType>),
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum SemanticCallableParams {
    Ellipsis,
    ParamList(Vec<SemanticType>),
    Concatenate(Vec<SemanticType>),
    Single(Box<SemanticType>),
}

#[derive(Debug, Default)]
pub(crate) struct TypeStore {
    arena: Vec<SemanticType>,
    ids_by_type: BTreeMap<SemanticType, TypeId>,
}

impl TypeStore {
    pub(crate) fn intern(&mut self, ty: SemanticType) -> TypeId {
        if let Some(existing) = self.ids_by_type.get(&ty) {
            return *existing;
        }
        let id = self.arena.len();
        self.arena.push(ty.clone());
        self.ids_by_type.insert(ty, id);
        id
    }

    pub(crate) fn get(&self, id: TypeId) -> Option<&SemanticType> {
        self.arena.get(id)
    }
}

impl SemanticType {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        TypeExpr::parse(text).map(lower_type_expr)
    }

    pub(crate) fn strip_annotated(&self) -> &Self {
        match self {
            Self::Annotated { value, .. } => value.strip_annotated(),
            _ => self,
        }
    }

    pub(crate) fn generic_parts(&self) -> Option<(&str, &[SemanticType])> {
        match self.strip_annotated() {
            Self::Generic { head, args } => Some((head.as_str(), args.as_slice())),
            _ => None,
        }
    }

    pub(crate) fn unpacked_inner(&self) -> Option<&SemanticType> {
        match self.strip_annotated() {
            Self::Unpack(inner) => Some(inner.as_ref()),
            _ => None,
        }
    }
}

pub(crate) fn lower_type_text(text: &str) -> Option<SemanticType> {
    SemanticType::parse(text)
}

pub(crate) fn lower_type_text_or_name(text: &str) -> SemanticType {
    lower_type_text(text).unwrap_or_else(|| SemanticType::Name(text.trim().to_owned()))
}

pub(crate) fn lower_type_expr(expr: TypeExpr) -> SemanticType {
    match expr {
        TypeExpr::Name(name) => SemanticType::Name(name),
        TypeExpr::Generic { head, args } => {
            SemanticType::Generic { head, args: args.into_iter().map(lower_type_expr).collect() }
        }
        TypeExpr::Callable { params, return_type } => SemanticType::Callable {
            params: lower_callable_params(*params),
            return_type: Box::new(lower_type_expr(*return_type)),
        },
        TypeExpr::Union { branches, .. } => {
            SemanticType::Union(branches.into_iter().map(lower_type_expr).collect())
        }
        TypeExpr::Annotated { value, metadata } => {
            SemanticType::Annotated { value: Box::new(lower_type_expr(*value)), metadata }
        }
        TypeExpr::Unpack(inner) => SemanticType::Unpack(Box::new(lower_type_expr(*inner))),
    }
}

pub(crate) fn render_semantic_type(ty: &SemanticType) -> String {
    match ty {
        SemanticType::Name(name) => name.clone(),
        SemanticType::Generic { head, args } => {
            let args = args.iter().map(render_semantic_type).collect::<Vec<_>>().join(", ");
            format!("{head}[{args}]")
        }
        SemanticType::Callable { params, return_type } => {
            format!(
                "Callable[{}, {}]",
                render_semantic_callable_params(params),
                render_semantic_type(return_type),
            )
        }
        SemanticType::Union(branches) => {
            let branches = branches.iter().map(render_semantic_type).collect::<Vec<_>>();
            if branches.len() == 1 {
                branches.into_iter().next().unwrap_or_default()
            } else {
                format!("Union[{}]", branches.join(", "))
            }
        }
        SemanticType::Annotated { value, metadata } => {
            if metadata.is_empty() {
                format!("Annotated[{}]", render_semantic_type(value))
            } else {
                format!("Annotated[{}, {}]", render_semantic_type(value), metadata.join(", "))
            }
        }
        SemanticType::Unpack(inner) => format!("Unpack[{}]", render_semantic_type(inner)),
    }
}

pub(crate) fn semantic_union_branches(ty: &SemanticType) -> Option<Vec<SemanticType>> {
    match ty.strip_annotated() {
        SemanticType::Union(branches) => Some(branches.clone()),
        SemanticType::Generic { head, args } if head == "Optional" && args.len() == 1 => {
            Some(vec![args[0].clone(), SemanticType::Name(String::from("None"))])
        }
        _ => None,
    }
}

pub(crate) fn join_semantic_type_candidates(candidates: Vec<SemanticType>) -> SemanticType {
    let mut branches = Vec::new();
    for candidate in candidates {
        if let Some(candidate_branches) = semantic_union_branches(&candidate) {
            for branch in candidate_branches {
                if !branches.contains(&branch) {
                    branches.push(branch);
                }
            }
        } else if !branches.contains(&candidate) {
            branches.push(candidate);
        }
    }
    if branches.len() == 1 {
        branches.into_iter().next().unwrap_or_else(|| SemanticType::Name(String::from("dynamic")))
    } else {
        SemanticType::Union(branches)
    }
}

pub(crate) fn unpacked_fixed_tuple_semantic_elements(
    ty: &SemanticType,
) -> Option<Vec<SemanticType>> {
    let (head, args) = ty.generic_parts()?;
    if head != "tuple" {
        return None;
    }
    if args.len() == 2 && matches!(&args[1], SemanticType::Name(name) if name == "...") {
        return None;
    }
    Some(args.to_vec())
}

pub(crate) fn expanded_tuple_shape_semantic_args(args: &[SemanticType]) -> Vec<SemanticType> {
    let mut expanded = Vec::new();
    for arg in args {
        if let Some(inner) = arg.unpacked_inner()
            && let Some(elements) = unpacked_fixed_tuple_semantic_elements(inner)
        {
            expanded.extend(elements);
            continue;
        }
        expanded.push(arg.clone());
    }
    expanded
}

pub(crate) fn lower_callable_params(params: CallableParamExpr) -> SemanticCallableParams {
    match params {
        CallableParamExpr::Ellipsis => SemanticCallableParams::Ellipsis,
        CallableParamExpr::ParamList(types) => {
            SemanticCallableParams::ParamList(types.into_iter().map(lower_type_expr).collect())
        }
        CallableParamExpr::Concatenate(types) => {
            SemanticCallableParams::Concatenate(types.into_iter().map(lower_type_expr).collect())
        }
        CallableParamExpr::Single(expr) => {
            SemanticCallableParams::Single(Box::new(lower_type_expr(*expr)))
        }
    }
}

pub(crate) fn render_semantic_callable_params(params: &SemanticCallableParams) -> String {
    match params {
        SemanticCallableParams::Ellipsis => String::from("..."),
        SemanticCallableParams::ParamList(types) => {
            let types = types.iter().map(render_semantic_type).collect::<Vec<_>>().join(", ");
            format!("[{types}]")
        }
        SemanticCallableParams::Concatenate(types) => {
            let types = types.iter().map(render_semantic_type).collect::<Vec<_>>().join(", ");
            format!("Concatenate[{types}]")
        }
        SemanticCallableParams::Single(expr) => render_semantic_type(expr),
    }
}
