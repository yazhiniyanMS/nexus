//! Agent roles: what each one is responsible for, what it is allowed to do, what it is asked,
//! and how its reply is parsed. A role is data plus a permission set, not a process.

use crate::error::{AppError, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Architect,
    Research,
    Developer,
    Testing,
    Review,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    /// Read files and the code index of the authorised project.
    ReadProject,
    /// Write files inside the run's isolated workspace. Never the project itself.
    WriteWorkspace,
    /// Run a command in the workspace, after the user approves that exact command.
    RunCommand,
}

pub const ALL_ROLES: [Role; 5] = [Role::Architect, Role::Research, Role::Developer, Role::Testing, Role::Review];

impl Role {
    pub fn id(self) -> &'static str {
        match self {
            Role::Architect => "architect",
            Role::Research => "research",
            Role::Developer => "developer",
            Role::Testing => "testing",
            Role::Review => "review",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Architect => "Architect",
            Role::Research => "Research",
            Role::Developer => "Developer",
            Role::Testing => "Testing",
            Role::Review => "Review",
        }
    }

    pub fn responsibilities(self) -> &'static [&'static str] {
        match self {
            Role::Architect => &["Analyses project structure", "Identifies architectural issues", "Proposes the implementation plan"],
            Role::Research => {
                &["Searches source files", "Retrieves relevant passages and documentation", "Collects evidence for decisions"]
            }
            Role::Developer => &["Implements the approved plan", "Works only in an isolated workspace", "Reports modified files"],
            Role::Testing => &["Discovers tests", "Runs test commands you approve", "Reports failures"],
            Role::Review => {
                &["Reviews the proposed diff", "Identifies potential regressions", "Checks the change against the original task"]
            }
        }
    }

    pub fn capabilities(self) -> &'static [Capability] {
        match self {
            Role::Architect | Role::Research | Role::Review => &[Capability::ReadProject],
            Role::Developer => &[Capability::ReadProject, Capability::WriteWorkspace],
            Role::Testing => &[Capability::ReadProject, Capability::RunCommand],
        }
    }
}

/// The single permission check. Every tool use in the engine goes through it.
pub fn require(role: Role, capability: Capability) -> Result<()> {
    if role.capabilities().contains(&capability) {
        Ok(())
    } else {
        Err(AppError::Msg(format!(
            "The {} agent is not permitted to {}",
            role.name(),
            match capability {
                Capability::ReadProject => "read the project",
                Capability::WriteWorkspace => "write files",
                Capability::RunCommand => "run commands",
            }
        )))
    }
}

#[derive(Serialize)]
pub struct RoleInfo {
    id: &'static str,
    name: &'static str,
    responsibilities: &'static [&'static str],
    can_read: bool,
    can_write_workspace: bool,
    can_run_commands: bool,
}

pub fn role_infos() -> Vec<RoleInfo> {
    ALL_ROLES
        .iter()
        .map(|r| RoleInfo {
            id: r.id(),
            name: r.name(),
            responsibilities: r.responsibilities(),
            can_read: r.capabilities().contains(&Capability::ReadProject),
            can_write_workspace: r.capabilities().contains(&Capability::WriteWorkspace),
            can_run_commands: r.capabilities().contains(&Capability::RunCommand),
        })
        .collect()
}

// ---------------------------------------------------------------- structured replies

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct PlanStep {
    pub title: String,
    pub detail: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Plan {
    pub analysis: String,
    pub issues: Vec<String>,
    pub steps: Vec<PlanStep>,
    /// Project-relative files the Developer should read and change.
    pub files: Vec<String>,
    /// Suggested test commands. Untrusted: each still needs explicit approval.
    pub test_commands: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct FileChange {
    pub path: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Changes {
    pub summary: String,
    pub changes: Vec<FileChange>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct ReviewIssue {
    pub severity: String,
    pub file: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct ReviewReport {
    /// "approve" or "changes_requested".
    pub verdict: String,
    pub summary: String,
    pub issues: Vec<ReviewIssue>,
}

/// Parse the first JSON object in a model reply, tolerating prose or code fences around it.
pub fn parse_reply<T: DeserializeOwned>(reply: &str) -> Result<T> {
    let (Some(start), Some(end)) = (reply.find('{'), reply.rfind('}')) else {
        return Err("The model did not return a JSON object".into());
    };
    if end < start {
        return Err("The model did not return a JSON object".into());
    }
    serde_json::from_str(&reply[start..=end]).map_err(|e| AppError::Msg(format!("The model's reply was not valid JSON: {e}")))
}

// ---------------------------------------------------------------- prompts

const JSON_ONLY: &str = "Reply with ONLY one JSON object and nothing else.";

pub fn research_prompt(outcome: &str, sources: &str) -> (String, String) {
    (
        format!(
            "You are the Research agent in a software engineering team. You only read; you never change anything. \
                 From the numbered source excerpts, collect the evidence relevant to the task: where the relevant code lives, \
                 how it currently behaves, and anything that constrains the change. Cite excerpts as [n]. Do not invent files. \
                 {JSON_ONLY} Shape: {{\"summary\": string}}"
        ),
        format!("Task: {outcome}\n\nSource excerpts:\n{sources}"),
    )
}

pub fn architect_prompt(outcome: &str, project: &str, structure: &str, research: &str, sources: &str) -> (String, String) {
    let os = std::env::consts::OS;
    let shell = crate::platform::process::shell_invocation().0;
    (
        format!("You are the Architect agent. You analyse structure and plan; you never write code or run commands. \
                 Using only the facts given, report architectural issues relevant to the task and propose a concrete plan. \
                 List the project-relative paths of the files that must be created or changed (at most 12). \
                 Suggest at most 3 non-interactive test commands; they will be shown to a human who must approve each one, \
                 so never suggest destructive commands. Commands run on {os} through `{shell}`, so use only \
                 syntax and tools that exist there. {JSON_ONLY} Shape: {{\"analysis\": string, \"issues\": [string], \
                 \"steps\": [{{\"title\": string, \"detail\": string}}], \"files\": [string], \"test_commands\": [string]}}"),
        format!("Task: {outcome}\n\nProject: {project}\n\nStructure:\n{structure}\n\nResearch findings:\n{research}\n\nSource excerpts:\n{sources}"),
    )
}

pub fn developer_prompt(outcome: &str, plan: &Plan, files: &str) -> (String, String) {
    let steps: String = plan.steps.iter().enumerate().map(|(i, s)| format!("{}. {} — {}\n", i + 1, s.title, s.detail)).collect();
    (
        format!(
            "You are the Developer agent. Implement exactly the approved plan and nothing more. You work in an isolated \
                 copy of the repository; you cannot run commands. Return the COMPLETE new content of every file you create or \
                 change, using project-relative paths. Do not return unchanged files. {JSON_ONLY} \
                 Shape: {{\"summary\": string, \"changes\": [{{\"path\": string, \"content\": string}}]}}"
        ),
        format!("Task: {outcome}\n\nApproved plan:\n{steps}\nCurrent file contents:\n{files}"),
    )
}

pub fn review_prompt(outcome: &str, plan: &Plan, diff: &str, tests: &str) -> (String, String) {
    let steps: String = plan.steps.iter().map(|s| format!("- {}\n", s.title)).collect();
    (
        format!(
            "You are the Review agent. You only read. Review the diff against the original task and the approved plan. \
                 Identify bugs, regressions, missing pieces and anything outside the plan. Consider the test results. \
                 Use verdict \"approve\" only if you would merge this. {JSON_ONLY} Shape: {{\"verdict\": \"approve\" | \
                 \"changes_requested\", \"summary\": string, \"issues\": [{{\"severity\": \"high\" | \"medium\" | \"low\", \
                 \"file\": string, \"note\": string}}]}}"
        ),
        format!("Task: {outcome}\n\nApproved plan:\n{steps}\nTest results:\n{tests}\n\nDiff:\n{diff}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_matrix_is_exact() {
        use Capability::*;
        let expected = [
            (Role::Architect, true, false, false),
            (Role::Research, true, false, false),
            (Role::Developer, true, true, false),
            (Role::Testing, true, false, true),
            (Role::Review, true, false, false),
        ];
        for (role, read, write, run) in expected {
            assert_eq!(require(role, ReadProject).is_ok(), read, "{role:?} read");
            assert_eq!(require(role, WriteWorkspace).is_ok(), write, "{role:?} write");
            assert_eq!(require(role, RunCommand).is_ok(), run, "{role:?} run");
        }
        // No single role can both write files and run commands.
        assert!(ALL_ROLES.iter().all(|r| require(*r, WriteWorkspace).is_err() || require(*r, RunCommand).is_err()));
        assert!(require(Role::Review, WriteWorkspace).unwrap_err().to_string().contains("not permitted"));
    }

    #[test]
    fn parses_replies_wrapped_in_prose_and_fills_defaults() {
        let plan: Plan =
            parse_reply("Here you go:\n```json\n{\"analysis\":\"ok\",\"steps\":[{\"title\":\"Edit\"}],\"files\":[\"a.rs\"]}\n```").unwrap();
        assert_eq!((plan.analysis.as_str(), plan.steps.len(), plan.files.len(), plan.test_commands.len()), ("ok", 1, 1, 0));
        assert!(parse_reply::<Plan>("no json here").is_err());
        assert!(parse_reply::<Changes>("{\"changes\": \"oops\"}").is_err());
    }
}
