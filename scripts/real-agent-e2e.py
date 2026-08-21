#!/usr/bin/env python3
"""Opt-in real Creator-agent E2E runner and durable trajectory scorer.

This deliberately drives the real ``tui-test`` profile through a PTY.  The
separate ``headless`` profile does not mount the TUI Client Cordis Providers
and therefore cannot validate this workflow.
"""

from __future__ import annotations

import argparse
from collections import Counter
import glob
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any


def read_events(log_path: Path) -> list[dict[str, Any]]:
    if log_path.name.endswith(".zstd"):
        completed = subprocess.run(
            ["zstdcat", str(log_path)],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        raw = completed.stdout.decode("utf-8")
    else:
        raw = log_path.read_text(encoding="utf-8")
    events: list[dict[str, Any]] = []
    for line_number, line in enumerate(raw.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"{log_path}:{line_number}: invalid JSON: {error}") from error
        if isinstance(event, dict):
            events.append(event)
    return events


def tool_arguments(event: dict[str, Any]) -> dict[str, Any]:
    raw = event.get("data", {}).get("arguments", {})
    if isinstance(raw, dict):
        return raw
    if not isinstance(raw, str):
        return {}
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def ordered_unique(values: list[str]) -> list[str]:
    return list(dict.fromkeys(values))


def score(log_path: Path, spec_path: Path) -> dict[str, Any]:
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    expected = spec.get("expected", {})
    if not isinstance(expected, dict):
        raise ValueError("spec.expected must be an object")
    events = read_events(log_path)
    calls = [event for event in events if event.get("type") == "tool/call"]
    names = [str(event.get("data", {}).get("name", "")) for event in calls]
    counts = Counter(names)

    skills = ordered_unique([
        str(tool_arguments(event).get("name"))
        for event in calls
        if event.get("data", {}).get("name") == "skill"
        and isinstance(tool_arguments(event).get("name"), str)
    ])
    inspect_providers = ordered_unique([
        str(tool_arguments(event).get("provider"))
        for event in calls
        if event.get("data", {}).get("name") == "cordis_inspect_query"
        and isinstance(tool_arguments(event).get("provider"), str)
    ])
    inspect_queries = [
        {
            "platform": str(tool_arguments(event).get("platform", "")),
            "provider": str(tool_arguments(event).get("provider", "")),
            "method": str(tool_arguments(event).get("method", "")),
        }
        for event in calls
        if event.get("data", {}).get("name") == "cordis_inspect_query"
    ]
    define_calls = [
        tool_arguments(event)
        for event in calls
        if event.get("data", {}).get("name") == "cordis_define"
    ]
    client_code = "\n".join(
        str(arguments.get("code", {}).get("client", ""))
        for arguments in define_calls
        if isinstance(arguments.get("code"), dict)
    )
    turn_ends = [event for event in events if event.get("type") == "turn/end"]
    finish_reason = None
    if turn_ends:
        reason = turn_ends[-1].get("data", {}).get("reason", {})
        if isinstance(reason, dict):
            finish_reason = reason.get("kind")

    failures: list[str] = []

    def require(condition: bool, failure: str) -> None:
        if not condition:
            failures.append(failure)

    expected_skills = expected.get("skills")
    if isinstance(expected_skills, list):
        require(
            skills == expected_skills,
            f"skills exactly match {expected_skills!r}; observed {skills!r}",
        )
    forbidden_skills = expected.get("forbiddenSkills", [])
    if isinstance(forbidden_skills, list):
        observed_forbidden = [skill for skill in skills if skill in forbidden_skills]
        require(
            not observed_forbidden,
            f"forbidden skills absent; observed {observed_forbidden!r}",
        )

    expected_providers = expected.get("inspectProviders")
    if isinstance(expected_providers, list):
        require(
            Counter(inspect_providers) == Counter(expected_providers),
            "inspect providers exactly match "
            f"{expected_providers!r}; observed {inspect_providers!r}",
        )
    expected_queries = expected.get("inspectQueries")
    if isinstance(expected_queries, list):
        require(
            inspect_queries == expected_queries,
            "inspect queries exactly match "
            f"{expected_queries!r}; observed {inspect_queries!r}",
        )
    forbidden_providers = expected.get("forbiddenInspectProviders", [])
    if isinstance(forbidden_providers, list):
        observed_forbidden = [
            provider for provider in inspect_providers if provider in forbidden_providers
        ]
        require(
            not observed_forbidden,
            f"forbidden inspect providers absent; observed {observed_forbidden!r}",
        )

    tool_counts = expected.get("toolCounts", {})
    if isinstance(tool_counts, dict):
        for name, wanted in tool_counts.items():
            require(
                counts[str(name)] == wanted,
                f"{name} count is {wanted}; observed {counts[str(name)]}",
            )
    max_tool_counts = expected.get("maxToolCounts", {})
    if isinstance(max_tool_counts, dict):
        for name, maximum in max_tool_counts.items():
            if not isinstance(maximum, int):
                continue
            require(
                counts[str(name)] <= maximum,
                f"{name} count <= {maximum}; observed {counts[str(name)]}",
            )
    max_tool_calls = expected.get("maxToolCalls")
    if isinstance(max_tool_calls, int):
        require(
            len(calls) <= max_tool_calls,
            f"tool call count <= {max_tool_calls}; observed {len(calls)}",
        )
    forbidden_tools = expected.get("forbiddenTools", [])
    if isinstance(forbidden_tools, list):
        observed_forbidden = [name for name in names if name in forbidden_tools]
        require(
            not observed_forbidden,
            f"forbidden tools absent; observed {observed_forbidden!r}",
        )

    if expected.get("directDefineRunAfterInspect") is True:
        inspect_indexes = [
            index
            for index, name in enumerate(names)
            if name in {"cordis_inspect_list", "cordis_inspect_query", "cordis_inspect_self"}
        ]
        direct = False
        if inspect_indexes:
            tail = names[inspect_indexes[-1] + 1 : inspect_indexes[-1] + 3]
            direct = tail == ["cordis_define", "cordis_run"]
        require(
            direct,
            "last inspect is followed directly by cordis_define then cordis_run; "
            f"observed tools {names!r}",
        )

    run_targets = [
        (
            str(tool_arguments(event).get("pluginId", "")),
            str(tool_arguments(event).get("packageId", "")),
        )
        for event in calls
        if event.get("data", {}).get("name") == "cordis_run"
    ]
    host_runner_messages = [
        event_text(event)
        for event in events
        if event.get("type") == "user/message"
        and event.get("data", {}).get("source", {}).get("plugin")
        == "cordis-host-runner"
    ]
    successful_runs = [
        f"{plugin_id}/{package_id}"
        for plugin_id, package_id in run_targets
        if plugin_id
        and package_id
        and any(
            f"Cordis run {plugin_id}/{package_id}" in message
            and "completed successfully" in message
            for message in host_runner_messages
        )
    ]
    if expected.get("successfulRun") is True:
        require(
            bool(run_targets) and len(successful_runs) == len(run_targets),
            "every cordis_run Package must activate successfully; "
            f"targets {[f'{plugin}/{package}' for plugin, package in run_targets]!r}, "
            f"confirmed {successful_runs!r}",
        )

    for pattern in expected.get("clientCodeMustMatch", []):
        require(
            re.search(str(pattern), client_code, flags=re.MULTILINE) is not None,
            f"client code must match /{pattern}/",
        )
    for pattern in expected.get("clientCodeMustNotMatch", []):
        require(
            re.search(str(pattern), client_code, flags=re.MULTILINE) is None,
            f"client code must not match /{pattern}/",
        )

    if expected.get("completedTurn", True) is True:
        require(
            finish_reason == "completed",
            f"turn finishes completed; observed {finish_reason!r}",
        )

    steps = ordered_unique([
        str(event.get("data", {}).get("step"))
        for event in calls
        if event.get("data", {}).get("step") is not None
    ])
    return {
        "passed": not failures,
        "log": str(log_path.resolve()),
        "spec": str(spec_path.resolve()),
        "finishReason": finish_reason,
        "metrics": {
            "toolCalls": len(calls),
            "toolSteps": len(steps),
            "inspectQueries": counts["cordis_inspect_query"],
        },
        "skills": skills,
        "inspectProviders": inspect_providers,
        "inspectQueries": inspect_queries,
        "successfulRuns": successful_runs,
        "tools": names,
        "failures": failures,
    }


def event_text(event: dict[str, Any]) -> str:
    content = event.get("data", {}).get("content", [])
    if not isinstance(content, list):
        return ""
    return "".join(
        str(block.get("text", ""))
        for block in content
        if isinstance(block, dict) and block.get("type") == "text"
    )


def workspace_slug(workspace: Path) -> str:
    readable = str(workspace.resolve()).replace(os.sep, "-").lstrip("-") or "root"
    return f"--{readable[:251]}--"


def session_roots(configured: Path) -> list[Path]:
    dsh_home = Path(os.environ.get("DSH_HOME", str(Path.home() / ".dsh")))
    roots = [
        configured,
        dsh_home / "sessions",
        Path.home() / ".martty" / "sessions",
        Path.home() / ".dsh-tui" / "sessions",
    ]
    return list(dict.fromkeys(path.resolve() for path in roots))


def workspace_logs(roots: list[Path], workspace: Path) -> list[Path]:
    candidates: list[Path] = []
    slug = workspace_slug(workspace)
    for root in roots:
        candidates.extend(Path(path) for path in glob.glob(str(root / slug / "*" / "session.jsonl*")))
        candidates.extend(Path(path) for path in glob.glob(str(root / "*" / "session.jsonl*")))
    matching: list[Path] = []
    for candidate in dict.fromkeys(candidates):
        try:
            events = read_events(candidate)
        except (OSError, subprocess.SubprocessError, ValueError):
            continue
        header = next((event for event in events if event.get("type") == "session"), None)
        cwd = header.get("cwd") if isinstance(header, dict) else None
        if isinstance(cwd, str) and Path(cwd).resolve() == workspace.resolve():
            matching.append(candidate)
    return sorted(matching, key=lambda path: path.stat().st_mtime, reverse=True)


def find_prompt_log(
    roots: list[Path], workspace: Path, prompt: str
) -> tuple[Path | None, list[dict[str, Any]]]:
    candidates = workspace_logs(roots, workspace)
    for candidate in sorted(candidates, key=lambda path: path.stat().st_mtime, reverse=True):
        try:
            events = read_events(candidate)
        except (OSError, subprocess.SubprocessError, ValueError):
            continue
        if any(event.get("type") == "user/message" and prompt in event_text(event) for event in events):
            return candidate, events
    return None, []


def has_selected_preset(roots: list[Path], workspace: Path, preset: str) -> bool:
    for raw_path in workspace_logs(roots, workspace):
        try:
            events = read_events(raw_path)
        except (OSError, subprocess.SubprocessError, ValueError):
            continue
        if any(
            event.get("type") == "agent-preset/selected"
            and event.get("data", {}).get("agentPreset") == preset
            for event in events
        ):
            return True
    return False


def drive_real_agent(args: argparse.Namespace) -> tuple[dict[str, Any], Path]:
    try:
        import pexpect  # type: ignore
    except ImportError as error:
        raise RuntimeError("run mode requires Python package pexpect") from error

    spec_path = Path(args.spec).resolve()
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    prompt = args.prompt or spec.get("prompt")
    if not isinstance(prompt, str) or not prompt.strip():
        raise ValueError("run requires --prompt or a non-empty spec.prompt")
    preset = args.agent_preset or spec.get("agentPreset", "cordis")
    if not isinstance(preset, str) or not preset:
        raise ValueError("agent preset must be a non-empty string")

    root = Path(args.root).resolve() if args.root else Path(
        tempfile.mkdtemp(prefix="dsh-tui-real-e2e.")
    )
    workspace = root / "workspace"
    session_root = root / "sessions"
    workspace.mkdir(parents=True, exist_ok=True)
    session_root.mkdir(parents=True, exist_ok=True)

    dsh = shutil.which(args.dsh_bin)
    if dsh is None:
        raise RuntimeError(f"cannot find dsh executable {args.dsh_bin!r}")
    environment = os.environ.copy()
    environment["DSH_SESSION_ROOT"] = str(session_root)
    roots = session_roots(session_root)
    if args.tui_bin:
        tui_bin = Path(args.tui_bin).resolve()
        if not tui_bin.is_file():
            raise RuntimeError(f"TUI binary does not exist: {tui_bin}")
        environment["DSH_TUI_BIN"] = str(tui_bin)

    child = pexpect.spawn(
        dsh,
        ["--profile", args.profile],
        cwd=str(workspace),
        env=environment,
        encoding="utf-8",
        codec_errors="ignore",
        timeout=1,
        dimensions=(40, 160),
    )
    output_tail = ""
    prompt_log: Path | None = None
    prompt_events: list[dict[str, Any]] = []
    approved_signatures: set[str] = set()
    started_at = time.time()
    try:
        child.expect(re.compile(args.ready_pattern), timeout=args.ready_timeout)
        preset_deadline = time.time() + args.preset_timeout
        next_preset_attempt = 0.0
        while time.time() < preset_deadline and child.isalive():
            if time.time() >= next_preset_attempt:
                child.sendcontrol("u")
                child.send(f"/agent {preset}\r")
                next_preset_attempt = time.time() + args.preset_retry
            try:
                output_tail = (output_tail + child.read_nonblocking(8192, timeout=0.2))[-65536:]
            except pexpect.TIMEOUT:
                pass
            except pexpect.EOF:
                break
            if has_selected_preset(roots, workspace, preset):
                break
            time.sleep(0.2)
        else:
            raise TimeoutError(f"agent preset {preset!r} was not durably selected")

        child.send(f"{prompt}\r")
        deadline = time.time() + args.timeout
        while time.time() < deadline and child.isalive():
            try:
                chunk = child.read_nonblocking(8192, timeout=0.2)
                output_tail = (output_tail + chunk)[-65536:]
            except pexpect.TIMEOUT:
                pass
            except pexpect.EOF:
                break

            prompt_log, prompt_events = find_prompt_log(roots, workspace, prompt)
            if prompt_log is not None:
                asks = [
                    event
                    for event in prompt_events
                    if event.get("type") in {"approval/asked", "permission/asked"}
                ]
                for event in asks:
                    signature = f"event:{event.get('seq')}"
                    if signature not in approved_signatures:
                        child.send("\r")
                        approved_signatures.add(signature)

                ends = [event for event in prompt_events if event.get("type") == "turn/end"]
                if ends:
                    break

            approval_match = re.search(
                r"(?i)(awaiting user approval|allow once|approve(?: this)? plugin)",
                output_tail,
            )
            if approval_match:
                signature = f"screen:{approval_match.group(0).lower()}"
                if signature not in approved_signatures:
                    child.send("\r")
                    approved_signatures.add(signature)
            time.sleep(0.2)
        else:
            raise TimeoutError(f"real agent turn did not finish within {args.timeout}s")
    finally:
        if child.isalive():
            child.sendcontrol("c")
            time.sleep(0.25)
            child.sendcontrol("c")
            time.sleep(0.5)
        if child.isalive():
            child.close(force=True)

    if prompt_log is None:
        prompt_log, prompt_events = find_prompt_log(roots, workspace, prompt)
    if prompt_log is None:
        raise RuntimeError(f"no durable session containing the prompt under {session_root}")
    report = score(prompt_log, spec_path)
    report["run"] = {
        "root": str(root),
        "workspace": str(workspace),
        "sessionRoot": str(session_root),
        "profile": args.profile,
        "agentPreset": preset,
        "durationSeconds": round(time.time() - started_at, 3),
        "approvalsSubmitted": len(approved_signatures),
    }
    if args.report:
        Path(args.report).write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
    return report, root


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    subparsers = root.add_subparsers(dest="command", required=True)

    analyze = subparsers.add_parser("analyze", help="score one durable session trajectory")
    analyze.add_argument("--log", required=True)
    analyze.add_argument("--spec", required=True)

    run = subparsers.add_parser("run", help="drive the real tui-test profile through a PTY")
    run.add_argument("--spec", required=True)
    run.add_argument("--prompt", default=os.environ.get("E2E_PROMPT"))
    run.add_argument("--profile", default="tui-test")
    run.add_argument("--agent-preset")
    run.add_argument("--dsh-bin", default="dsh")
    run.add_argument("--tui-bin", default=os.environ.get("DSH_TUI_BIN"))
    run.add_argument("--root")
    run.add_argument("--report")
    run.add_argument("--ready-pattern", default="describe what you want to build")
    run.add_argument("--ready-timeout", type=float, default=120)
    run.add_argument("--preset-timeout", type=float, default=60)
    run.add_argument("--preset-retry", type=float, default=5)
    run.add_argument("--timeout", type=float, default=900)
    run.add_argument("--cleanup", action="store_true")
    return root


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "analyze":
            report = score(Path(args.log), Path(args.spec))
            print(json.dumps(report, ensure_ascii=False))
            return 0 if report["passed"] else 1
        report, run_root = drive_real_agent(args)
        print(json.dumps(report, ensure_ascii=False, indent=2))
        passed = bool(report["passed"])
        if args.cleanup and passed:
            shutil.rmtree(run_root)
        return 0 if passed else 1
    except Exception as error:  # setup/runtime failures are distinct from score failures
        print(json.dumps({"passed": False, "error": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
