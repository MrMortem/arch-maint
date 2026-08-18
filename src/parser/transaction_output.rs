use crate::domain::{HookExecution, HookExecutionStage, HookExecutionStatus};

pub fn parse_hook_executions(stdout: &str, stderr: &str) -> Vec<HookExecution> {
    let mut stage = None;
    let mut hooks: Vec<HookExecution> = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let trimmed = line.trim();
        if trimmed.contains("Running pre-transaction hooks") {
            stage = Some(HookExecutionStage::PreTransaction);
            continue;
        }
        if trimmed.contains("Running post-transaction hooks") {
            stage = Some(HookExecutionStage::PostTransaction);
            continue;
        }
        if trimmed.starts_with(":: Processing package changes")
            || trimmed.starts_with(":: Retrieving packages")
            || trimmed.starts_with(":: Synchronizing package databases")
        {
            stage = None;
            continue;
        }
        let Some(current_stage) = stage else {
            continue;
        };
        if let Some(description) = numbered_hook_description(trimmed) {
            hooks.push(HookExecution {
                description: description.to_owned(),
                stage: current_stage,
                status: HookExecutionStatus::Succeeded,
                output: Vec::new(),
            });
        } else if let Some(hook) = hooks.last_mut() {
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.to_ascii_lowercase().contains("error:")
                || trimmed.contains("command failed to execute correctly")
            {
                hook.status = HookExecutionStatus::Failed;
            }
            hook.output.push(trimmed.to_owned());
        }
    }
    hooks
}

fn numbered_hook_description(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('(')?;
    let (counter, description) = rest.split_once(") ")?;
    let (current, total) = counter.split_once('/')?;
    if current.parse::<usize>().is_err() || total.parse::<usize>().is_err() {
        return None;
    }
    Some(description.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_hook_stage_output_and_failure_without_inventing_names() {
        let output = ":: Running post-transaction hooks...\n(1/2) Reloading system manager configuration...\n(2/2) Building DKMS modules...\n==> dkms output\nerror: command failed to execute correctly\n";
        let hooks = parse_hook_executions(output, "");
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0].status, HookExecutionStatus::Succeeded);
        assert_eq!(hooks[1].status, HookExecutionStatus::Failed);
        assert_eq!(hooks[1].stage, HookExecutionStage::PostTransaction);
        assert!(hooks[1].output.iter().any(|line| line.contains("dkms")));
    }
}
