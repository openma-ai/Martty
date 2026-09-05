//! Slow ACP control requests, serialized within their owning lane.

use super::*;

pub(super) type SetupResult = Result<(SessionId, Value, Option<String>), AcpError>;

pub(super) enum ControlFinish {
    SessionOperationDone {
        session_id: String,
    },
    Setup {
        result: SetupResult,
    },
    Authenticated {
        method: AuthMethodInfo,
        result: Result<(), AcpError>,
    },
}

type ControlJob = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

#[derive(Default)]
pub(super) struct ControlWorkers {
    pending: HashMap<String, usize>,
    lanes: HashMap<String, tokio::sync::mpsc::UnboundedSender<ControlJob>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for ControlWorkers {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl ControlWorkers {
    pub(super) fn busy(&self, session: &str) -> bool {
        self.pending.get(session).copied().unwrap_or(0) > 0
    }

    pub(super) fn settled(&mut self, session: &str) {
        if let Some(count) = self.pending.get_mut(session) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.pending.remove(session);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn enqueue(
        &mut self,
        cmd: Cmd,
        cx: &ConnectionTo<Agent>,
        bus: &Sender<AppEvent>,
        surface: &Arc<Mutex<Surface>>,
        methods: &[AuthMethodInfo],
        selected: &Option<AuthMethodInfo>,
        cwd: &std::path::Path,
        load_session: bool,
        resume_session: bool,
        list_session: bool,
        done: &tokio::sync::mpsc::UnboundedSender<ControlFinish>,
    ) {
        // Preserve setup FIFO and per-session mutation order without blocking
        // prompts, cancellations, or other sessions' control requests.
        let lane = match &cmd {
            Cmd::NewSession | Cmd::ResumeSession { .. } => "setup".to_string(),
            Cmd::SelectModel { session_id, .. }
            | Cmd::SetPermission { session_id, .. }
            | Cmd::SetPreset { session_id, .. }
            | Cmd::SetConfigOption { session_id, .. } => format!("config:{session_id}"),
            Cmd::Authenticate { .. } => "auth".to_string(),
            _ => "client".to_string(),
        };
        let session = lane.strip_prefix("config:").map(str::to_string);
        if let Some(session) = &session {
            *self.pending.entry(session.clone()).or_default() += 1;
        }
        let sender = self.lanes.entry(lane).or_insert_with(|| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ControlJob>();
            self.tasks.push(tokio::spawn(async move {
                while let Some(job) = rx.recv().await {
                    job.await;
                }
            }));
            tx
        });
        let job = run_control(
            cmd,
            cx.clone(),
            bus.clone(),
            Arc::clone(surface),
            methods.to_vec(),
            selected.clone(),
            cwd.to_path_buf(),
            load_session,
            resume_session,
            list_session,
            done.clone(),
        );
        let done = done.clone();
        let _ = sender.send(Box::pin(async move {
            job.await;
            if let Some(session_id) = session {
                let _ = done.send(ControlFinish::SessionOperationDone { session_id });
            }
        }));
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_control(
    cmd: Cmd,
    cx: ConnectionTo<Agent>,
    bus: Sender<AppEvent>,
    surface: Arc<Mutex<Surface>>,
    methods: Vec<AuthMethodInfo>,
    selected: Option<AuthMethodInfo>,
    cwd: std::path::PathBuf,
    load_session: bool,
    resume_session: bool,
    list_session: bool,
    done: tokio::sync::mpsc::UnboundedSender<ControlFinish>,
) {
    match cmd {
        Cmd::SelectModel {
            session_id: cmd_session,
            model,
            effort,
            ..
        } => {
            let sid = SessionId::new(cmd_session);
            if let Some(model) = model {
                let value = {
                    let surface = surface.lock().unwrap_or_else(|e| e.into_inner());
                    match surface
                        .session(&sid.0)
                        .models
                        .iter()
                        .find(|m| m.id == model)
                    {
                        Some(m) if !m.provider.is_empty() => {
                            format!("{}/{}", m.provider, model)
                        }
                        _ => model.clone(),
                    }
                };
                let _ = cx
                    .send_request(SetSessionConfigOptionRequest::new(
                        sid.clone(),
                        "model",
                        SessionConfigOptionValue::value_id(value),
                    ))
                    .block_task_deadline()
                    .await
                    .map_err(|err| {
                        if is_auth_required_error(&err) {
                            emit_needs_auth_open(
                                &bus,
                                methods.clone(),
                                selected.as_ref(),
                                Some(acp_error_message(&err)),
                            );
                        }
                    });
            }
            if let Some(effort) = effort {
                let _ = cx
                    .send_request(SetSessionConfigOptionRequest::new(
                        sid,
                        "effort",
                        SessionConfigOptionValue::value_id(effort),
                    ))
                    .block_task_deadline()
                    .await
                    .map_err(|err| {
                        if is_auth_required_error(&err) {
                            emit_needs_auth_open(
                                &bus,
                                methods.clone(),
                                selected.as_ref(),
                                Some(acp_error_message(&err)),
                            );
                        }
                    });
            }
        }
        Cmd::FetchStaticPlugins => match fetch_static_plugins(&cx).await {
            Ok(plugins) => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::StaticPlugins { plugins }));
            }
            Err(error) => {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                    "static plugins unavailable: {error}"
                ))));
            }
        },
        Cmd::FetchCordisPlugins { agent_id } => {
            if !ensure_agent_cordis(&surface, &bus) {
                return;
            }
            match fetch_dynamic_plugins(&cx, &agent_id).await {
                Ok(plugins) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::CordisPlugins { plugins }));
                }
                Err(error) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                        "dynamic plugins unavailable: {error}"
                    ))));
                }
            }
        }
        Cmd::SetCordisPluginEnabled {
            agent_id,
            plugin_id,
            enabled,
        } => {
            if !ensure_agent_cordis(&surface, &bus) {
                return;
            }
            let method = if enabled {
                crate::cordis::PLUGIN_START
            } else {
                crate::cordis::PLUGIN_STOP
            };
            let action = call_tui_extension(
                &cx,
                method,
                serde_json::json!({
                    "agentId": &agent_id,
                    "pluginId": &plugin_id,
                }),
            )
            .await;
            match action {
                Ok(value) if value.get("ok").and_then(Value::as_bool) == Some(false) => {
                    let message = value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("the Host rejected the lifecycle change");
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(message.to_string())));
                }
                Ok(_) => match fetch_dynamic_plugins(&cx, &agent_id).await {
                    Ok(plugins) => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::CordisPlugins { plugins }));
                    }
                    Err(error) => {
                        let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                            "plugin changed, but inventory refresh failed: {error}"
                        ))));
                    }
                },
                Err(error) => {
                    let action = if enabled { "restore" } else { "stop" };
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                        "plugin {action} failed: {error}"
                    ))));
                }
            }
        }
        Cmd::RespondCordisApproval {
            request_id,
            decision,
        } => {
            if !ensure_agent_cordis(&surface, &bus) {
                return;
            }
            if let Err(error) = call_tui_extension(
                &cx,
                crate::cordis::APPROVAL_RESPOND,
                serde_json::json!({
                    "protocol": crate::cordis::PROTOCOL,
                    "requestId": request_id,
                    "decision": decision,
                }),
            )
            .await
            {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                    "plugin approval failed: {error}"
                ))));
            }
        }
        Cmd::InvokePluginCommand { name, args } => {
            if !ensure_agent_cordis(&surface, &bus) {
                return;
            }
            if let Err(error) = call_tui_extension(
                &cx,
                crate::cordis::COMMAND_INVOKE,
                serde_json::json!({
                    "protocol": 0,
                    "name": name,
                    "args": args,
                }),
            )
            .await
            {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                    "plugin command failed: {error}"
                ))));
            }
        }
        Cmd::PluginThemeSelected { agent_id, id } => {
            if !ensure_agent_cordis(&surface, &bus) {
                return;
            }
            let result = call_tui_extension(
                &cx,
                crate::cordis::THEME_SELECTED,
                serde_json::json!({
                    "protocol": 0,
                    "agentId": agent_id,
                    "id": id,
                }),
            )
            .await;
            if let Err(error) = result {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                    "theme Plugin selection failed: {error}"
                ))));
            }
        }
        Cmd::PluginUiSelected { agent_id, id } => {
            if !ensure_agent_cordis(&surface, &bus) {
                return;
            }
            match call_tui_extension(
                &cx,
                crate::cordis::UI_SELECTED,
                serde_json::json!({
                    "protocol": crate::cordis::PROTOCOL,
                    "agentId": agent_id,
                    "id": id,
                }),
            )
            .await
            {
                Ok(_) => {}
                Err(error) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                        "UI Plugin selection failed: {error}"
                    ))));
                }
            }
        }
        Cmd::PluginOverlayEvent { id, event, value } => {
            if !ensure_agent_cordis(&surface, &bus) {
                return;
            }
            let mut params = serde_json::json!({
                "protocol": 0,
                "id": id,
                "event": event,
            });
            if let Some(value) = value {
                params["value"] = value;
            }
            let result = if event == "change" {
                UntypedMessage::new(crate::cordis::OVERLAY_EVENT, params)
                    .and_then(|notification| cx.send_notification(notification))
                    .map(|_| Value::Null)
            } else {
                call_tui_extension(&cx, crate::cordis::OVERLAY_EVENT, params).await
            };
            if let Err(error) = result {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                    "plugin overlay event failed: {error}"
                ))));
            }
        }
        Cmd::SetPermission {
            session_id: cmd_session,
            preset,
        } => {
            let sid = SessionId::new(cmd_session);
            match cx
                .send_request(SetSessionModeRequest::new(sid.clone(), preset.clone()))
                .block_task_deadline()
                .await
            {
                Ok(_) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(format!(
                        "permission → {preset}"
                    ))));
                }
                Err(err) if is_auth_required_error(&err) => {
                    emit_needs_auth_open(
                        &bus,
                        methods.clone(),
                        selected.as_ref(),
                        Some(acp_error_message(&err)),
                    );
                }
                Err(err) => {
                    let _ = cx
                        .send_request(SetSessionConfigOptionRequest::new(
                            sid,
                            "mode",
                            SessionConfigOptionValue::value_id(preset.clone()),
                        ))
                        .block_task_deadline()
                        .await;
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpDone(format!(
                        "permission → {preset} ({err})"
                    ))));
                }
            }
        }
        Cmd::SetPreset {
            session_id: cmd_session,
            preset,
        } => {
            let sid = SessionId::new(cmd_session);
            let config_id = surface
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .session(&sid.0)
                .composition_id
                .clone()
                .unwrap_or_else(|| "agent".into());
            match cx
                .send_request(SetSessionConfigOptionRequest::new(
                    sid.clone(),
                    config_id,
                    SessionConfigOptionValue::value_id(preset.clone()),
                ))
                .block_task_deadline()
                .await
            {
                Ok(response) => {
                    if let Ok(value) = serde_json::to_value(&response) {
                        if let Some(options) = value
                            .get("configOptions")
                            .or_else(|| value.get("config_options"))
                        {
                            if let Ok(mut surface) = surface.lock() {
                                surface.apply_config_options(options, &bus, Some(&sid.0));
                            }
                        }
                    }
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::PresetSet {
                        session_id: sid.to_string(),
                        preset,
                    }));
                }
                Err(err) if is_auth_required_error(&err) => {
                    emit_needs_auth_open(
                        &bus,
                        methods.clone(),
                        selected.as_ref(),
                        Some(acp_error_message(&err)),
                    );
                }
                Err(err) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(format!(
                        "composition switch failed: {err}"
                    ))));
                }
            }
        }
        Cmd::SetConfigOption {
            session_id: cmd_session,
            config_id,
            value,
        } => {
            let sid = SessionId::new(cmd_session);
            match cx
                .send_request(SetSessionConfigOptionRequest::new(
                    sid.clone(),
                    config_id,
                    SessionConfigOptionValue::value_id(value),
                ))
                .block_task_deadline()
                .await
            {
                Ok(response) => {
                    if let Ok(value) = serde_json::to_value(&response) {
                        let _ = apply_config_response(&value, &sid, &surface, &bus);
                    }
                }
                Err(err) if is_auth_required_error(&err) => {
                    emit_needs_auth_open(
                        &bus,
                        methods.clone(),
                        selected.as_ref(),
                        Some(acp_error_message(&err)),
                    );
                }
                Err(err) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionOpFailed {
                        session_id: sid.to_string(),
                        message: format!("config option switch failed: {err}"),
                    }));
                }
            }
        }
        Cmd::Authenticate { method_id, values } => {
            let method = select_auth_method(&methods, Some(&method_id))
                .or_else(|| select_auth_method(&methods, None))
                .cloned();
            let Some(method) = method else {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    if method_id.is_empty() {
                        "no supported ACP auth method is available".into()
                    } else {
                        format!("ACP auth method is unavailable or not supported: {method_id}")
                    },
                )));
                return;
            };
            if method.is_env_prompt() {
                let vars = method
                    .vars
                    .iter()
                    .map(|v| v.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                                    if vars.is_empty() {
                                        format!(
                                            "ACP auth method {} requires credential variables and cannot be started as a sign-in flow.",
                                            method.id
                                        )
                                    } else {
                                        format!(
                                            "ACP auth method {} requires credential variables ({vars}) and cannot be started as a sign-in flow.",
                                            method.id
                                        )
                                    },
                                )));
                return;
            }
            if method.form && authenticate_meta_from_method(&method, &values).is_none() {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::TuiOpFailed(
                    "sign-in needs values — /auth <api-key> (gateway: /auth <base-url> <api-key>)"
                        .into(),
                )));
                return;
            }
            let mut req = AuthenticateRequest::new(method.id.clone());
            if let Some(meta) = authenticate_meta_from_method(&method, &values) {
                req = req.meta(meta);
            }
            let result = cx.send_request(req).block_task_deadline().await.map(|_| ());
            let _ = done.send(ControlFinish::Authenticated { method, result });
        }
        Cmd::NewSession => {
            let result = cx
                .send_request(NewSessionRequest::new(cwd.clone()))
                .block_task_deadline()
                .await
                .map(|created| {
                    let sid = created.session_id.clone();
                    let notice = Some(format!(
                        "new session · {sid} — /agent picks its agent preset"
                    ));
                    (
                        sid,
                        serde_json::to_value(created).unwrap_or(Value::Null),
                        notice,
                    )
                });
            let _ = done.send(ControlFinish::Setup { result });
        }
        Cmd::ListSessions {
            requester_session_id,
            prefix,
            limit,
        } => {
            if !list_session {
                let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionListUnavailable {
                    requester_session_id,
                    prefix,
                    limit,
                    error: "agent did not advertise sessionCapabilities.list".into(),
                }));
                return;
            }
            match cx
                .send_request(ListSessionsRequest::new().cwd(cwd.clone()))
                .block_task_deadline()
                .await
            {
                Ok(listed) => {
                    let sessions: Vec<SessionListItem> = listed
                        .sessions
                        .into_iter()
                        .map(|s| SessionListItem {
                            id: s.session_id.to_string(),
                            title: s.title,
                            updated_at: s.updated_at,
                        })
                        .collect();
                    // `/resume n`: the limit is applied on the
                    // App side, after the current session is
                    // filtered out of the list.
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionList {
                        requester_session_id,
                        sessions,
                        prefix,
                        limit,
                    }));
                }
                Err(err) if is_auth_required_error(&err) => {
                    emit_needs_auth_open(
                        &bus,
                        methods.clone(),
                        selected.as_ref(),
                        Some(acp_error_message(&err)),
                    );
                }
                Err(err) => {
                    let _ = bus.send(AppEvent::Ctl(CtlEvent::SessionListUnavailable {
                        requester_session_id,
                        prefix,
                        limit,
                        error: err.to_string(),
                    }));
                }
            }
        }
        Cmd::ResumeSession { session_id: id } => {
            let sid = SessionId::new(id.clone());
            let restored = if resume_session {
                match cx
                    .send_request(ResumeSessionRequest::new(sid.clone(), cwd.clone()))
                    .block_task_deadline()
                    .await
                {
                    Ok(resumed) => Ok((serde_json::to_value(resumed).unwrap_or(Value::Null), true)),
                    Err(err) if load_session && !is_auth_required_error(&err) => cx
                        .send_request(LoadSessionRequest::new(sid.clone(), cwd.clone()))
                        .block_task_deadline()
                        .await
                        .map(|loaded| (serde_json::to_value(loaded).unwrap_or(Value::Null), false)),
                    Err(err) => Err(err),
                }
            } else {
                cx.send_request(LoadSessionRequest::new(sid.clone(), cwd.clone()))
                    .block_task_deadline()
                    .await
                    .map(|loaded| (serde_json::to_value(loaded).unwrap_or(Value::Null), false))
            };
            let result = restored.map(|(setup, resumed)| {
                let notice = if resumed {
                    format!("⟲ resumed {id} — previous transcript was not replayed")
                } else {
                    format!("⟲ loaded {id} — transcript from session/update")
                };
                (sid, setup, Some(notice))
            });
            let _ = done.send(ControlFinish::Setup { result });
        }
        Cmd::QueueSnapshot { snapshot } => {
            let items = snapshot
                .items
                .into_iter()
                .map(|item| {
                    serde_json::json!({
                        "id": item.id,
                        "ordinal": item.ordinal,
                        "summary": item.summary,
                    })
                })
                .collect::<Vec<_>>();
            // This method belongs to the local Client compositor.
            // Direct native launches may not have one, so absence is
            // intentionally silent and never affects prompt flow.
            // Guarded by the initialize negotiation: an
            // un-negotiated agent gets no `_dsh/cordis`
            // traffic at all (protocol 0 rule).
            if agent_cordis_negotiated(&surface) {
                let _ = call_tui_extension(
                    &cx,
                    crate::cordis::QUEUE_UPDATE,
                    serde_json::json!({
                        "protocol": crate::cordis::PROTOCOL,
                        "count": snapshot.count,
                        "items": items,
                        "selectedId": snapshot.selected_id,
                        "editingId": snapshot.editing_id,
                        "deleteConfirm": snapshot.delete_confirm,
                    }),
                )
                .await;
            }
        }
        Cmd::AgentsSnapshot { snapshot } => {
            let items = snapshot
                .items
                .into_iter()
                .map(|item| {
                    serde_json::json!({
                        "id": item.id,
                        "label": item.label,
                        "kind": item.kind,
                        "status": item.status,
                        "current": item.current,
                    })
                })
                .collect::<Vec<_>>();
            // Agent transcript navigation is Client chrome;
            // it never becomes an ACP prompt or timeline cell.
            // Same negotiation guard as the queue snapshot.
            if agent_cordis_negotiated(&surface) {
                let _ = call_tui_extension(
                    &cx,
                    crate::cordis::AGENTS_UPDATE,
                    serde_json::json!({
                        "protocol": crate::cordis::PROTOCOL,
                        "activeId": snapshot.active_id,
                        "selectedId": snapshot.selected_id,
                        "items": items,
                    }),
                )
                .await;
            }
        }
        Cmd::ActiveSession { session_id } => {
            if agent_cordis_negotiated(&surface) {
                let _ = call_tui_extension(
                    &cx,
                    crate::cordis::SESSION_ACTIVE,
                    serde_json::json!({
                        "protocol": crate::cordis::PROTOCOL,
                        "sessionId": session_id,
                    }),
                )
                .await;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "../../tests/unit/acp__control_tests.rs"]
mod tests;
