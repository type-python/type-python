use super::*;

pub(super) fn source_range(source: &str, range: ruff_text_size::TextRange) -> SourceRange {
    let absolute_start = range.start().to_usize();
    let byte_length = range.end().to_usize().saturating_sub(absolute_start);
    let line_start = source[..absolute_start].rfind('\n').map_or(0, |index| index + 1);
    let start = absolute_start - line_start;
    SourceRange { start, end: start + byte_length }
}

mod ast_backed;
mod calls;
mod control_flow;
mod expr_metadata;
mod guards;
mod lambdas;
mod syntax_extensions;

pub(super) use ast_backed::*;
pub(super) use calls::*;
pub(super) use control_flow::*;
pub(super) use expr_metadata::*;
pub(super) use guards::*;
pub(super) use lambdas::*;
pub(super) use syntax_extensions::*;

#[cfg(test)]
mod tests;
