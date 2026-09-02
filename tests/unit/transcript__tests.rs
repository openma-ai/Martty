use super::*;

fn t(session: &str) -> Transcript {
    Transcript::new(session.to_string())
}

#[test]
fn cancel_open_work_stops_running_tools() {
    let mut tr = t("s");
    tr.apply(UiEvent::ToolCall {
        session: "s".into(),
        call_id: "c1".into(),
        name: "bash".into(),
        arguments: "{}".into(),
    });
    tr.cancel_open_work();
    match &tr.cells[0].kind {
        CellKind::Tool { ok, error, .. } => {
            assert_eq!(*ok, Some(false));
            assert_eq!(error.as_deref(), Some("cancelled"));
        }
        other => panic!("expected tool, got {other:?}"),
    }
    assert_eq!(tr.last_finish.as_deref(), Some("cancelled"));
}

#[test]
fn streaming_text_appends_and_finalizes() {
    let mut tr = t("s");
    tr.apply(UiEvent::TextDelta {
        session: "s".into(),
        text: "Hello ".into(),
    });
    tr.apply(UiEvent::TextDelta {
        session: "s".into(),
        text: "world".into(),
    });
    assert!(tr.streaming());
    tr.apply(UiEvent::AssistantFinal {
        session: "s".into(),
        text: "Hello world!".into(),
        model: Some("m".into()),
    });
    assert!(!tr.streaming());
    match &tr.cells[0].kind {
        CellKind::Assistant { text, done, .. } => {
            assert_eq!(text, "Hello world!");
            assert!(done);
        }
        other => panic!("unexpected cell {other:?}"),
    }
}

#[test]
fn structured_session_warning_renders_as_notice_not_assistant_copy() {
    let mut tr = t("s");
    tr.apply(UiEvent::SessionNotice {
        session: "s".into(),
        severity: "warning".into(),
        title: "Skill descriptions were shortened".into(),
        details: Some("Disable unused skills or plugins.".into()),
    });

    assert!(matches!(
        &tr.cells[0].kind,
        CellKind::Notice {
            level: NoticeLevel::Warn,
            text,
        } if text == "Skill descriptions were shortened — Disable unused skills or plugins."
    ));
    assert!(!matches!(&tr.cells[0].kind, CellKind::Assistant { .. }));
}

#[test]
fn streaming_assistant_keeps_its_cursor_without_a_fallback_loading_icon() {
    let mut tr = t("s");
    tr.apply(UiEvent::TextDelta {
        session: "s".into(),
        text: "starting subagents".into(),
    });

    let mut rendered = |spinner| {
        tr.lines(&Theme::dark(), 80, spinner)
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>()
    };
    let first = rendered('⠋');
    let second = rendered('⠙');

    assert!(first.contains("starting subagents▍"), "{first}");
    assert!(second.contains("starting subagents▍"), "{second}");
    assert!(
        !first.contains('⠋'),
        "fallback loading icon leaked: {first}"
    );
    assert!(
        !second.contains('⠙'),
        "fallback loading icon leaked: {second}"
    );
}

#[test]
fn tool_call_pairs_with_result() {
    let mut tr = t("s");
    tr.apply(UiEvent::ToolCall {
        session: "s".into(),
        call_id: "c1".into(),
        name: "bash".into(),
        arguments: r#"{"command":"ls"}"#.into(),
    });
    tr.apply(UiEvent::ToolResult {
        session: "s".into(),
        call_id: "c1".into(),
        is_error: false,
        text: "a\nb".into(),
        error: None,
    });
    match &tr.cells[0].kind {
        CellKind::Tool {
            name, title, ok, ..
        } => {
            assert_eq!(name, "bash");
            assert_eq!(title, "ls");
            assert_eq!(*ok, Some(true));
        }
        other => panic!("unexpected cell {other:?}"),
    }
}

#[test]
fn streamed_tool_request_updates_one_existing_cell() {
    let mut tr = t("s");
    tr.apply(UiEvent::ToolCall {
        session: "s".into(),
        call_id: "c1".into(),
        name: "Subagent".into(),
        arguments: r#"{"arguments":"{\"task\":"}"#.into(),
    });
    tr.apply(UiEvent::ToolCall {
        session: "s".into(),
        call_id: "c1".into(),
        name: "Subagent: inspect renderer".into(),
        arguments: r#"{"task":"inspect renderer"}"#.into(),
    });

    assert_eq!(
        tr.cells.len(),
        1,
        "a tool update must not append another request"
    );
    match &tr.cells[0].kind {
        CellKind::Tool {
            name, request, ok, ..
        } => {
            assert_eq!(name, "Subagent: inspect renderer");
            assert_eq!(request, r#"{"task":"inspect renderer"}"#);
            assert_eq!(*ok, None);
        }
        other => panic!("unexpected cell {other:?}"),
    }
}

#[test]
fn turn_end_settles_an_orphaned_tool_in_the_client_presentation() {
    let mut tr = t("s");
    tr.apply(UiEvent::ToolCall {
        session: "s".into(),
        call_id: "c1".into(),
        name: "Subagent".into(),
        arguments: "{}".into(),
    });
    tr.apply(UiEvent::TurnEnd {
        session: "s".into(),
        kind: "interrupted".into(),
    });

    match &tr.cells[0].kind {
        CellKind::Tool { ok, error, .. } => {
            assert_eq!(*ok, Some(false));
            assert_eq!(error.as_deref(), Some("interrupted"));
        }
        other => panic!("unexpected cell {other:?}"),
    }
    assert!(
        !tr.streaming(),
        "the stopped turn owns all open presentation state"
    );
}

#[test]
fn reasoning_closes_when_text_starts() {
    let mut tr = t("s");
    tr.apply(UiEvent::ReasoningDelta {
        session: "s".into(),
        text: "plan".into(),
    });
    tr.apply(UiEvent::TextDelta {
        session: "s".into(),
        text: "answer".into(),
    });
    match &tr.cells[0].kind {
        CellKind::Reasoning { done, .. } => assert!(done),
        other => panic!("unexpected cell {other:?}"),
    }
}

#[test]
fn usage_accumulates() {
    let mut tr = t("s");
    for _ in 0..2 {
        tr.apply(UiEvent::Usage {
            session: "s".into(),
            input: 10,
            output: 5,
            cached: 3,
            reasoning: 1,
        });
    }
    assert_eq!(tr.usage.input, 20);
    assert_eq!(tr.usage.output, 10);
    assert_eq!(tr.usage.cached, 6);
}

fn line_width(line: &Line) -> usize {
    line.spans
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
        .sum()
}

#[test]
fn wrap_handles_cjk() {
    let lines = wrap("深度求索深度求索", 8);
    assert_eq!(lines.len(), 2);
}

#[test]
fn user_bubble_stays_within_the_pane_width() {
    // The bubble line is "❯ " (2 cells) + " {l} " (text + 2 cells); the wrap
    // budget must pay for all 4 chrome cells or packed CJK lines clip.
    let mut tr = t("s");
    tr.push_user("深度求索深度求索深度求索深度求索深度求索".into(), false);
    let theme = Theme::dark();
    for width in [20u16, 40, 80] {
        for (i, line) in tr.lines(&theme, width, '⠋').iter().enumerate() {
            assert!(
                line_width(line) <= width as usize,
                "line {i} at pane width {width} paints {} cells: {line:?}",
                line_width(line),
            );
        }
    }
}

#[test]
fn wrap_expands_tabs_and_never_exceeds_width() {
    use unicode_width::UnicodeWidthStr;
    let cases = [
        format!("\t{}", "x".repeat(120)),
        format!("{}\t{}", "x".repeat(10), "y".repeat(110)),
        "\t".repeat(20) + &"code();".repeat(15),
        "a".repeat(200),
        "深度求索".repeat(40),
    ];
    for text in cases {
        for width in [40usize, 80, 120] {
            for line in wrap(&text, width) {
                let w = UnicodeWidthStr::width(line.as_str());
                assert!(
                    w <= width,
                    "wrapped line {line:?} is {w} cols, budget {width}"
                );
                assert!(
                    !line.contains('\t'),
                    "tabs must expand to spaces so display width matches: {line:?}"
                );
            }
        }
    }
}

#[test]
fn write_tool_tab_indented_body_fits_terminal_width() {
    // GitHub issue #5: writing a file whose lines contain tabs rendered
    // at 128 cols in a 120-col terminal (tab counted as 0, displayed as 8,
    // plus the "│ " gutter).
    let mut tr = t("s");
    let body = format!("\t{}", "x".repeat(120));
    tr.apply(UiEvent::ToolCall {
        session: "s".into(),
        call_id: "c1".into(),
        name: "str_replace_editor".into(),
        arguments: r#"{"command":"create","path":"src/lib.rs"}"#.into(),
    });
    tr.apply(UiEvent::ToolResult {
        session: "s".into(),
        call_id: "c1".into(),
        is_error: false,
        text: body,
        error: Some("x".repeat(140)),
    });
    let theme = Theme::dark();
    let width = 120u16;
    let layout = tr.layout(&theme, width, ' ', false);
    for (i, line) in layout.lines.iter().enumerate() {
        let w = line_width(line);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            w <= width as usize,
            "line {i} width {w} > {width}: {text:?}"
        );
        assert!(!text.contains('\t'), "tab leaked into layout line {i}");
    }
}

#[test]
fn render_smoke() {
    let mut tr = t("s");
    tr.push_user("hi".into(), false);
    tr.apply(UiEvent::TextDelta {
        session: "s".into(),
        text: "yo".into(),
    });
    let theme = Theme::dark();
    let lines = tr.lines(&theme, 40, '⠋');
    assert!(lines.len() >= 3);
}

#[test]
fn acp_load_replay_paints_user_title_and_plan() {
    let mut tr = t("s");
    tr.apply(UiEvent::UserMessage {
        session: "s".into(),
        text: "hello from load".into(),
    });
    tr.apply(UiEvent::SessionTitle {
        session: "s".into(),
        title: "fix the tests".into(),
    });
    tr.apply(UiEvent::Plan {
        session: "s".into(),
        summary: "locate fn main [completed]".into(),
    });
    match &tr.cells[0].kind {
        CellKind::User { text, queued } => {
            assert_eq!(text, "hello from load");
            assert!(!*queued);
        }
        other => panic!("expected user cell, got {other:?}"),
    }
    let notices: Vec<&str> = tr
        .cells
        .iter()
        .filter_map(|c| match &c.kind {
            CellKind::Notice { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(notices.iter().any(|n| *n == "session · fix the tests"));
    let plans: Vec<&str> = tr
        .cells
        .iter()
        .filter_map(|c| match &c.kind {
            CellKind::Plan { summary } => Some(summary.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(plans, ["locate fn main [completed]"]);
}

#[test]
fn plan_snapshots_replace_the_existing_plan_in_place() {
    let mut tr = t("s");
    tr.apply(UiEvent::Plan {
        session: "s".into(),
        summary: "inspect [in_progress]".into(),
    });
    let cells_after_first = tr.cells.len();

    tr.apply(UiEvent::Plan {
        session: "s".into(),
        summary: "inspect [completed] · test [in_progress]".into(),
    });

    assert_eq!(
        tr.cells.len(),
        cells_after_first,
        "a full plan snapshot updates one client-side view instead of appending chat history"
    );
    let rendered = tr
        .lines(&Theme::dark(), 80, '⠋')
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(!rendered.contains("inspect [in_progress]"));
    assert!(rendered.contains("inspect [completed] · test [in_progress]"));
}

#[test]
fn empty_plan_snapshot_hides_the_existing_plan() {
    let mut tr = t("s");
    tr.apply(UiEvent::Plan {
        session: "s".into(),
        summary: "inspect [in_progress]".into(),
    });
    tr.apply(UiEvent::Plan {
        session: "s".into(),
        summary: String::new(),
    });

    let rendered = tr
        .lines(&Theme::dark(), 80, '⠋')
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>();
    assert!(!rendered.contains("plan ·"));
    assert!(!rendered.contains("inspect"));
}

#[test]
fn tool_preview_shows_a_fixed_tail_and_expand_all_opens_it() {
    let text = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8";
    let mut tr = t("s");
    tr.apply(UiEvent::ToolCall {
        session: "s".into(),
        call_id: "c1".into(),
        name: "bash".into(),
        arguments: "{}".into(),
    });
    tr.apply(UiEvent::ToolResult {
        session: "s".into(),
        call_id: "c1".into(),
        is_error: false,
        text: text.into(),
        error: None,
    });
    let theme = Theme::dark();
    let plain = |lines: &[Line]| -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect()
    };

    // Default: a fixed 4-line tail preview (l5..l8) + expand hint.
    let lines = tr.lines(&theme, 40, ' ');
    let p = plain(&lines);
    assert!(
        p.contains("last 4/8 lines"),
        "footer describes the fixed tail preview: {p}"
    );
    assert!(
        !p.contains("wheel"),
        "tool preview has no inner scroll: {p}"
    );
    assert!(!p.contains("l4"), "l4 above the tail window is hidden: {p}");
    assert!(p.contains("l8"), "tail line visible: {p}");

    // expand_all (ctrl+o) opens the whole body and drops the footer.
    tr.expand_all = true;
    let lines = tr.lines(&theme, 40, ' ');
    let p = plain(&lines);
    assert!(p.contains("l1"), "expand_all shows the top: {p}");
    assert!(
        !p.contains("click to expand"),
        "no footer when expanded: {p}"
    );
}

#[test]
fn image_cell_reserves_thumbnail_or_falls_back_to_path() {
    let mut tr = t("s");
    let mut png = vec![0u8; 24];
    png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    png[16..20].copy_from_slice(&100u32.to_be_bytes());
    png[20..24].copy_from_slice(&100u32.to_be_bytes());
    tr.push_image(
        "pic.png".into(),
        "look".into(),
        "/tmp/pic.png".into(),
        std::sync::Arc::from(png.clone()),
        false,
    );
    let theme = Theme::dark();

    // Thumbnails on: reserve blank lines and report the placement.
    let layout = tr.layout(&theme, 40, ' ', true);
    assert_eq!(layout.images.len(), 1, "PNG image reports one shot");
    let shot = &layout.images[0];
    assert_eq!(shot.cols, 24);
    assert!(
        (2..=12).contains(&shot.rows),
        "aspect-true rows: {}",
        shot.rows
    );
    let reserved: String = layout.lines[shot.line]
        .spans
        .iter()
        .map(|s| s.content.to_string())
        .collect();
    assert!(
        reserved.trim().is_empty(),
        "reserved line is blank: {reserved:?}"
    );

    // Thumbnails off: no reservation, path shown as the fallback.
    let layout = tr.layout(&theme, 40, ' ', false);
    assert!(layout.images.is_empty());
    let text: String = layout
        .lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
        .collect();
    assert!(text.contains("/tmp/pic.png"), "fallback path shown: {text}");
}

#[test]
fn stats_accumulate_turns_steps_and_ttft() {
    let mut tr = t("s");
    tr.apply(UiEvent::TurnStart {
        session: "s".into(),
        turn: 1,
    });
    tr.apply(UiEvent::AssistantFinal {
        session: "s".into(),
        text: "hi".into(),
        model: None,
    });
    tr.apply(UiEvent::ToolCall {
        session: "s".into(),
        call_id: "c1".into(),
        name: "bash".into(),
        arguments: "{}".into(),
    });
    tr.apply(UiEvent::ToolResult {
        session: "s".into(),
        call_id: "c1".into(),
        is_error: false,
        text: "ok".into(),
        error: None,
    });
    tr.apply(UiEvent::TextDelta {
        session: "s".into(),
        text: "first".into(),
    });
    tr.apply(UiEvent::TextDelta {
        session: "s".into(),
        text: "second".into(),
    });
    tr.apply(UiEvent::TurnEnd {
        session: "s".into(),
        kind: "completed".into(),
    });

    assert_eq!(tr.stats.turns, 1);
    assert_eq!(tr.stats.steps, 1);
    assert_eq!(tr.stats.ttft_count, 1, "only the first delta samples TTFT");
    assert!(tr.stats.tool_millis <= tr.stats.turn_millis);
}
