use std::process::ExitCode;

use anyhow::Result;

use crate::{
    cli::{CompatArgs, VerifyArgs},
    verification::{expand_checker_list, run_verify_with_command},
};

pub(crate) fn run_compat(args: CompatArgs) -> Result<ExitCode> {
    let checkers = match args.profile.as_deref() {
        Some(profile) => expand_compat_profile(profile)?,
        None => expand_checker_list(&args.checkers)?,
    };
    let checker_allowlist =
        compat_checker_allowlist(args.strict_portability, args.checker_allowlist);
    let verify_args = VerifyArgs {
        run: args.run,
        wheels: Vec::new(),
        sdists: Vec::new(),
        checkers,
        checker_preset: None,
        checker_allowlist,
        unsafe_runtime_imports: false,
        publication_type_health: false,
    };
    let command_name =
        if args.strict_portability { "compat --strict-portability" } else { "compat" };
    run_verify_with_command(command_name, verify_args)
}

pub(crate) fn compat_checker_allowlist(
    strict_portability: bool,
    checker_allowlist: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if strict_portability { None } else { checker_allowlist }
}

pub(crate) fn expand_compat_profile(profile: &str) -> Result<Vec<String>> {
    match profile {
        "library-portable" | "app-strict" => expand_checker_list("all"),
        "pyright-first" => expand_checker_list("pyright"),
        "mypy-compatible" => expand_checker_list("mypy"),
        "experimental-checkers" => expand_checker_list("all,pyrefly,basedpyright,zuban"),
        unknown => anyhow::bail!(
            "unknown compat profile `{unknown}`; expected library-portable, app-strict, pyright-first, mypy-compatible, or experimental-checkers"
        ),
    }
}
