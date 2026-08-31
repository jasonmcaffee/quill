//! Launching Claude or Codex on a ticket, and resuming the conversation later.
//!
//! No process is started here and nothing about this machine is read, so every command line below is a
//! test with no terminal. `services::agent_tasks::mod` is what spawns what this builds, through
//! `quill_terminal::Session`, which is the same session and the same emulator the terminal tile uses.
//!
//! ## Which agents exist is a list in Quill, and which one a ticket uses is data on the row
//!
//! The fourth time that division is made, after the renderers, the project detectors and the debuggers.
//! [`AGENTS`] is what a ticket's `assignee` is checked against, and the code that knows how to invoke
//! one is here. A ticket naming an agent this version cannot launch says so rather than starting
//! nothing.
//!
//! ## Resuming is the conversation, not the process
//!
//! The board being replaced runs a daemon in a separate process so that a terminal outlives an API
//! restart. A plugin inside the window cannot have that, and it does not need it: both agents resume a
//! conversation by id, so what has to survive is the id. It is on the ticket, in the database, which is
//! also why another Quill window can resume a session this one started.

use super::model::Assignee;

/// The agents this version of Quill can launch.
///
/// Checked the way `plugins::DEBUGGERS` is checked, and for the same reason: a ticket naming something
/// that cannot be launched should say so plainly rather than open a terminal that sits at a shell.
pub const AGENTS: &[&str] = &["claude", "codex"];

/// The models each agent can be asked for, which is what the `Model` dropdown offers.
///
/// `task-28`: "I'm unable to get an agent to do work because the model is a text field." A model identifier
/// is a closed list that changes rarely and cannot be guessed, so it belongs in a list here beside the code
/// that knows how to invoke each agent — the fifth time that division is made, after the renderers, the
/// project detectors, the debuggers and [`AGENTS`].
///
/// **This is not a validation list.** A ticket whose `model` column holds something not here keeps it and
/// draws it as the chosen one, because a board written by hand or by a later Quill must not silently lose a
/// model. See [`models_for`].
///
/// The names are the ones this machine's own configuration uses: `~/.zshrc` exports
/// `ANTHROPIC_MODEL='claude-opus-5[1m]'`, and `~/.codex/config.toml` names its Iliad models.
pub const MODELS: &[(&str, &[&str])] = &[
    (
        "claude",
        &[
            "claude-opus-5[1m]",
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-fable-5",
            "claude-haiku-4-5-20251001",
        ],
    ),
    ("codex", &["gpt-5.6-sol", "gpt-5.6-terra"]),
];

/// The models to offer for `agent`, with `holding` kept in the list wherever it is.
///
/// `holding` is what the ticket's row already says. A value that is not one of [`MODELS`]' own is added at the
/// front rather than dropped, so opening a ticket in a dropdown cannot quietly change which model it names —
/// which would be the worst possible outcome of replacing a text field with a list.
pub fn models_for(agent: Assignee, holding: Option<&str>) -> Vec<String> {
    let known = MODELS
        .iter()
        .find(|(name, _)| *name == agent.name())
        .map(|(_, models)| *models)
        .unwrap_or(&[]);
    let mut offered: Vec<String> = Vec::new();
    if let Some(holding) = holding.map(str::trim).filter(|value| !value.is_empty()) {
        if !known.contains(&holding) {
            offered.push(holding.to_owned());
        }
    }
    offered.extend(known.iter().map(|model| (*model).to_owned()));
    offered
}

/// How long after starting Claude its prompt is ready for input.
///
/// Measured on the board being replaced, which waits exactly this long before typing the handoff line.
/// Typing sooner loses the line: the command line interface has not drawn its prompt and the characters
/// go nowhere.
pub const CLAUDE_READY_MS: u64 = 1800;

/// How long after starting Codex its prompt is ready for input.
pub const CODEX_READY_MS: u64 = 3000;

/// How long after starting `agent` its prompt is ready for input.
///
/// Codex takes longer than Claude, because it prints its banner and its model line first. Reading its
/// screen for a prompt marker instead would be the better signal and is not the one used: a marker read
/// out of a character grid is a marker a colour scheme or a narrow terminal can move. A delay is wrong in
/// one direction only, and everything the window sends waits in one queue behind it, so a slow start
/// delays the handoff rather than losing it.
pub fn ready_after(agent: Assignee) -> std::time::Duration {
    std::time::Duration::from_millis(match agent {
        Assignee::Codex => CODEX_READY_MS,
        _ => CLAUDE_READY_MS,
    })
}

/// A command line to run: the program and its arguments, already split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub arguments: Vec<String>,
}

impl Launch {
    /// What a person reads in the terminal's header and what a test asserts against.
    pub fn line(&self) -> String {
        let mut said = self.program.clone();
        for argument in &self.arguments {
            said.push(' ');
            said.push_str(argument);
        }
        said
    }
}

/// What a ticket is launched with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The ticket's assignee, which decides everything below.
    pub agent: Assignee,
    /// The conversation this run is, which is `--session-id` on a first run and `--resume` on a later
    /// one.
    pub session: String,
    pub model: Option<String>,
    /// `low`, `medium`, `high`, `xhigh` or `max`.
    pub effort: Option<String>,
    /// True when this is a session that already exists.
    pub resuming: bool,
}

/// The command line for `plan`, or a sentence when the ticket names something that cannot be launched.
pub fn launch(plan: &Plan) -> Result<Launch, String> {
    match plan.agent {
        Assignee::Claude => Ok(claude(plan)),
        Assignee::Codex => Ok(codex(plan)),
        Assignee::Human => Err(
            "this ticket is assigned to a person, and a person is not launched in a terminal".to_owned(),
        ),
    }
}

/// Claude's own invocation.
///
/// `--dangerously-skip-permissions` is what the board being replaced passes, because an agent working a
/// ticket in a terminal nobody is watching cannot answer a permission prompt. `--effort` takes the same
/// five levels the board offers, so the level passes through unchanged.
fn claude(plan: &Plan) -> Launch {
    let mut arguments = vec!["--dangerously-skip-permissions".to_owned()];
    if let Some(model) = &plan.model {
        arguments.push("--model".to_owned());
        arguments.push(model.clone());
    }
    if let Some(effort) = &plan.effort {
        arguments.push("--effort".to_owned());
        arguments.push(effort.clone());
    }
    arguments.push(match plan.resuming {
        true => "--resume".to_owned(),
        false => "--session-id".to_owned(),
    });
    arguments.push(plan.session.clone());
    Launch { program: "claude".to_owned(), arguments }
}

/// Codex's own invocation.
///
/// Its reasoning effort is a configuration value rather than a flag, and it recognises nothing above
/// `high`, so `xhigh` and `max` collapse onto `high` rather than being passed through and refused. That
/// is the board being replaced's own rule, and it is why [`codex_effort`] exists rather than the value
/// being handed over as it stands.
///
/// **Codex has no way to be told what to call a new session, and Claude does.** `codex --help` offers no
/// `--session-id`; a session gets an id from Codex and `codex resume <id>` takes it back. So a first run
/// names nothing and [`can_resume`] says that a Codex ticket cannot be resumed by an id Quill chose. The
/// board being replaced solves this by reading the rollout id Codex wrote after the launch, and doing the
/// same here is its own piece of work — §10 of `tasks/agent-tasks-plugin-tdd.md` records it.
///
/// `resume` is a subcommand, so it comes **first**, before the flags. `codex --model x resume <id>` is
/// accepted by clap as well, but a command line a person reads should be in the order its own help
/// writes it.
fn codex(plan: &Plan) -> Launch {
    let mut arguments = Vec::new();
    if plan.resuming {
        arguments.push("resume".to_owned());
        arguments.push(plan.session.clone());
    }
    if let Some(model) = &plan.model {
        arguments.push("--model".to_owned());
        arguments.push(model.clone());
    }
    if let Some(effort) = &plan.effort {
        arguments.push("-c".to_owned());
        arguments.push(format!("model_reasoning_effort={}", codex_effort(effort)));
    }
    Launch { program: "codex".to_owned(), arguments }
}

/// Whether a ticket's recorded session can be handed back to its agent.
///
/// **True for Claude and false for Codex**, and the difference is not a preference: Claude takes
/// `--session-id <uuid>` on a first run, so the id in the ticket is one Claude will answer to, and Codex
/// assigns its own, so the id in the ticket is Quill's own marker and means nothing to Codex.
///
/// The marker is still written for a Codex ticket, because the watchdog's whole question is whether a card
/// has a worker at all. What it cannot do is name a conversation, so `Resume session` on a Codex ticket
/// refuses with a sentence rather than starting a fresh agent that looks like a resumed one.
pub fn can_resume(agent: Assignee) -> bool {
    matches!(agent, Assignee::Claude)
}

/// Why a Codex ticket cannot be resumed, in the words a person reads in the status bar.
pub fn why_it_cannot_resume(key: &str) -> String {
    format!(
        "{key} is Codex's, and Codex names its own sessions rather than taking one, so the id on the ticket \
         is only Quill's marker that a worker was here. Starting it again begins a new conversation: press \
         Start rather than Resume session, and the ticket's comments are what the new one reads."
    )
}

/// The effort level Codex understands.
///
/// Anything above `high` collapses onto it. Passing `max` through would be passing a value Codex
/// rejects, and the ticket would open a terminal holding an error.
pub fn codex_effort(effort: &str) -> &str {
    match effort {
        "xhigh" | "max" => "high",
        other => other,
    }
}

/// The line typed into a fresh agent to hand it the ticket.
///
/// It is the skill the machine already has installed, so the agent reads the task protocol before it
/// touches anything, which is what makes a handoff a handoff rather than a paste of the description.
pub fn handoff(agent: Assignee, key: &str) -> String {
    match agent {
        Assignee::Claude => format!("/task begin {key}"),
        Assignee::Codex => format!(
            "Read docs/agent-task-protocol.md and work {key} on the board through its complete workflow."
        ),
        Assignee::Human => String::new(),
    }
}

/// The line typed into an agent whose conversation has just been handed back.
///
/// The task protocol requires a resumed run to re-read the ticket and take its newest human comments as the
/// specification, so a resumed agent is told that rather than being left to carry on from whatever it last
/// remembered. That is the difference between resuming a conversation and resuming the work.
pub fn resumed(agent: Assignee, key: &str) -> String {
    match agent {
        Assignee::Human => String::new(),
        _ => format!(
            "This session has been resumed on {key}. Re-read the ticket before doing anything, and take its \
             newest human comments as the specification. Then finish the open todos."
        ),
    }
}

/// The line typed into an agent when a comment is sent to it from the ticket.
pub fn comment_handoff(key: &str, body: &str) -> String {
    format!("A new comment was posted on {key}. It is the specification now. It says: {body}")
}

#[cfg(test)]
mod tests_task_28 {
    use super::*;

    /// Every agent that can be launched has a list of models to offer, or its dropdown would be empty.
    #[test]
    fn every_agent_has_models_to_offer() {
        for name in AGENTS {
            let agent = Assignee::parse(name).expect("an agent");
            assert!(!models_for(agent, None).is_empty(), "{name} has models to offer");
        }
        assert!(
            models_for(Assignee::Human, None).is_empty(),
            "a person is not launched, so there is no model to choose"
        );
    }

    /// A model the list has never heard of is kept and offered, so a dropdown cannot silently change what a
    /// ticket says. This is the one rule that makes replacing a text field with a list safe.
    #[test]
    fn a_model_the_list_does_not_know_is_kept_rather_than_dropped() {
        let offered = models_for(Assignee::Claude, Some("something-new-9"));
        assert_eq!(offered.first().map(String::as_str), Some("something-new-9"), "{offered:?}");
        assert!(offered.iter().any(|model| model == "claude-opus-5"), "and the known ones are still there");

        // A value that is already known is not listed twice.
        let again = models_for(Assignee::Claude, Some("claude-opus-5"));
        assert_eq!(
            again.iter().filter(|model| *model == "claude-opus-5").count(),
            1,
            "no duplicate: {again:?}"
        );
        // Nothing chosen adds nothing.
        assert_eq!(models_for(Assignee::Claude, Some("  ")), models_for(Assignee::Claude, None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(agent: Assignee) -> Plan {
        Plan {
            agent,
            session: "0f9a-session".to_owned(),
            model: None,
            effort: None,
            resuming: false,
        }
    }

    #[test]
    fn a_codex_ticket_that_has_stopped_can_only_be_started_again() {
        // What the ticket's buttons are chosen from. Codex names its own sessions, so a Codex ticket carrying an
        // id has nothing to resume: `Resume session` refuses it and tells the person to press Start. The card and
        // the modal used to read only whether an id was there, so such a ticket offered `Resume session` alone and
        // that button could only fail. Both now ask this question instead.
        assert!(!can_resume(Assignee::Codex), "Codex cannot be handed a session id");
        assert!(can_resume(Assignee::Claude), "Claude can");
        let refusal = why_it_cannot_resume("task-7");
        assert!(refusal.contains("press Start"), "the refusal names Start: {refusal}");
    }

    #[test]
    fn claude_is_started_with_a_session_id_and_resumed_with_the_same_one() {
        let mut asked = plan(Assignee::Claude);
        let first = launch(&asked).expect("claude");
        assert_eq!(first.program, "claude");
        assert_eq!(
            first.arguments,
            ["--dangerously-skip-permissions", "--session-id", "0f9a-session"],
            "a first run names the session it is creating"
        );
        asked.resuming = true;
        let again = launch(&asked).expect("claude");
        assert_eq!(
            again.arguments,
            ["--dangerously-skip-permissions", "--resume", "0f9a-session"],
            "a later run resumes the same conversation, which is what Resume session means"
        );
    }

    #[test]
    fn claude_takes_the_model_and_the_effort_the_ticket_names() {
        let mut asked = plan(Assignee::Claude);
        asked.model = Some("claude-opus-5".to_owned());
        asked.effort = Some("max".to_owned());
        let launched = launch(&asked).expect("claude");
        assert_eq!(
            launched.line(),
            "claude --dangerously-skip-permissions --model claude-opus-5 --effort max --session-id 0f9a-session",
            "Claude takes all five levels, so max passes through unchanged"
        );
    }

    #[test]
    fn codex_takes_its_effort_as_a_configuration_value_and_knows_nothing_above_high() {
        let mut asked = plan(Assignee::Codex);
        asked.model = Some("gpt-5.3-codex".to_owned());
        asked.effort = Some("max".to_owned());
        let launched = launch(&asked).expect("codex");
        assert_eq!(
            launched.line(),
            "codex --model gpt-5.3-codex -c model_reasoning_effort=high",
            "max collapses onto high, because passing max would be passing a value Codex rejects"
        );
        assert_eq!(codex_effort("xhigh"), "high");
        assert_eq!(codex_effort("high"), "high");
        assert_eq!(codex_effort("medium"), "medium", "the levels it does know pass through");
        assert_eq!(codex_effort("low"), "low");
    }

    #[test]
    fn a_new_codex_session_names_nothing_because_codex_names_its_own() {
        // `codex --help` offers no `--session-id`. A first run that passed one would be a first run that
        // failed to start, and passing Quill's own id to `codex resume` would be naming a conversation
        // Codex has never heard of.
        let launched = launch(&plan(Assignee::Codex)).expect("codex");
        assert_eq!(launched.line(), "codex", "no session argument at all on a first run");
        assert!(!launched.arguments.iter().any(|argument| argument.contains("session")));
        let mut resuming = plan(Assignee::Codex);
        resuming.resuming = true;
        let again = launch(&resuming).expect("codex");
        assert_eq!(
            again.line(),
            "codex resume 0f9a-session",
            "`resume` is a subcommand, so it comes first, which is the order its own help writes"
        );
    }

    #[test]
    fn only_claude_can_be_handed_its_conversation_back() {
        // Claude takes `--session-id`, so the id on the ticket is one it will answer to. Codex assigns its
        // own, so the id on the ticket is Quill's marker that a worker was here and nothing more.
        assert!(can_resume(Assignee::Claude));
        assert!(!can_resume(Assignee::Codex));
        assert!(!can_resume(Assignee::Human));
        let said = why_it_cannot_resume("task-27");
        assert!(said.contains("task-27"));
        assert!(said.contains("names its own sessions"), "{said}");
        assert!(said.contains("Start"), "it says what to press instead: {said}");
    }

    #[test]
    fn a_ticket_assigned_to_a_person_launches_nothing_and_says_why() {
        let problem = launch(&plan(Assignee::Human)).expect_err("a person is not launched");
        assert!(problem.contains("assigned to a person"), "{problem}");
    }

    #[test]
    fn the_handoff_line_is_the_skill_the_machine_already_has() {
        assert_eq!(handoff(Assignee::Claude, "task-27"), "/task begin task-27");
        let codex = handoff(Assignee::Codex, "task-27");
        assert!(codex.contains("docs/agent-task-protocol.md"), "{codex}");
        assert!(codex.contains("task-27"));
        assert!(handoff(Assignee::Human, "task-27").is_empty());
    }

    #[test]
    fn a_comment_sent_to_an_agent_says_it_is_the_specification_now() {
        // The task protocol says the newest human comments are authoritative on a resumed run, so the
        // line that carries one says so rather than leaving the agent to guess its standing.
        let said = comment_handoff("task-27", "Use the dark palette.");
        assert!(said.contains("task-27"));
        assert!(said.contains("specification"));
        assert!(said.contains("Use the dark palette."));
    }

    #[test]
    fn the_two_agents_in_the_registry_are_the_two_that_can_be_launched() {
        assert_eq!(AGENTS, ["claude", "codex"]);
        for name in AGENTS {
            let assignee = Assignee::parse(name).unwrap_or_else(|| panic!("{name} is not an assignee"));
            assert!(assignee.is_an_agent());
            assert!(launch(&plan(assignee)).is_ok(), "{name} should be launchable");
        }
    }
}
