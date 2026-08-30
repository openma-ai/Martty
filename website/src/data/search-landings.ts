export interface ArticleCode {
  language: string;
  source: string;
  href: string;
  body: string;
  explanation: readonly string[];
}

export interface ArticleSection {
  id: string;
  title: string;
  paragraphs: readonly string[];
  code?: ArticleCode;
  commands?: readonly string[];
}

export interface SearchLanding {
  slug: string;
  eyebrow: string;
  title: string;
  metaTitle: string;
  description: string;
  lede: string;
  thesis: string;
  quickAnswer: { title: string; paragraphs: readonly string[]; commands?: readonly string[] };
  sourceRevision: { commit: string; href: string };
  updated: string;
  readingTime: string;
  facts: readonly { label: string; value: string }[];
  diagram: { title: string; body: string; caption: string };
  sourceMap: readonly { path: string; role: string; href: string }[];
  sections: readonly ArticleSection[];
  failureModes: readonly { symptom: string; likelyCause: string; firstCheck: string }[];
  verification: { title: string; commands: readonly string[]; expected: readonly string[] };
  faq: readonly { question: string; answer: string }[];
  related: readonly { href: string; label: string; description: string }[];
}

const SOURCE_REVISION = {
  commit: "d5a0501",
  href: "https://github.com/openma-ai/Martty/tree/d5a0501",
} as const;
const GH = "https://github.com/openma-ai/Martty/blob/d5a0501/";

export const SEARCH_LANDINGS = {
  "deepseek-harness-tui": {
    slug: "deepseek-harness-tui",
    eyebrow: "DeepSeek Harness / process architecture",
    title: "DeepSeek Harness TUI: install it, then trace the two processes",
    metaTitle: "DeepSeek Harness TUI: Install and Architecture | Martty",
    description:
      "Install Martty for DeepSeek Harness, then trace its two-process design: ACP on stdio, TTY on fd 3/4, session updates, plugins, and debugging.",
    lede:
      "Martty connects to DeepSeek Harness through a strict two-process design. DSH owns the Host tree and agent capabilities; a separate Client process owns terminal composition and the Rust painter. The boundary is ACP on standard pipes, with the user's TTY carried separately on fd 3/4.",
    thesis:
      "Martty's primary design claim is testable: `dsh --profile martty` must not import the Harness into the terminal package or recursively start a second agent. The Host runner connects the existing DSH ACP server to an independent TUI Client over that client's standard pipes, while fd 3/4 carry only the user's TTY to the native painter. This split is what makes process ownership, shutdown, compatibility, and plugin authority understandable instead of accidental.",
    quickAnswer: {
      title: "Run the DSH profile in three commands",
      paragraphs: [
        "Install DSH, add Martty to a dedicated profile, and start that profile. DSH remains the Host runtime; Martty is the terminal Client. The command sequence below is the supported path when DSH should own agent plugins, credentials, sessions, and package upgrades.",
        "A successful launch should produce one DSH Host process and one Martty Client process. The Host and Client exchange ACP over the Client's standard pipes, while the native painter receives the user's terminal separately. If the frame opens without an agent, jump to the failure matrix instead of reinstalling the renderer.",
      ],
      commands: [
        "npm install --global @deepseek-ai/dsh",
        "dsh plugin --profile martty add martty@latest",
        "dsh --profile martty",
      ],
    },
    sourceRevision: SOURCE_REVISION,
    updated: "2026-08-29",
    readingTime: "8 min",
    facts: [
      { label: "Composition owner", value: "DSH Host process" },
      { label: "Client boundary", value: "ACP over stdin / stdout" },
      { label: "Terminal path", value: "TTY on fd 3 / 4" },
      { label: "Renderer", value: "Rust + Ratatui" },
    ],
    diagram: {
      title: "The profile path has two independent process trees",
      body: `User terminal
     │ keystrokes / frames
     │ fd 3 / fd 4
     ▼
┌──────────────────────── Martty Client process ────────────────────────┐
│ Cordis Client tree → UI services → semantic snapshots → Rust painter │
│             stdin/stdout are reserved for Host ACP                   │
└───────────────────────────────▲───────────────────────────────────────┘
                                │ initialize, session/*, _dsh/cordis/*
                                │ ACP over Client stdin / stdout
┌───────────────────────────────▼───────────────────────────────────────┐
│ DSH Base tree → ACP server plugin → model, tools, Host plugins       │
└────────────────────────── DSH Host process ───────────────────────────┘`,
      caption:
        "The crossed channels are deliberate. Host ACP never travels on the inherited TTY descriptors, and terminal bytes never enter the Host's ACP parser.",
    },
    sourceMap: [
      { path: "docs/architecture.en.md", role: "Normative Host/Client process contract", href: `${GH}docs/architecture.en.md` },
      { path: "npm/lib/client-process.js", role: "Independent Client entry point and fd mapping", href: `${GH}npm/lib/client-process.js` },
      { path: "npm/lib/spawn-tui.js", role: "Native painter spawn, fd and Windows TCP transports", href: `${GH}npm/lib/spawn-tui.js` },
      { path: "npm/lib/acp-client.js", role: "Spawn-or-stream ACP client service", href: `${GH}npm/lib/acp-client.js` },
      { path: "src/main.rs", role: "Native attach modes and ACP endpoint selection", href: `${GH}src/main.rs` },
      { path: "src/acp.rs", role: "Official ACP client and session projection", href: `${GH}src/acp.rs` },
    ],
    sections: [
      {
        id: "profile-contract",
        title: "Start from process ownership, not from the screen",
        paragraphs: [
          "Running `dsh --profile martty` means DSH remains the composition root. Its Base Cordis tree mounts the agent-facing ACP plugin and Host-side packages. The Martty runner then starts an independent Node Client process. That process builds the terminal-side Cordis tree and launches the native painter. There is no shared dependency-injection container hiding behind the pipe.",
          "This distinction prevents a subtle deployment bug. If the terminal package imported DSH and spawned it again, the user could end up with two different Host trees, two plugin registries, and two notions of the active session. A screen could look healthy while commands or authentication were applied to the wrong process. The profile contract gives every durable fact one owner: DSH owns agent execution; the Client owns presentation state.",
        ],
      },
      {
        id: "crossed-pipes",
        title: "Why ACP and the TTY use crossed channels",
        paragraphs: [
          "The Client process receives Host ACP on its standard input and sends ACP responses on standard output. At the same time, it inherits the user's terminal on descriptors 3 and 4. `client-process.js` intentionally reverses the stream names when it calls `bootClient`: from the Client's perspective, `process.stdin` contains bytes written by the Host, and `process.stdout` returns bytes to the Host; the real terminal is a separate object.",
          "The native Rust painter is then spawned with its own standard input and output attached to that TTY. On Unix, Node and Rust exchange their private compositor protocol over two extra pipes. On Windows, or when `DSH_TUI_FORCE_TCP=1`, the same semantic channel uses token-authenticated loopback TCP. Neither implementation gives a theme or slot plugin a terminal handle; only the trusted painter controls raw mode and escape sequences.",
        ],
        code: {
          language: "js",
          source: "npm/lib/client-process.js",
          href: `${GH}npm/lib/client-process.js`,
          body: `await bootClient({
  // ACP is connected to the Host runner through this process's stdio.
  stream: { stdin: process.stdout, stdout: process.stdin },
  extraArgs: process.argv.slice(2),

  // The user terminal is inherited separately. These descriptors never
  // carry Host-to-Client ACP messages.
  tty: { stdin: 3, stdout: 4 },
  packagePlugins: parseClientPluginsEnv(
    process.env.DSH_TUI_CLIENT_PLUGINS_V0,
  ),
})`,
          explanation: [
            "`stream` is the ACP connection already owned by the Host runner; the Client does not spawn another Harness on this path.",
            "`tty` is capability-minimized input/output for the trusted terminal shell. Plugin code receives semantic services instead of these descriptors.",
          ],
        },
      },
      {
        id: "initialize",
        title: "Initialization is the compatibility gate",
        paragraphs: [
          "The first successful paint proves only that the native binary and terminal setup work. The first protocol proof is ACP `initialize`. Martty sends its implementation identity and declares filesystem, terminal, session-configuration, authentication, and elicitation capabilities. The agent response determines whether session loading, image prompts, auth methods, and optional DSH features are actually available.",
          "DSH-specific methods are gated by `_meta.dsh.cordis.protocol = 0`. The Rust client records that bit in its `Surface`; calls such as plugin listing or UI selection first pass `ensure_agent_cordis`. A plain ACP agent can therefore run sessions without pretending to understand DSH extensions. An unsupported extension is a disabled feature, not a reason to corrupt the base client connection.",
        ],
      },
      {
        id: "session-projection",
        title: "The transcript comes from session/update, never from Host internals",
        paragraphs: [
          "After initialization and authentication, Martty creates or loads an ACP session. Prompts, cancellation, permission requests, configuration changes, and agent updates all cross the standard protocol. In `src/acp.rs`, each typed `SessionNotification` is serialized into an internal `AppEvent::Rpc` with method `session/update`. The application layer folds that event into transcript nodes, command catalogs, mode choices, usage, and plan state.",
          "That extra projection layer is essential for a long-running agent. A tool card can begin as pending and later be replaced in place; a partial assistant message can grow without duplicating earlier text; resize can reflow the same state. If the Host wrote terminal lines directly, scrollback and update ordering would become transport artifacts. ACP events describe state changes; the painter decides how that state occupies cells.",
        ],
      },
      {
        id: "plugin-boundary",
        title: "Host plugins and Client plugins are different products",
        paragraphs: [
          "A Host plugin changes agent-side behavior and must project anything user-visible through ACP. A Client plugin changes presentation or interaction on the terminal side. Martty does not synchronize plugin ids, Cordis `inject` declarations, or fibers between the two processes. Doing so would turn a serialized protocol boundary back into a distributed in-memory framework with undefined lifecycle rules.",
          "Themes and shell slots are registered as sibling Client plugins. They contribute palettes or validated `TuiNode` trees through services such as `tuiTheme` and `tuiSlots`; Node serializes snapshots to the Rust painter. Conversation content is intentionally not an open slot. The agent remains the source of durable transcript updates, so a decorative plugin cannot impersonate a user, tool, or assistant event.",
        ],
      },
      {
        id: "shutdown",
        title: "Shutdown is part of the architecture",
        paragraphs: [
          "A full-screen terminal changes raw mode, alternate-screen state, mouse capture, and cursor visibility. Leaving an orphan painter behind is not a cosmetic failure: it can keep the TTY unusable after the Host exits. `client-process.js` converts SIGTERM, SIGINT, and SIGHUP into orderly `process.exit` calls so the Client's exit handlers tear down the native child and restore the terminal.",
          "The reverse direction also matters. The shell remembers when the user intentionally quits and refuses to respawn an empty TUI on the same terminal. Spawned ACP agents are terminated when their specification changes or the client exits. These ownership rules make Ctrl+C, profile reload, and failed native startup observable rather than producing invisible background processes.",
        ],
      },
      {
        id: "debug-order",
        title: "Debug from the outer process inward",
        paragraphs: [
          "An empty timeline is not one failure category. It can mean the profile did not load, the Client failed before spawning the painter, ACP initialization never completed, authentication blocked session creation, or updates reached the client but failed to project. Debugging from pixels backward mixes all five layers and usually leads to changing CSS or model configuration without evidence.",
          "Use the ownership chain as the diagnostic order. First confirm the profile package graph. Then prove both processes exist and the painter can attach. Next prove ACP initialization and inspect the advertised auth methods. Only after `session/new` succeeds should you investigate model routes, permissions, or a missing `session/update`. This order narrows the fault without assuming that a visible frame means a usable agent.",
        ],
      },
      {
        id: "standalone",
        title: "Standalone Martty changes the connection owner, not the protocol",
        paragraphs: [
          "The `martty` executable can also be the client composition root. In that mode it may spawn `dsh-acp`, run `dsh --profile acp`, launch another ACP-compatible command, or attach to caller-provided streams. The executable and argument list are explicit data; repeated `--agent-arg` flags avoid asking a shell to reinterpret a single command string.",
          "Use standalone mode for protocol testing, non-DSH agents, or a parent application that already owns the server pipes. Use the profile when DSH should own Host packages and upgrades. Both routes converge on the same ACP session model, so switching launch topology should not require a second transcript implementation.",
        ],
        commands: [
          "npm install --global martty",
          "martty --check-runtime",
          "martty --agent <acp-command> --agent-arg <argument>",
        ],
      },
    ],
    failureModes: [
      { symptom: "The terminal opens, but no agent name appears", likelyCause: "The painter is alive but ACP initialize is pending or failed", firstCheck: "Run `martty --check-runtime` and inspect the agent stderr" },
      { symptom: "The profile starts two agent runtimes", likelyCause: "The Client was configured to spawn instead of receiving the Host stream", firstCheck: "Inspect the runner config and confirm `config.stream` reaches the Client" },
      { symptom: "Theme or slot actions work, but prompts do not", likelyCause: "The private compositor channel is healthy while Host ACP is broken", firstCheck: "Trace initialize and session/new on Client stdin/stdout" },
      { symptom: "Prompts work, but DSH plugin controls are absent", likelyCause: "The agent did not negotiate Cordis protocol 0", firstCheck: "Inspect `agentCapabilities._meta.dsh.cordis.protocol`" },
      { symptom: "The shell is corrupted after Ctrl+C", likelyCause: "The painter outlived the Client or skipped terminal restoration", firstCheck: "Verify signal handlers and native child exit cleanup" },
    ],
    verification: {
      title: "Reproduce the profile and protocol boundary",
      commands: ["dsh plugin --profile martty list", "dsh --profile martty", "martty --check-runtime"],
      expected: [
        "The profile list contains `martty`, and starting the profile creates one DSH Host plus one TUI Client—not a second Harness tree.",
        "The runtime check prints `initialize ok ... → <agent-name>`; a live prompt produces `session/update` traffic before transcript paint changes.",
        "On Unix, the Host ACP path is Client stdin/stdout while the TTY is fd 3/4. Closing either process restores the terminal and terminates its owned child.",
      ],
    },
    faq: [
      { question: "Is Martty a fork of DeepSeek Harness?", answer: "No. DSH remains the Host runtime. Martty is a separate ACP client composition and native terminal renderer." },
      { question: "Does `dsh --profile martty` start another Harness inside the TUI?", answer: "No. The Host runner passes its ACP stream to the Client. Spawn mode is for standalone connections, not the primary profile topology." },
      { question: "Why not pass the TTY through ACP?", answer: "ACP carries semantic session operations. Raw terminal bytes have different security, lifecycle, and backpressure rules, so Martty keeps them on a private trusted channel." },
      { question: "Can Martty still connect to a non-DSH agent?", answer: "Yes. Standard ACP is the base contract. DSH Cordis features appear only after protocol-0 capability negotiation." },
    ],
    related: [
      { href: "/acp-terminal-client", label: "ACP terminal client", description: "The protocol state machine behind the process split." },
      { href: "/ratatui-agent-tui", label: "Ratatui Agent TUI", description: "How semantic state becomes a terminal frame." },
      { href: "/docs/architecture", label: "Architecture reference", description: "The repository's normative Host, Client, and painter contract." },
    ],
  },

  "acp-terminal-client": {
    slug: "acp-terminal-client",
    eyebrow: "Agent Client Protocol / state-machine engineering",
    title: "ACP terminal client architecture: transport, sessions, rendering",
    metaTitle: "ACP Terminal Client Architecture and Sessions | Martty",
    description:
      "Build an ACP terminal client with spawn or stream ownership, initialize gating, session/update projection, authentication, cancellation, and extensions.",
    lede:
      "Martty models an ACP terminal client as three layers: an owned connection, a session-state projection, and a snapshot renderer. That client must negotiate capabilities, create or load sessions, handle authentication and permissions, merge partial updates, and cancel work without coupling the screen to transport order.",
    thesis:
      "The reliable architecture has three explicit layers: a typed ACP connection, a client-owned session projection, and a renderer that consumes snapshots. Standard ACP controls the lifecycle; vendor extensions are enabled only by initialize metadata. This separation lets Martty attach to DeepSeek Harness or another ACP agent without letting transport order, executable names, or server-private objects leak into the terminal model.",
    quickAnswer: {
      title: "Keep transport, session state, and rendering separate",
      paragraphs: [
        "An ACP terminal client should treat the agent connection as an owned resource, not a stream of lines to print. Spawn mode owns a child process; attach mode borrows caller-provided streams. Both modes feed the same typed initialize, authentication, session, prompt, cancellation, and update lifecycle.",
        "Project protocol updates into client state before rendering them. Stable transcript nodes let a pending tool call become a result in place, while separate projections hold plans, configuration, usage, and authentication. Gate every vendor method on initialize metadata so a plain ACP agent keeps the complete standard session path.",
      ],
    },
    sourceRevision: SOURCE_REVISION,
    updated: "2026-08-29",
    readingTime: "8 min",
    facts: [
      { label: "Wire contract", value: "ACP v1 messages" },
      { label: "Connection modes", value: "Spawn or caller stream" },
      { label: "State source", value: "session/update" },
      { label: "Extension rule", value: "Negotiate, then gate" },
    ],
    diagram: {
      title: "One message crosses three representations",
      body: `ACP agent
   │ typed notification: session/update
   ▼
┌──────────────── protocol connection ────────────────┐
│ validates message shape and session ownership      │
└────────────────────────┬────────────────────────────┘
                         ▼
┌──────────────── client projection ─────────────────┐
│ transcript nodes · plan · config · auth · usage    │
└────────────────────────┬────────────────────────────┘
                         ▼ immutable render snapshot
┌──────────────── terminal renderer ─────────────────┐
│ layout · clipping · scroll · focus · current frame │
└─────────────────────────────────────────────────────┘`,
      caption:
        "A source typed value, an internal event, and a rendered cell are related but not interchangeable. The projection layer is where retries, replacement, and session scoping become deterministic.",
    },
    sourceMap: [
      { path: "npm/lib/acp-client.js", role: "Cordis ACP client service and connection ownership", href: `${GH}npm/lib/acp-client.js` },
      { path: "npm/lib/cordis-protocol.js", role: "Extension names and capability reader", href: `${GH}npm/lib/cordis-protocol.js` },
      { path: "src/acp.rs", role: "Typed ACP lifecycle and request handlers", href: `${GH}src/acp.rs` },
      { path: "src/acp_auth.rs", role: "Form and terminal authentication state", href: `${GH}src/acp_auth.rs` },
      { path: "src/events.rs", role: "Protocol update to UI-event projection", href: `${GH}src/events.rs` },
      { path: "scripts/acp-client.test.mjs", role: "Spawn, stream, and lifecycle contract tests", href: `${GH}scripts/acp-client.test.mjs` },
    ],
    sections: [
      {
        id: "connection-ownership",
        title: "Make connection ownership a data type",
        paragraphs: [
          "Martty's Node ACP plugin accepts either `config.stream` or an agent specification. Stream mode means the caller owns the process and hands the client readable and writable endpoints. Spawn mode means the client owns a child command, its arguments, stderr, shutdown, and replacement. Treating these as separate service kinds prevents both sides from trying to kill the same process—or neither side cleaning it up.",
          "The default executable is `dsh-acp`, but that is policy at the edge, not an import dependency. `resolveAgent` can read explicit configuration or `DSH_TUI_AGENT`; the plugin itself imports no Harness. If the agent specification changes, the old owned child receives SIGTERM before a new one is spawned. If spawn emits ENOENT or EACCES, stream errors are surfaced so pending requests fail instead of hanging forever.",
        ],
        code: {
          language: "js",
          source: "npm/lib/acp-client.js",
          href: `${GH}npm/lib/acp-client.js`,
          body: `if (config.stream !== undefined && config.stream !== null) {
  provide(ctx, {
    kind: 'stream',
    stdin: config.stream.stdin,
    stdout: config.stream.stdout,
    child: config.stream.child,
  })
  return
}

const agent = resolveAgent(config)
const child = spawn(agent.command, agent.args ?? [], {
  stdio: ['pipe', 'pipe', 'inherit'],
  env: { ...process.env, ...(agent.env ?? {}) },
})

provide(ctx, {
  kind: 'spawn',
  command: agent.command,
  args: agent.args ?? [],
  stdin: child.stdin,
  stdout: child.stdout,
  child,
})`,
          explanation: [
            "Both branches expose the same semantic service, but `kind` records who owns lifetime and restart behavior.",
            "stderr is inherited rather than mixed into stdout because stdout must remain a clean ACP transport.",
          ],
        },
      },
      {
        id: "initialize-contract",
        title: "Initialize is a contract, not a greeting",
        paragraphs: [
          "Martty's Rust client builds an official `InitializeRequest` with implementation name and version, filesystem read/write support, terminal operations, session config options, terminal authentication, and form elicitation. The response is converted into a client `Surface`: image-prompt support, load-session support, model and mode catalogs, auth methods, and the negotiated Cordis bit all originate here or in later standard updates.",
          "Do not infer capabilities from the command name. A binary named `dsh-acp` can change its feature set, and another agent may implement more ACP features than expected. A button, slash command, or extension request is legal only when the current connection advertised the required capability. Unknown metadata should remain forward-compatible; a known extension with the wrong protocol version should remain disabled.",
        ],
      },
      {
        id: "session-state",
        title: "A session is more than a transcript array",
        paragraphs: [
          "Creating a session establishes the workspace, configuration surface, and identity used by later prompts, cancellation, permissions, and updates. Loading a session is a different capability and may reconstruct its transcript through subsequent notifications. Martty therefore keeps session identity and connection state outside the scrollback nodes rather than treating the latest visible line as proof of the active server session.",
          "The client projects `session/update` into several bounded views. Available commands become the skill or command catalog. Config-option updates become model, effort, preset, and mode selectors. Plan updates feed a plan service. Usage and timing feed statistics. Transcript nodes keep stable ids so a running tool call or streaming assistant message can be replaced rather than appended as a duplicate.",
        ],
      },
      {
        id: "request-ordering",
        title: "Requests and notifications have different timing obligations",
        paragraphs: [
          "A prompt request can remain pending while dozens of session notifications arrive. Martty runs prompt work in a Tokio task so the ACP dispatch loop continues painting updates and answering agent-initiated requests. Cancellation sends `session/cancel` immediately and does not wait for the original prompt promise to settle. The eventual prompt result is still observed so the client can classify completion, cancellation, authentication failure, or transport loss correctly.",
          "Tool calls also expose a frame-order problem. A fast tool can emit start and completion in the same receive burst. If the renderer draws only after draining the queue, the pending state never appears. Martty marks tool-call transitions as requiring an immediate frame, stops the current receive burst, paints once, and then consumes the result. This is a UI guarantee built on protocol semantics, not an arbitrary animation delay.",
        ],
      },
      {
        id: "auth-permission",
        title: "Authentication and permission are protocol subflows",
        paragraphs: [
          "The initialize response may advertise environment-backed methods, in-app forms, or terminal authentication. A form-capable method can already have persistent credentials, so startup remains optimistic until `session/new` proves otherwise. When authentication is required, the client parks the unsent prompt, runs the chosen flow, calls `authenticate` again to confirm state, then retries the parked prompt only after success.",
          "Terminal auth is deliberately explicit. `_meta[\"terminal-auth\"]` tells the agent that the method needs an out-of-band child attached to the real TTY. Permission requests are agent-to-client ACP requests tied to a tool call and active session; the client renders the options, returns the selected outcome through the responder, and distinguishes denial from cancellation or agent exit. A local keybinding is never a substitute for that response.",
        ],
      },
      {
        id: "extension-gating",
        title: "Optional extensions must degrade to nothing",
        paragraphs: [
          "Martty and DSH negotiate `_meta.dsh.cordis.protocol = 0`. Only then may the client send inspect, run, plugin, or TUI child-domain methods. The JavaScript reader walks the initialize result defensively and returns either `{ protocol: 0 }` or `null`. The Rust side repeats the check before user-triggered extension operations and reports a UI failure instead of emitting an unsupported request.",
          "This design makes extension absence boring. Core ACP sessions, prompts, updates, cancellation, auth, permissions, and filesystem requests continue. DSH-only plugin controls or semantic shell nodes simply do not appear. That is a stronger compatibility property than catching `method not found` after optimistic calls, because it keeps server logs clean and prevents a partial vendor feature from mutating client state.",
        ],
      },
      {
        id: "renderer-boundary",
        title: "Project state before it reaches pixels",
        paragraphs: [
          "ACP delivers domain facts: content blocks, tool lifecycle, plan entries, usage, config options, and requests. The client converts them into internal events and services. The renderer receives a snapshot and terminal dimensions. This means a resize can recompute wrapping without replaying the network stream, and a restored session can paint the same result even if updates arrived in different batches.",
          "The boundary also contains authority. Server extensions can send validated semantic UI nodes, but they do not receive Ratatui objects, raw-mode access, coordinates, or the global frame loop. Terminal chrome is not encoded inside `session/update`, and Client plugins cannot fabricate durable assistant events. Each side can evolve as long as the serialized schema remains compatible.",
        ],
      },
      {
        id: "test-harness",
        title: "Test the state machine without opening a terminal",
        paragraphs: [
          "Connection and projection logic should be testable with in-memory streams and fixture agents. Spawn tests verify command selection, child reuse, replacement, error handling, and cleanup. Protocol tests feed initialize results with and without capabilities, interleave prompt responses with notifications, and assert that optional methods are gated. Renderer snapshots are a later layer, not the only integration test.",
          "For a live smoke test, `martty --check-runtime` spawns the configured agent, sends initialize, prints the negotiated name, and exits before entering full-screen mode. It isolates binary discovery, process spawn, pipe integrity, and initialization from session and rendering failures. Only after that passes should a test create a session and assert visible update projection.",
        ],
        commands: ["npm install --global martty", "martty --check-runtime", "martty --agent <acp-command> --agent-arg <argument>"],
      },
    ],
    failureModes: [
      { symptom: "A request hangs forever after spawn failure", likelyCause: "The child emitted `error`, but pending stream readers never saw EOF", firstCheck: "Verify child error handlers destroy stdin/stdout and clear the cached handle" },
      { symptom: "Old tool cards are duplicated on every update", likelyCause: "Notifications are printed directly instead of replacing projected nodes by id", firstCheck: "Inspect the session/update reducer and node identity" },
      { symptom: "Cancel freezes until the model returns", likelyCause: "The client awaits the prompt promise before sending session/cancel", firstCheck: "Send cancellation as an independent notification and observe the pending task separately" },
      { symptom: "Vendor controls fail against a plain ACP agent", likelyCause: "The client inferred extensions from the executable name", firstCheck: "Inspect initialize metadata and gate on Cordis protocol 0" },
      { symptom: "Login succeeds but the parked prompt disappears", likelyCause: "Authentication state and pending composer payload were not modeled together", firstCheck: "Trace authenticate completion and the parked-prompt retry path" },
    ],
    verification: {
      title: "Verify transport, capability, and session layers separately",
      commands: ["node --test scripts/acp-client.test.mjs", "martty --check-runtime", "martty --agent <acp-command> --agent-arg <argument>"],
      expected: [
        "Spawn and stream modes expose the same ACP service while retaining different lifetime ownership.",
        "Initialize succeeds before a session is created; optional DSH controls appear only when protocol 0 is advertised.",
        "During a live prompt, session/update can repaint transcript state while the prompt request remains pending, and cancel does not wait for prompt completion.",
      ],
    },
    faq: [
      { question: "What is the difference between an ACP client and agent?", answer: "The client owns interaction, local state projection, and agent-directed requests. The agent owns model execution and tools. ACP is their only required shared contract." },
      { question: "Why not render each JSON message immediately?", answer: "Messages describe transitions, not final layout. Projection is required for replacement, session scoping, resize, scroll, and deterministic replay." },
      { question: "Does an ACP terminal client need DSH?", answer: "No. Martty defaults to DSH, but standard ACP works without DSH. Cordis extensions are optional and capability-gated." },
      { question: "Why are stderr and stdout treated differently?", answer: "stdout carries protocol frames and must stay parseable. Human diagnostics belong on stderr so they cannot corrupt the ACP stream." },
    ],
    related: [
      { href: "/deepseek-harness-tui", label: "DeepSeek Harness TUI", description: "A concrete two-process ACP deployment." },
      { href: "/ratatui-agent-tui", label: "Ratatui Agent TUI", description: "The renderer on the far side of the projection." },
      { href: "/docs/plugins", label: "Plugin API reference", description: "What negotiated Client plugins may observe and change." },
    ],
  },

  "ratatui-agent-tui": {
    slug: "ratatui-agent-tui",
    eyebrow: "Rust / Ratatui / long-running agent interfaces",
    title: "Ratatui agent TUI: state, Unicode, and plugin boundaries",
    metaTitle: "Ratatui Agent TUI: State and Unicode Layout | Martty",
    description:
      "Build a Ratatui agent TUI around stable state: mutable tool nodes, Unicode composer layout, scroll anchoring, input routing, and semantic plugins.",
    lede:
      "Martty uses Rust and Ratatui for terminal mechanics while keeping agent, protocol, and plugin authority outside the renderer. Long turns stream into mutable tool nodes, permissions interrupt input, Markdown reflows on resize, and the composer maps Unicode graphemes to terminal cells instead of byte offsets.",
    thesis:
      "Ratatui is valuable here because it makes every frame an explicit function of application state and terminal dimensions. It is not a reason to move agent execution, plugin loading, or protocol ownership into Rust. Martty's renderer owns raw mode, layout, focus, hit testing, clipping, and cell output; ACP and Cordis layers deliver validated semantic state. That boundary is what keeps a responsive TUI from becoming a second agent runtime.",
    quickAnswer: {
      title: "Give Ratatui snapshots, not agent responsibilities",
      paragraphs: [
        "Keep the agent and protocol outside the render pass. Normalize incoming updates into application state, then let Ratatui measure and paint a complete frame from that state and the current terminal dimensions. Tool calls, permissions, overlays, composer text, and scroll anchors become explicit nodes instead of irreversible terminal output.",
        "Use grapheme clusters and display-cell widths for composer layout, and route input through the most specific active surface before global shortcuts. Plugins should contribute validated semantic nodes; the Rust renderer alone owns `Frame`, `Rect`, raw mode, cursor state, clipping, and terminal output.",
      ],
    },
    sourceRevision: SOURCE_REVISION,
    updated: "2026-08-29",
    readingTime: "8 min",
    facts: [
      { label: "Frame model", value: "Snapshot → cells" },
      { label: "Composer", value: "ratatui-textarea + LayoutMap" },
      { label: "Text width", value: "Unicode display cells" },
      { label: "Plugin input", value: "Validated TuiNode trees" },
    ],
    diagram: {
      title: "The renderer is the last stage, not the composition root",
      body: `ACP updates + Client plugin snapshots
                 │ semantic, validated data
                 ▼
┌──────────────── App state ────────────────┐
│ transcript · composer · overlay · docks  │
│ focus · selection · scroll anchors       │
└────────────────────┬──────────────────────┘
                     │ snapshot + Rect
                     ▼
┌────────────── Ratatui render pass ──────────────┐
│ measure → allocate → wrap → clip → paint cells │
└────────────────────┬────────────────────────────┘
                     ▼
              terminal back buffer`,
      caption:
        "Input handlers mutate application state. Rendering reads that state and produces a complete frame; it does not call the agent or append transport messages during layout.",
    },
    sourceMap: [
      { path: "src/input/composer.rs", role: "Textarea adapter and Unicode cell-to-character map", href: `${GH}src/input/composer.rs` },
      { path: "src/app.rs", role: "Application state, input routing, scroll, and overlays", href: `${GH}src/app.rs` },
      { path: "src/ui.rs", role: "Frame layout and semantic surface rendering", href: `${GH}src/ui.rs` },
      { path: "src/events.rs", role: "ACP update normalization into UI events", href: `${GH}src/events.rs` },
      { path: "src/cordis.rs", role: "Private compositor method constants", href: `${GH}src/cordis.rs` },
      { path: "docs/composer-input.md", role: "Composer behavior and verification contract", href: `${GH}docs/composer-input.md` },
    ],
    sections: [
      {
        id: "frame-model",
        title: "Render complete frames from stable state",
        paragraphs: [
          "Ratatui writes into a back buffer and diffs it against the previous frame. That model fits an agent UI only if the application owns stable semantic state. Transcript nodes, running tools, permission cards, the composer, overlays, navigation docks, and status are updated first; the render pass then measures and paints all visible regions from a snapshot.",
          "The alternative—printing transport fragments as they arrive—cannot reflow on resize, replace a pending card, or keep scroll position anchored. It also makes tests depend on event batching. Martty's event loop can drain several updates before a normal frame, but it explicitly interrupts that drain for transitions whose intermediate visual state matters, such as the beginning of a tool call.",
        ],
      },
      {
        id: "tool-lifecycle",
        title: "Tool calls are mutable nodes with a frame guarantee",
        paragraphs: [
          "A tool call is not three log lines labeled start, output, and done. It is one node whose status, body, and timing change. Stable identity lets the reducer update that node in place and lets collapsed or expanded state survive later results. The transcript therefore remains a model of the agent turn rather than a chronology of wire packets.",
          "Fast tools reveal why frame scheduling belongs in the application contract. Start and completion may arrive in one receive burst. Martty's `event_requires_immediate_frame` detects a tool-call transition in either the typed UI event or ACP `session/update`, stops draining, and renders the pending card once. Completion can then replace it in the next frame without an artificial timeout.",
        ],
        code: {
          language: "rust",
          source: "src/main.rs",
          href: `${GH}src/main.rs`,
          body: `fn event_requires_immediate_frame(event: &AppEvent) -> bool {
    match event {
        AppEvent::Ui(events::UiEvent::ToolCall { .. }) => true,
        AppEvent::Rpc { method, params } if method == "session/update" => {
            params
                .get("update")
                .unwrap_or(params)
                .get("sessionUpdate")
                .and_then(serde_json::Value::as_str)
                == Some("tool_call")
        }
        AppEvent::Rpc { method, params } if method == "session.event" => {
            params
                .get("event")
                .unwrap_or(params)
                .get("type")
                .and_then(serde_json::Value::as_str)
                == Some("tool/call")
        }
        _ => false,
    }
}`,
          explanation: [
            "The predicate recognizes both the current ACP shape and a compatibility event shape, but both become the same frame-order guarantee.",
            "It does not sleep or animate. It changes queue-draining policy so the semantic pending state is observable for one real frame.",
          ],
        },
      },
      {
        id: "composer",
        title: "The composer needs an editor model and a layout model",
        paragraphs: [
          "Martty delegates editing operations to `ratatui-textarea`: insertion, cursor movement, selection, undo-friendly buffer semantics, and glyph wrapping. The widget does not expose the full screen-cell map required for mouse hit testing and drag selection, so Martty builds a `LayoutMap` using the same wrap rules. That is a deliberate adapter, not a second editor implementation.",
          "`LayoutMap` records each soft-wrapped row and every grapheme's character range, starting display column, and display width. A click on the left half of a wide grapheme maps before it; the right half maps after it. Blank cells snap to row end, and rows beyond the buffer snap to total character count. The map is invalidated whenever the textarea mutates and rebuilt for the current wrap width.",
        ],
      },
      {
        id: "unicode",
        title: "Bytes, characters, graphemes, and cells are four different units",
        paragraphs: [
          "Rust string indices are byte offsets, editor cursors commonly use character offsets, user-perceived clusters are graphemes, and terminal layout uses display cells. ASCII hides the difference. CJK characters normally occupy two cells; combining marks may add characters without adding width; emoji sequences can contain several code points; tabs advance to the next tab stop instead of having a fixed width.",
          "Martty iterates grapheme clusters with `unicode-segmentation` and computes cell width with `unicode-width`. Wrapping happens before a grapheme that would overflow the row, except that a grapheme wider than the entire row stands alone. Tests cover CJK, emoji, combining marks, tabs, narrow widths, midpoint hit testing, and drag endpoints. Without those cases, mouse selection can appear correct in English while slicing or jumping inside real user text.",
        ],
      },
      {
        id: "scroll-resize",
        title: "Scroll position must be anchored to content, not stale rows",
        paragraphs: [
          "Long agent turns continuously change document height. If the user is following the tail, new content should keep the viewport at the bottom. If the user has scrolled upward to inspect an earlier tool, new updates should not steal the viewport. Martty models that distinction explicitly instead of always setting scroll to the latest row after every event.",
          "Resize makes raw row offsets unstable because Markdown, code, and wide glyphs rewrap. The renderer measures content for the new width, clamps offsets, and preserves the user's semantic position as closely as possible. Overlays and docks are allocated before transcript width is finalized, so opening a right navigation rail can reflow the conversation without writing over it.",
        ],
      },
      {
        id: "input-routing",
        title: "Input routing is a priority stack",
        paragraphs: [
          "The same key can mean different things depending on state. Escape may close an overlay, clear a selection, leave a menu, cancel a running turn, or do nothing. Enter may confirm a permission, choose a completion, insert a newline, or submit a prompt. Handling keys as a flat match table causes background surfaces to react underneath a modal.",
          "Martty routes input from the most specific active surface outward: terminal-auth handoff, approvals, overlays and menus, composer selection and completion, transcript navigation, then global commands. Mouse coordinates are tested against the rectangles produced by the latest frame. This makes focus and hit testing products of layout state rather than duplicated constants in event handlers.",
        ],
      },
      {
        id: "plugin-surface",
        title: "Plugins contribute semantic nodes, never Ratatui widgets",
        paragraphs: [
          "Martty's Client plugins run in Node and inject services such as `tuiTheme`, `tuiSlots`, `tuiCommands`, and `tuiOverlay`. A slot contribution is a validated `TuiNode` tree with text, grouping, emphasis, and actions. The Rust side receives monotonic-revision snapshots and decides how those nodes fit the terminal. The plugin does not receive the `Frame`, `Rect`, terminal size, or raw-mode handle.",
          "This is more restrictive than exposing a widget trait, and that is the point. A JavaScript package can add a right rail or composer dock without controlling cursor state, escape sequences, or the global render loop. Conversation slots remain closed so plugins cannot fabricate durable session history. The transport methods in `src/cordis.rs` are compositor-private plumbing, not a public imperative drawing API.",
        ],
      },
      {
        id: "ratatouille",
        title: "Ratatui is the crate; Ratatouille is the typo",
        paragraphs: [
          "The Rust terminal UI library is Ratatui. Search queries sometimes contain “Ratatouille,” the film title and a common autocorrect result. Martty does not use a separate Ratatouille UI framework. The relevant dependencies and APIs are Ratatui plus supporting crates such as `ratatui-textarea`, `unicode-segmentation`, and `unicode-width`.",
          "The naming correction matters when debugging. Searching crate documentation or compiler errors for Ratatouille produces unrelated results, while Ratatui documentation explains buffers, layouts, widgets, and terminal backends. In this codebase, the higher-level design still belongs to Martty: Ratatui provides rendering primitives, not ACP sessions or plugin lifecycle.",
        ],
      },
      {
        id: "verification",
        title: "Verify semantics before visual polish",
        paragraphs: [
          "Unit tests should first prove state transitions and layout math: tool replacement, immediate frames, cancellation, Unicode mapping, history, selection, and resize clamping. Snapshot or frame-dump tests then prove that the same state renders correctly at known terminal sizes. A live terminal test is still necessary for raw mode, mouse capture, clipboard behavior, and shutdown restoration.",
          "Martty exposes `--dump-frame WIDTHxHEIGHT` for deterministic noninteractive rendering and `--demo` for scripted terminal behavior. The final acceptance is a real ACP session: submit a prompt, observe a pending tool frame, resize during streaming, scroll away from the tail, answer a permission request, and exit. A green Rust build alone cannot prove those interactions.",
        ],
        commands: ["cargo test --locked", "martty --dump-frame 120x36", "martty --demo"],
      },
    ],
    failureModes: [
      { symptom: "A fast tool appears only after completion", likelyCause: "The event loop drains start and result before rendering", firstCheck: "Trace `event_requires_immediate_frame` and frame scheduling" },
      { symptom: "Clicking after a CJK glyph moves the cursor backward", likelyCause: "Hit testing uses character count instead of display-cell width", firstCheck: "Run LayoutMap CJK and wide-grapheme midpoint tests" },
      { symptom: "Streaming output snaps the user back to the bottom", likelyCause: "Scroll state does not distinguish follow-tail from manual inspection", firstCheck: "Inspect the anchor flag before applying transcript growth" },
      { symptom: "A plugin corrupts the terminal or overlaps the composer", likelyCause: "The plugin received imperative drawing or terminal coordinates", firstCheck: "Require validated TuiNode snapshots and keep layout in Rust" },
      { symptom: "Resize duplicates or loses transcript text", likelyCause: "Rendered rows were stored as source state", firstCheck: "Rebuild rows from semantic nodes at the new width" },
    ],
    verification: {
      title: "Exercise the renderer at three levels",
      commands: ["cargo test --locked composer", "martty --dump-frame 120x36", "martty --demo"],
      expected: [
        "Composer tests preserve grapheme boundaries and map CJK, emoji, combining marks, tabs, and narrow rows to correct character offsets.",
        "Frame dumps are deterministic at fixed dimensions and do not write outside allocated regions when docks or overlays are present.",
        "A live run shows a pending tool state before completion, preserves manual scroll during streaming, reflows on resize, and restores the terminal on exit.",
      ],
    },
    faq: [
      { question: "Why use Rust and Ratatui for an agent TUI?", answer: "They provide explicit frame rendering, predictable memory and process behavior, strong Unicode tooling, and direct control over terminal lifecycle. Agent and plugin ownership can still remain outside Rust." },
      { question: "Is Ratatouille a Rust TUI framework?", answer: "No. The crate is Ratatui. Ratatouille is a common typo or reference to the film." },
      { question: "Why not let plugins return Ratatui widgets?", answer: "That would couple untrusted package code to renderer internals and terminal authority. Semantic node snapshots preserve validation, layout ownership, and cross-process compatibility." },
      { question: "Why mirror the textarea layout?", answer: "The editor owns text mutation, but Martty also needs cell-accurate mouse hit testing. LayoutMap mirrors the widget's glyph-wrap rules without reimplementing editing behavior." },
    ],
    related: [
      { href: "/acp-terminal-client", label: "ACP terminal client", description: "The state machine that supplies renderer snapshots." },
      { href: "/deepseek-harness-tui", label: "DeepSeek Harness TUI", description: "The complete Host, Client, and painter topology." },
      { href: "/docs/plugins", label: "Plugin API reference", description: "The semantic nodes and services exposed to packages." },
    ],
  },
} satisfies Record<string, SearchLanding>;
