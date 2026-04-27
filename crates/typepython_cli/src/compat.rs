use std::process::ExitCode;

use anyhow::Result;

use crate::{
    cli::{CompatArgs, VerifyArgs},
    verification::run_verify_with_command,
};

pub(crate) fn run_compat(args: CompatArgs) -> Result<ExitCode> {
    let checkers = expand_checker_list(&args.checkers)?;
    let verify_args = VerifyArgs {
        run: args.run,
        wheels: Vec::new(),
        sdists: Vec::new(),
        checkers,
        unsafe_runtime_imports: false,
    };
    let command_name =
        if args.strict_portability { "compat --strict-portability" } else { "compat" };
    run_verify_with_command(command_name, verify_args)
}

pub(crate) fn expand_checker_list(raw: &str) -> Result<Vec<String>> {
    let values = raw.split(',').map(str::trim).filter(|value| !value.is_empty());
    let mut checkers = Vec::new();
    for value in values {
        if value == "all" {
            checkers.extend([String::from("mypy"), String::from("pyright"), String::from("ty")]);
        } else {
            checkers.push(value.to_owned());
        }
    }
    if checkers.is_empty() {
        anyhow::bail!("--checkers must name at least one checker");
    }
    checkers.sort();
    checkers.dedup();
    Ok(checkers)
}
