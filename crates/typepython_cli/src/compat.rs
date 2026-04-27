use std::process::ExitCode;

use anyhow::Result;

use crate::{
    cli::{CompatArgs, VerifyArgs},
    verification::{expand_checker_list, run_verify_with_command},
};

pub(crate) fn run_compat(args: CompatArgs) -> Result<ExitCode> {
    let checkers = expand_checker_list(&args.checkers)?;
    let verify_args = VerifyArgs {
        run: args.run,
        wheels: Vec::new(),
        sdists: Vec::new(),
        checkers,
        checker_preset: None,
        checker_allowlist: args.checker_allowlist,
        unsafe_runtime_imports: false,
    };
    let command_name =
        if args.strict_portability { "compat --strict-portability" } else { "compat" };
    run_verify_with_command(command_name, verify_args)
}
