use std::{ffi::OsString, fmt, process::ExitStatus, time::Duration};
use thiserror::Error;
use tokio::{process::Command, time::timeout};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub timeout: Duration,
}

impl CommandSpec {
    pub fn read_only(
        program: impl Into<OsString>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn display(&self) -> String {
        std::iter::once(self.program.to_string_lossy().into_owned())
            .chain(
                self.args
                    .iter()
                    .map(|arg| shell_quote_for_display(&arg.to_string_lossy())),
            )
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn shell_quote_for_display(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-._/:+@".contains(c))
    {
        value.to_owned()
    } else {
        format!("'{escaped}'", escaped = value.replace('\'', "'\\''"))
    }
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Error)]
pub enum CommandFailure {
    #[error("required command `{program}` was not found")]
    Missing { program: String },
    #[error("could not start `{command}`: {source}")]
    Spawn {
        command: String,
        source: std::io::Error,
    },
    #[error("`{command}` timed out after {seconds}s")]
    Timeout { command: String, seconds: u64 },
    #[error("`{command}` failed with {status}: {stderr}")]
    Exit {
        command: String,
        status: String,
        stderr: String,
    },
    #[error("`{command}` returned non-UTF-8 output")]
    InvalidUtf8 { command: String },
}

#[derive(Debug, Clone, Default)]
pub struct CommandRunner;

impl CommandRunner {
    pub async fn run(&self, spec: CommandSpec) -> Result<CommandOutput, CommandFailure> {
        let command_display = spec.display();
        tracing::debug!(command = %command_display, "running read-only command");
        let mut command = Command::new(&spec.program);
        command.args(&spec.args).envs(spec.env).kill_on_drop(true);
        let output = timeout(spec.timeout, command.output())
            .await
            .map_err(|_| CommandFailure::Timeout {
                command: command_display.clone(),
                seconds: spec.timeout.as_secs(),
            })?
            .map_err(|source| {
                if source.kind() == std::io::ErrorKind::NotFound {
                    CommandFailure::Missing {
                        program: spec.program.to_string_lossy().into_owned(),
                    }
                } else {
                    CommandFailure::Spawn {
                        command: command_display.clone(),
                        source,
                    }
                }
            })?;
        let stdout = String::from_utf8(output.stdout).map_err(|_| CommandFailure::InvalidUtf8 {
            command: command_display.clone(),
        })?;
        let stderr = String::from_utf8(output.stderr).map_err(|_| CommandFailure::InvalidUtf8 {
            command: command_display.clone(),
        })?;
        Ok(CommandOutput {
            status: output.status,
            stdout,
            stderr,
        })
    }

    pub async fn run_checked(&self, spec: CommandSpec) -> Result<CommandOutput, CommandFailure> {
        let display = spec.display();
        let output = self.run(spec).await?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(CommandFailure::Exit {
                command: display,
                status: StatusDisplay(output.status).to_string(),
                stderr: concise_stderr(&output.stderr),
            })
        }
    }
}

fn concise_stderr(stderr: &str) -> String {
    let text = stderr.trim();
    if text.is_empty() {
        "no diagnostic output".into()
    } else {
        text.chars().take(500).collect()
    }
}

struct StatusDisplay(ExitStatus);

impl fmt::Display for StatusDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.code() {
            Some(code) => write!(f, "exit status {code}"),
            None => f.write_str("termination by signal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_display_quotes_for_diagnostics_only() {
        let spec = CommandSpec::read_only("pacman", ["-Ss", "two words"]);
        assert_eq!(spec.display(), "pacman -Ss 'two words'");
        assert_eq!(spec.args[1], "two words");
    }
}
