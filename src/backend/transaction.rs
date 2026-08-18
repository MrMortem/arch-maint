use super::{HelperKind, TransactionBackend};
use crate::domain::{
    OutputStream, TransactionControl, TransactionEvent, TransactionRequest, TransactionResult,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use std::{ffi::OsString, process::Stdio};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::mpsc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl TransactionCommand {
    pub fn display_parts(&self) -> Vec<String> {
        std::iter::once(self.program.to_string_lossy().into_owned())
            .chain(
                self.args
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned()),
            )
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct PacmanTransactionBackend {
    helper: Option<HelperKind>,
}

impl PacmanTransactionBackend {
    pub fn new(helper: Option<HelperKind>) -> Self {
        Self { helper }
    }

    pub fn command_for(&self, request: &TransactionRequest) -> Result<TransactionCommand> {
        validate_request(request)?;
        let command = match request {
            TransactionRequest::SystemUpgrade => privileged_pacman(["-Syu"]),
            TransactionRequest::OfficialUpdate { package } => privileged(
                "pacman",
                vec![
                    "-Syu".into(),
                    "--needed".into(),
                    "--color".into(),
                    "never".into(),
                    "--".into(),
                    package.clone(),
                ],
            ),
            TransactionRequest::OfficialInstall { packages } => {
                let mut args = vec![
                    "-Syu".into(),
                    "--needed".into(),
                    "--color".into(),
                    "never".into(),
                    "--".into(),
                ];
                args.extend(packages.iter().cloned());
                privileged("pacman", args)
            }
            TransactionRequest::OfficialRemove {
                packages,
                remove_unused,
            } => {
                let operation = if *remove_unused { "-Rs" } else { "-R" };
                let mut args = vec![
                    operation.into(),
                    "--color".into(),
                    "never".into(),
                    "--".into(),
                ];
                args.extend(packages.iter().cloned());
                privileged("pacman", args)
            }
            TransactionRequest::AurInstall { packages } => {
                let helper = self.helper.context("no AUR helper is configured")?;
                let mut args = vec![
                    "-Syu".into(),
                    "--needed".into(),
                    "--color".into(),
                    "never".into(),
                    "--".into(),
                ];
                args.extend(packages.iter().cloned());
                direct(helper.command(), args)
            }
            TransactionRequest::AurRemove { packages } => {
                let helper = self.helper.context("no AUR helper is configured")?;
                let mut args = vec!["-Rs".into(), "--color".into(), "never".into(), "--".into()];
                args.extend(packages.iter().cloned());
                direct(helper.command(), args)
            }
            TransactionRequest::AurUpgrade => {
                let helper = self.helper.context("no AUR helper is configured")?;
                direct(
                    helper.command(),
                    vec!["-Sua".into(), "--color".into(), "never".into()],
                )
            }
        };
        Ok(command)
    }
}

#[async_trait]
impl TransactionBackend for PacmanTransactionBackend {
    fn command_preview(&self, request: &TransactionRequest) -> Result<Vec<String>> {
        Ok(self.command_for(request)?.display_parts())
    }

    async fn execute(
        &self,
        request: TransactionRequest,
        events: mpsc::UnboundedSender<TransactionEvent>,
        mut controls: mpsc::UnboundedReceiver<TransactionControl>,
    ) -> Result<()> {
        let transaction = self.command_for(&request)?;
        let command_parts = transaction.display_parts();
        let mut command = Command::new(&transaction.program);
        command
            .args(&transaction.args)
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start {}", command_parts.join(" ")))?;
        let mut stdin = child
            .stdin
            .take()
            .context("transaction stdin unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("transaction stdout unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("transaction stderr unavailable")?;
        events
            .send(TransactionEvent::Started {
                command: command_parts.clone(),
            })
            .ok();
        let stdout_task = stream_reader(stdout, OutputStream::Stdout, events.clone());
        let stderr_task = stream_reader(stderr, OutputStream::Stderr, events.clone());
        let mut cancelled = false;

        let status = loop {
            tokio::select! {
                status = child.wait() => break status.context("failed to wait for package manager")?,
                Some(control) = controls.recv() => match control {
                    TransactionControl::Input(value) => {
                        stdin.write_all(value.as_bytes()).await.context("failed to answer package-manager prompt")?;
                        stdin.flush().await.context("failed to flush package-manager input")?;
                    }
                    TransactionControl::Cancel => {
                        cancelled = true;
                        cancel_process_group(&mut child).await?;
                    }
                }
            }
        };
        drop(stdin);
        let stdout = stdout_task.await.context("stdout reader task failed")?;
        let stderr = stderr_task.await.context("stderr reader task failed")?;
        let hooks = crate::parser::parse_hook_executions(&stdout, &stderr);
        events
            .send(TransactionEvent::Finished(Box::new(TransactionResult {
                command: command_parts,
                exit_code: status.code(),
                stdout,
                stderr,
                cancelled,
                hooks,
            })))
            .ok();
        Ok(())
    }
}

async fn cancel_process_group(child: &mut Child) -> Result<()> {
    #[cfg(unix)]
    if let Some(process_id) = child.id() {
        let status = Command::new("kill")
            .args(["-INT", "--", &format!("-{process_id}")])
            .status()
            .await
            .context("failed to signal the package-manager process group")?;
        if status.success() {
            return Ok(());
        }
    }
    child
        .start_kill()
        .context("failed to cancel package manager")
}

fn stream_reader<R>(
    mut reader: R,
    stream: OutputStream,
    events: mpsc::UnboundedSender<TransactionEvent>,
) -> tokio::task::JoinHandle<String>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut complete = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => {
                    complete.extend_from_slice(&buffer[..read]);
                    events
                        .send(TransactionEvent::Output {
                            stream,
                            chunk: String::from_utf8_lossy(&buffer[..read]).into_owned(),
                        })
                        .ok();
                }
                Err(error) => {
                    events
                        .send(TransactionEvent::Output {
                            stream: OutputStream::Stderr,
                            chunk: format!("\noutput reader failed: {error}\n"),
                        })
                        .ok();
                    break;
                }
            }
        }
        String::from_utf8_lossy(&complete).into_owned()
    })
}

fn privileged_pacman<const N: usize>(args: [&str; N]) -> TransactionCommand {
    let mut owned = args.into_iter().map(ToOwned::to_owned).collect::<Vec<_>>();
    owned.extend(["--color".into(), "never".into()]);
    privileged("pacman", owned)
}

fn privileged(program: &str, args: Vec<String>) -> TransactionCommand {
    let mut sudo_args = vec![
        OsString::from("-n"),
        OsString::from("--"),
        OsString::from(program),
    ];
    sudo_args.extend(args.into_iter().map(OsString::from));
    TransactionCommand {
        program: OsString::from("sudo"),
        args: sudo_args,
    }
}

fn direct(program: &str, args: Vec<String>) -> TransactionCommand {
    TransactionCommand {
        program: OsString::from(program),
        args: args.into_iter().map(OsString::from).collect(),
    }
}

fn validate_request(request: &TransactionRequest) -> Result<()> {
    if let TransactionRequest::OfficialUpdate { package } = request {
        validate_transaction_package_name(package)?;
        return Ok(());
    }
    let packages = match request {
        TransactionRequest::OfficialInstall { packages }
        | TransactionRequest::OfficialRemove { packages, .. }
        | TransactionRequest::AurInstall { packages }
        | TransactionRequest::AurRemove { packages } => Some(packages),
        TransactionRequest::SystemUpgrade
        | TransactionRequest::OfficialUpdate { .. }
        | TransactionRequest::AurUpgrade => None,
    };
    if let Some(packages) = packages {
        if packages.is_empty() {
            bail!("at least one package is required");
        }
        for package in packages {
            validate_transaction_package_name(package)?;
        }
    }
    Ok(())
}

fn validate_transaction_package_name(package: &str) -> Result<()> {
    if package.is_empty()
        || package.len() > 255
        || package.starts_with('-')
        || !package
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "@._+:-".contains(character))
    {
        bail!("invalid package name `{package}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_upgrade_is_interactive_and_privilege_scoped() {
        let backend = PacmanTransactionBackend::new(None);
        let command = backend
            .command_for(&TransactionRequest::SystemUpgrade)
            .expect("command");
        assert_eq!(
            command.display_parts(),
            ["sudo", "-n", "--", "pacman", "-Syu", "--color", "never"]
        );
        assert!(
            !command
                .display_parts()
                .iter()
                .any(|arg| arg == "--noconfirm")
        );
    }

    #[test]
    fn package_names_are_separated_from_options() {
        let backend = PacmanTransactionBackend::new(None);
        let command = backend
            .command_for(&TransactionRequest::OfficialInstall {
                packages: vec!["firefox".into(), "git".into()],
            })
            .expect("command")
            .display_parts();
        assert_eq!(&command[command.len() - 3..], ["--", "firefox", "git"]);
        assert!(
            backend
                .command_for(&TransactionRequest::OfficialInstall {
                    packages: vec!["--overwrite".into()],
                })
                .is_err()
        );
    }

    #[test]
    fn selected_official_update_still_performs_full_system_sync() {
        let backend = PacmanTransactionBackend::new(None);
        let command = backend
            .command_for(&TransactionRequest::OfficialUpdate {
                package: "linux".into(),
            })
            .expect("command")
            .display_parts();
        assert_eq!(
            command,
            [
                "sudo", "-n", "--", "pacman", "-Syu", "--needed", "--color", "never", "--", "linux"
            ]
        );
    }
}
