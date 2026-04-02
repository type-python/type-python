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
    let expanded = argument.replace("{file}", file).replace("{workspace_root}", workspace_root);
    if expanded.starts_with('-') {
        return expanded;
    }
    let path = Path::new(&expanded);
    if path.is_absolute()
        || !expanded.contains(std::path::MAIN_SEPARATOR)
        || expanded == file
        || expanded == workspace_root
    {
        return expanded;
    }
    config.config_dir.join(path).to_string_lossy().into_owned()
}

pub(super) fn resolve_formatter_program(config: &ConfigHandle, program: &str) -> PathBuf {
    let path = Path::new(program);
    if path.is_absolute() || !program.contains(std::path::MAIN_SEPARATOR) {
        return path.to_path_buf();
    }
    config.config_dir.join(path)
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

    Err(LspError::Other(format!(
        "TPY6003: no formatter backend is available; tried {}",
        attempted.join(", ")
    )))
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
            return Err(LspError::Other(format!(
                "TPY6003: unable to start formatter `{}`: {}",
                command.label, error
            )));
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).map_err(|error| {
            LspError::Other(format!(
                "TPY6003: unable to write formatter input for `{}`: {}",
                command.label, error
            ))
        })?;
    }

    let output = child.wait_with_output().map_err(|error| {
        LspError::Other(format!(
            "TPY6003: formatter `{}` did not complete successfully: {}",
            command.label, error
        ))
    })?;
    if output.status.success() {
        return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !command.explicit && formatter_backend_unavailable(&stderr) {
        return Ok(None);
    }

    Err(LspError::Other(format!(
        "TPY6003: formatter `{}` exited with status {}{}",
        command.label,
        output.status,
        formatter_stderr_suffix(stderr.trim())
    )))
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
