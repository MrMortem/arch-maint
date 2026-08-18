use crate::domain::HookStage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlpmHook {
    pub name: String,
    pub description: String,
    pub stage: HookStage,
    pub command: Option<String>,
    pub triggers: Vec<HookTrigger>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookTrigger {
    pub operations: Vec<String>,
    pub trigger_type: Option<String>,
    pub targets: Vec<String>,
}

pub fn parse_alpm_hook(name: &str, input: &str) -> Option<AlpmHook> {
    enum Section {
        None,
        Trigger(usize),
        Action,
    }

    let mut section = Section::None;
    let mut triggers = Vec::<HookTrigger>::new();
    let mut description = name.trim_end_matches(".hook").replace(['-', '_'], " ");
    let mut stage = HookStage::PostTransaction;
    let mut command = None;

    for raw_line in input.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "[Trigger]" => {
                triggers.push(HookTrigger::default());
                section = Section::Trigger(triggers.len() - 1);
                continue;
            }
            "[Action]" => {
                section = Section::Action;
                continue;
            }
            _ => {}
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().to_owned();
        match section {
            Section::Trigger(index) => match key {
                "Operation" => triggers[index].operations.push(value),
                "Type" => triggers[index].trigger_type = Some(value),
                "Target" => triggers[index].targets.push(value),
                _ => {}
            },
            Section::Action => match key {
                "Description" => description = value,
                "When" if value == "PreTransaction" => stage = HookStage::PreTransaction,
                "When" if value == "PostTransaction" => stage = HookStage::PostTransaction,
                "Exec" => command = Some(value),
                _ => {}
            },
            Section::None => {}
        }
    }
    (!triggers.is_empty()).then(|| AlpmHook {
        name: name.to_owned(),
        description,
        stage,
        command,
        triggers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_triggers_and_action() {
        let input = "[Trigger]\nOperation = Upgrade\nType = Package\nTarget = linux*\n\n[Trigger]\nOperation=Install\nType=Package\nTarget=linux\n\n[Action]\nDescription = Updating module dependencies\nWhen = PostTransaction\nExec = /usr/bin/depmod -a\n";
        let hook = parse_alpm_hook("60-depmod.hook", input).expect("hook");
        assert_eq!(hook.triggers.len(), 2);
        assert_eq!(hook.description, "Updating module dependencies");
        assert_eq!(hook.stage, HookStage::PostTransaction);
    }
}
