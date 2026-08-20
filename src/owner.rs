use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::bus::{AppEvent, Cmd, CtlEvent, PermissionAskReply};
use crate::controller::Controller;
use crate::elicitation::ElicitationReply;
use crate::events::UiEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerConfig {
    pub startup_commands: Vec<String>,
    pub shutdown_commands: Vec<String>,
}

impl OwnerConfig {
    pub fn validate(&self) -> Result<()> {
        for command in self
            .startup_commands
            .iter()
            .chain(self.shutdown_commands.iter())
        {
            command_name(command)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerInput {
    SessionBound(String),
    Skills(Vec<String>),
    TurnEnded { session_id: String, kind: String },
    StopRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerAction {
    FetchSkills,
    Prompt { session_id: String, text: String },
    Shutdown,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Starting,
    Running,
    Stopping,
    Done,
}

pub struct OwnerMachine {
    config: OwnerConfig,
    phase: Phase,
    session_id: Option<String>,
    advertised: BTreeSet<String>,
    next_command: usize,
    in_flight: Option<String>,
    stop_requested: bool,
}

impl OwnerMachine {
    pub fn new(config: OwnerConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            phase: Phase::Starting,
            session_id: None,
            advertised: BTreeSet::new(),
            next_command: 0,
            in_flight: None,
            stop_requested: false,
        })
    }

    pub fn on_input(&mut self, input: OwnerInput) -> Result<Vec<OwnerAction>> {
        match input {
            OwnerInput::SessionBound(session_id) => {
                if let Some(bound) = &self.session_id {
                    if bound != &session_id {
                        bail!("ACP owner session changed from {bound} to {session_id}");
                    }
                }
                self.session_id = Some(session_id);
                if self.config.startup_commands.is_empty() {
                    self.phase = Phase::Running;
                    return Ok(Vec::new());
                }
                Ok(vec![OwnerAction::FetchSkills])
            }
            OwnerInput::Skills(skills) => {
                self.advertised = skills.into_iter().collect();
                self.dispatch_next()
            }
            OwnerInput::TurnEnded { session_id, kind } => {
                let Some(command) = self.in_flight.take() else {
                    return Ok(Vec::new());
                };
                if self.session_id.as_deref() != Some(session_id.as_str()) {
                    bail!(
                        "ACP owner command {command} completed for unexpected session {session_id}"
                    );
                }
                if kind != "completed" {
                    bail!("ACP owner command {command} ended with {kind}");
                }
                self.next_command += 1;
                match self.phase {
                    Phase::Starting => {
                        if self.next_command >= self.config.startup_commands.len() {
                            self.next_command = 0;
                            if self.stop_requested {
                                self.phase = Phase::Stopping;
                                if self.config.shutdown_commands.is_empty() {
                                    self.phase = Phase::Done;
                                    return Ok(vec![OwnerAction::Shutdown, OwnerAction::Exit]);
                                }
                                return Ok(vec![OwnerAction::FetchSkills]);
                            }
                            self.phase = Phase::Running;
                            return Ok(Vec::new());
                        }
                    }
                    Phase::Stopping => {
                        if self.next_command >= self.config.shutdown_commands.len() {
                            self.phase = Phase::Done;
                            return Ok(vec![OwnerAction::Shutdown, OwnerAction::Exit]);
                        }
                    }
                    Phase::Running | Phase::Done => {
                        bail!("ACP owner completed {command} outside a lifecycle command phase")
                    }
                }
                self.dispatch_next()
            }
            OwnerInput::StopRequested => {
                if self.phase == Phase::Done {
                    return Ok(Vec::new());
                }
                self.stop_requested = true;
                if self.in_flight.is_some() {
                    return Ok(Vec::new());
                }
                self.phase = Phase::Stopping;
                self.next_command = 0;
                if self.config.shutdown_commands.is_empty() {
                    self.phase = Phase::Done;
                    return Ok(vec![OwnerAction::Shutdown, OwnerAction::Exit]);
                }
                Ok(vec![OwnerAction::FetchSkills])
            }
        }
    }

    fn dispatch_next(&mut self) -> Result<Vec<OwnerAction>> {
        if self.in_flight.is_some() || self.session_id.is_none() {
            return Ok(Vec::new());
        }
        let commands = match self.phase {
            Phase::Starting => &self.config.startup_commands,
            Phase::Stopping => &self.config.shutdown_commands,
            Phase::Running | Phase::Done => return Ok(Vec::new()),
        };
        let Some(command) = commands.get(self.next_command) else {
            return Ok(Vec::new());
        };
        let name = command_name(command)?;
        if !self.advertised.contains(name) {
            return Ok(Vec::new());
        }
        let session_id = self.session_id.clone().expect("checked above");
        self.in_flight = Some(command.clone());
        Ok(vec![OwnerAction::Prompt {
            session_id,
            text: command.clone(),
        }])
    }

    fn waiting_for_progress(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Stopping)
    }

    fn wait_description(&self) -> String {
        if self.session_id.is_none() {
            return "ACP session".into();
        }
        if let Some(command) = &self.in_flight {
            return format!("completion of {command}");
        }
        let command = match self.phase {
            Phase::Starting => self.config.startup_commands.get(self.next_command),
            Phase::Stopping => self.config.shutdown_commands.get(self.next_command),
            Phase::Running | Phase::Done => None,
        };
        command
            .map(|command| format!("ACP advertisement for {command}"))
            .unwrap_or_else(|| "owner lifecycle progress".into())
    }
}

fn command_name(command: &str) -> Result<&str> {
    let Some(rest) = command.strip_prefix('/') else {
        bail!("owner lifecycle command must start with '/': {command}");
    };
    let Some(name) = rest
        .split_whitespace()
        .next()
        .filter(|name| !name.is_empty())
    else {
        bail!("owner lifecycle command is empty");
    };
    Ok(name)
}

pub fn run_owner(
    config: OwnerConfig,
    bus_rx: Receiver<AppEvent>,
    stop_rx: Receiver<()>,
    controller: &Controller,
    command_timeout: Duration,
) -> Result<()> {
    let result = run_owner_inner(config, bus_rx, stop_rx, controller, command_timeout);
    if result.is_err() {
        controller.send(Cmd::Shutdown);
    }
    result
}

fn run_owner_inner(
    config: OwnerConfig,
    bus_rx: Receiver<AppEvent>,
    stop_rx: Receiver<()>,
    controller: &Controller,
    command_timeout: Duration,
) -> Result<()> {
    let mut machine = OwnerMachine::new(config)?;
    let mut deadline = Some(Instant::now() + command_timeout);
    let mut stop_seen = false;

    loop {
        if !stop_seen {
            match stop_rx.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => {
                    stop_seen = true;
                    let actions = machine.on_input(OwnerInput::StopRequested)?;
                    if apply_actions(actions, controller) {
                        return Ok(());
                    }
                    deadline = machine
                        .waiting_for_progress()
                        .then(|| Instant::now() + command_timeout);
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        if let Some(until) = deadline {
            if Instant::now() >= until {
                bail!("timed out waiting for {}", machine.wait_description());
            }
        }

        let event = match bus_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => bail!("ACP owner event channel closed"),
        };
        let input = match event {
            AppEvent::Ctl(CtlEvent::SessionBound { session_id, .. }) => {
                Some(OwnerInput::SessionBound(session_id))
            }
            AppEvent::Ctl(CtlEvent::Skills { skills }) => Some(OwnerInput::Skills(
                skills.into_iter().map(|skill| skill.name).collect(),
            )),
            AppEvent::Ui(UiEvent::TurnEnd { session, kind }) => Some(OwnerInput::TurnEnded {
                session_id: session,
                kind,
            }),
            AppEvent::PermissionAsk { title, reply, .. } => {
                let _ = reply.send(PermissionAskReply::Cancelled);
                bail!("headless ACP owner cancelled permission request: {title}");
            }
            AppEvent::ElicitationAsk { form, reply } => {
                let _ = reply.send(ElicitationReply::Cancelled);
                bail!(
                    "headless ACP owner cancelled elicitation request: {}",
                    form.message
                );
            }
            AppEvent::Ctl(CtlEvent::Error(error)) => bail!("ACP owner: {error}"),
            AppEvent::Ctl(CtlEvent::OpenAuth) => bail!(
                "ACP owner authentication requires an interactive client; configure credentials before starting headless owner mode"
            ),
            AppEvent::RuntimeStderr(line) => {
                eprintln!("owner runtime: {line}");
                None
            }
            AppEvent::RuntimeExited(code) => bail!("ACP owner runtime exited: {code:?}"),
            _ => None,
        };
        let Some(input) = input else {
            continue;
        };
        let made_progress = !matches!(input, OwnerInput::Skills(_));
        let actions = machine.on_input(input)?;
        let has_actions = !actions.is_empty();
        if apply_actions(actions, controller) {
            return Ok(());
        }
        if made_progress || has_actions {
            deadline = machine
                .waiting_for_progress()
                .then(|| Instant::now() + command_timeout);
        } else if !machine.waiting_for_progress() {
            deadline = None;
        }
    }
}

fn apply_actions(actions: Vec<OwnerAction>, controller: &Controller) -> bool {
    let mut exit = false;
    for action in actions {
        match action {
            OwnerAction::FetchSkills => controller.send(Cmd::FetchSkills),
            OwnerAction::Prompt { session_id, text } => {
                eprintln!("owner: {text}");
                controller.send(Cmd::Prompt { session_id, text });
            }
            OwnerAction::Shutdown => controller.send(Cmd::Shutdown),
            OwnerAction::Exit => exit = true,
        }
    }
    exit
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::bus::{AppEvent, Cmd, CtlEvent, SkillInfo};
    use crate::controller::test_controller;
    use crate::events::UiEvent;

    #[test]
    fn lifecycle_waits_for_advertised_commands_and_stays_alive_until_stop() {
        let mut owner = OwnerMachine::new(OwnerConfig {
            startup_commands: vec!["/telegram-connect".into()],
            shutdown_commands: vec!["/telegram-disconnect".into()],
        })
        .unwrap();

        assert_eq!(
            owner
                .on_input(OwnerInput::SessionBound("session-1".into()))
                .unwrap(),
            vec![OwnerAction::FetchSkills]
        );
        assert_eq!(
            owner
                .on_input(OwnerInput::Skills(vec!["unrelated".into()]))
                .unwrap(),
            Vec::<OwnerAction>::new(),
            "an unadvertised slash command must never fall through as a model prompt"
        );
        assert_eq!(
            owner
                .on_input(OwnerInput::Skills(vec!["telegram-connect".into()]))
                .unwrap(),
            vec![OwnerAction::Prompt {
                session_id: "session-1".into(),
                text: "/telegram-connect".into(),
            }]
        );
        assert_eq!(
            owner
                .on_input(OwnerInput::TurnEnded {
                    session_id: "session-1".into(),
                    kind: "completed".into(),
                })
                .unwrap(),
            Vec::<OwnerAction>::new(),
            "the owner remains alive after startup"
        );
        assert_eq!(
            owner.on_input(OwnerInput::StopRequested).unwrap(),
            vec![OwnerAction::FetchSkills]
        );
        assert_eq!(
            owner
                .on_input(OwnerInput::Skills(vec!["telegram-disconnect".into()]))
                .unwrap(),
            vec![OwnerAction::Prompt {
                session_id: "session-1".into(),
                text: "/telegram-disconnect".into(),
            }]
        );
        assert_eq!(
            owner
                .on_input(OwnerInput::TurnEnded {
                    session_id: "session-1".into(),
                    kind: "completed".into(),
                })
                .unwrap(),
            vec![OwnerAction::Shutdown, OwnerAction::Exit]
        );
    }

    #[test]
    fn channel_owner_drives_the_real_controller_without_a_tui() {
        let (bus_tx, bus_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (controller, command_rx) = test_controller();
        let owner = std::thread::spawn(move || {
            run_owner(
                OwnerConfig {
                    startup_commands: vec!["/telegram-connect".into()],
                    shutdown_commands: vec!["/telegram-disconnect".into()],
                },
                bus_rx,
                stop_rx,
                &controller,
                Duration::from_secs(1),
            )
        });

        bus_tx
            .send(AppEvent::Ctl(CtlEvent::SessionBound {
                session_id: "session-1".into(),
                notice: None,
            }))
            .unwrap();
        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Cmd::FetchSkills
        ));
        bus_tx
            .send(AppEvent::Ctl(CtlEvent::Skills {
                skills: vec![SkillInfo {
                    name: "telegram-connect".into(),
                    description: "connect".into(),
                    config_action: None,
                    client_command: false,
                }],
            }))
            .unwrap();
        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Cmd::Prompt { session_id, text }
                if session_id == "session-1" && text == "/telegram-connect"
        ));
        bus_tx
            .send(AppEvent::Ui(UiEvent::TurnEnd {
                session: "session-1".into(),
                kind: "completed".into(),
            }))
            .unwrap();
        assert!(matches!(
            command_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        stop_tx.send(()).unwrap();
        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Cmd::FetchSkills
        ));
        bus_tx
            .send(AppEvent::Ctl(CtlEvent::Skills {
                skills: vec![SkillInfo {
                    name: "telegram-disconnect".into(),
                    description: "disconnect".into(),
                    config_action: None,
                    client_command: false,
                }],
            }))
            .unwrap();
        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Cmd::Prompt { session_id, text }
                if session_id == "session-1" && text == "/telegram-disconnect"
        ));
        bus_tx
            .send(AppEvent::Ui(UiEvent::TurnEnd {
                session: "session-1".into(),
                kind: "completed".into(),
            }))
            .unwrap();
        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Cmd::Shutdown
        ));
        owner.join().unwrap().unwrap();
    }

    #[test]
    fn headless_owner_cancels_permission_instead_of_auto_approving() {
        let (bus_tx, bus_rx) = mpsc::channel();
        let (_stop_tx, stop_rx) = mpsc::channel();
        let (controller, command_rx) = test_controller();
        let owner = std::thread::spawn(move || {
            run_owner(
                OwnerConfig {
                    startup_commands: Vec::new(),
                    shutdown_commands: Vec::new(),
                },
                bus_rx,
                stop_rx,
                &controller,
                Duration::from_secs(1),
            )
        });
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        bus_tx
            .send(AppEvent::PermissionAsk {
                title: "run external command".into(),
                options: Vec::new(),
                reply: reply_tx,
            })
            .unwrap();

        assert_eq!(
            reply_rx.blocking_recv().unwrap(),
            crate::bus::PermissionAskReply::Cancelled
        );
        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Cmd::Shutdown
        ));
        let error = owner.join().unwrap().unwrap_err();
        assert!(error.to_string().contains("cancelled permission request"));
    }

    #[test]
    fn headless_owner_fails_fast_when_authentication_needs_an_interactive_surface() {
        let (bus_tx, bus_rx) = mpsc::channel();
        let (_stop_tx, stop_rx) = mpsc::channel();
        let (controller, command_rx) = test_controller();
        let owner = std::thread::spawn(move || {
            run_owner(
                OwnerConfig {
                    startup_commands: Vec::new(),
                    shutdown_commands: Vec::new(),
                },
                bus_rx,
                stop_rx,
                &controller,
                Duration::from_secs(1),
            )
        });

        bus_tx.send(AppEvent::Ctl(CtlEvent::OpenAuth)).unwrap();

        assert!(matches!(
            command_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Cmd::Shutdown
        ));
        let error = owner.join().unwrap().unwrap_err();
        assert!(error
            .to_string()
            .contains("authentication requires an interactive client"));
    }
}
