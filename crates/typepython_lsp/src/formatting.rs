use super::*;

pub(super) fn resolve_formatter_commands(
    config: &ConfigHandle,
    path: &Path,
) -> Vec<FormatterCommand> {
    let file = path.to_string_lossy().into_owned();
    let workspace_root = config.config_dir.to_string_lossy().into_owned();

    if let Some(command) = &config.config.format.command {
        let expanded = command
            .iter()
            .map(|part| expand_formatter_argument(config, part, &file, &workspace_root))
            .collect::<Vec<_>>();
        let program = resolve_formatter_program(config, &expanded[0]);
        return vec![FormatterCommand {
            label: expanded.join(" "),
            program,
            args: expanded[1..].to_vec(),
            explicit: true,
        }];
    }

    let line_length = config.config.format.line_length.to_string();
    let python = resolve_python_executable(config);
    vec![
        FormatterCommand {
            label: format!("{} -m ruff format", python.display()),
            program: python.clone(),
            args: vec![
                String::from("-m"),
                String::from("ruff"),
                String::from("format"),
                String::from("--line-length"),
                line_length.clone(),
                String::from("--stdin-filename"),
                file.clone(),
                String::from("-"),
            ],
            explicit: false,
        },
        FormatterCommand {
            label: format!("{} -m black", python.display()),
            program: python,
            args: vec![
                String::from("-m"),
                String::from("black"),
                String::from("--quiet"),
                String::from("--line-length"),
                line_length.clone(),
                String::from("--stdin-filename"),
                file.clone(),
                String::from("-"),
            ],
            explicit: false,
        },
        FormatterCommand {
            label: String::from("ruff format"),
            program: PathBuf::from("ruff"),
            args: vec![
                String::from("format"),
                String::from("--line-length"),
                line_length.clone(),
                String::from("--stdin-filename"),
                file.clone(),
                String::from("-"),
            ],
            explicit: false,
        },
        FormatterCommand {
            label: String::from("black"),
            program: PathBuf::from("black"),
            args: vec![
                String::from("--quiet"),
                String::from("--line-length"),
                line_length,
                String::from("--stdin-filename"),
                file,
                String::from("-"),
            ],
            explicit: false,
        },
    ]
}

pub(super) fn expand_formatter_argument(
    config: &ConfigHandle,
    argument: &str,
    file: &str,
    workspace_root: &str,
) -> String {
    let has_path_placeholder = argument.contains("{file}") || argument.contains("{workspace_root}");
    let expanded = argument.replace("{file}", file).replace("{workspace_root}", workspace_root);
    if expanded.starts_with('-') {
        return expanded;
    }
    if expanded == file || expanded == workspace_root {
        return expanded;
    }

    // A lone backslash is also common in regexes and escape sequences passed to formatters. Treat
    // backslash-only relative arguments as paths only when they use an explicit relative prefix;
    // portable implicit relative paths use `/`, as documented by the configuration examples.
    let is_explicit_path = has_path_placeholder
        || Path::new(&expanded).is_absolute()
        || argument.contains('/')
        || argument.starts_with(r".\")
        || argument.starts_with(r"..\")
        || argument.starts_with(r"\\");
    if !is_explicit_path {
        return expanded;
    }

    typepython_config::resolve_command_path(&config.config_dir, &expanded)
        .to_string_lossy()
        .into_owned()
}

pub(super) fn resolve_formatter_program(config: &ConfigHandle, program: &str) -> PathBuf {
    typepython_config::resolve_command_path(&config.config_dir, program)
}

pub(super) fn run_formatter(
    commands: &[FormatterCommand],
    input: &str,
) -> Result<String, LspError> {
    let mut attempted = Vec::new();
    for command in commands {
        attempted.push(command.label.clone());
        match run_formatter_command(command, input)? {
            Some(output) => return Ok(output),
            None => continue,
        }
    }

    Err(LspError::request_failed(format!(
        "TPY6003: no formatter backend is available; tried {}",
        attempted.join(", ")
    ))
    .with_tpy_code("TPY6003"))
}

pub(super) fn run_formatter_command(
    command: &FormatterCommand,
    input: &str,
) -> Result<Option<String>, LspError> {
    let mut child = match ProcessCommand::new(&command.program)
        .args(&command.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if !command.explicit && error.kind() == io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => {
            return Err(LspError::request_failed(format!(
                "TPY6003: unable to start formatter `{}`: {}",
                command.label, error
            ))
            .with_tpy_code("TPY6003"));
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).map_err(|error| {
            LspError::request_failed(format!(
                "TPY6003: unable to write formatter input for `{}`: {}",
                command.label, error
            ))
            .with_tpy_code("TPY6003")
        })?;
    }

    let output = child.wait_with_output().map_err(|error| {
        LspError::request_failed(format!(
            "TPY6003: formatter `{}` did not complete successfully: {}",
            command.label, error
        ))
        .with_tpy_code("TPY6003")
    })?;
    if output.status.success() {
        return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !command.explicit && formatter_backend_unavailable(&stderr) {
        return Ok(None);
    }

    Err(LspError::request_failed(format!(
        "TPY6003: formatter `{}` exited with status {}{}",
        command.label,
        output.status,
        formatter_stderr_suffix(stderr.trim())
    ))
    .with_tpy_code("TPY6003"))
}

pub(super) fn formatter_backend_unavailable(stderr: &str) -> bool {
    stderr.contains("No module named ruff")
        || stderr.contains("No module named black")
        || stderr.contains("No module named 'ruff'")
        || stderr.contains("No module named 'black'")
}

pub(super) fn formatter_stderr_suffix(stderr: &str) -> String {
    if stderr.is_empty() { String::new() } else { format!(": {stderr}") }
}
