# PromptGuard CLI

The `promptguard` binary. It instruments a developer's codebase so its LLM
traffic passes through PromptGuard, and it is the command-line front end to the
PromptGuard API for inspecting content and managing a project's guardrails.

Two sides meet in this repo, and most naming confusion comes from crossing
them: the **codebase** (local, static, offline) and the **project** (remote,
account-scoped, reached over the API). Terms below are grouped accordingly.

## Language

### The codebase side

**Codebase**:
The local directory tree the CLI instruments. Its root is the directory holding
the repo config.
_Avoid_: project (that is the remote thing), workspace (means a Cargo workspace
in this repo), target directory

**Repo config**:
`.promptguard.json` at the codebase root. Committed, so it is attacker-supplied
when a codebase is cloned from someone else.
_Avoid_: project config, local config, project-local config

**Global credentials**:
`~/.promptguard/credentials.json`. Holds the API key and which project is
active. Never committed; owner-only permissions.
_Avoid_: global config, credentials file

**Provider**:
An LLM vendor whose SDK the CLI recognises: OpenAI, Anthropic, Cohere,
HuggingFace, Gemini, Groq, Bedrock.
_Avoid_: vendor, model, SDK (when the vendor is meant)

**Call site**:
A source location that constructs a provider's SDK client. What detection
finds and what a transform edits.
_Avoid_: instance, usage, occurrence, detection (as a noun for the location)

**Unroutable call site**:
A call site left untouched on purpose because its arguments are dynamic
(`**cfg` in Python, a spread or identifier in TypeScript), where injecting a
base URL could collide with what those arguments already carry. Reaching it
needs a hand edit; commands must report these rather than imply the file was
fully handled.
_Avoid_: skipped call site, failed call site, unsupported call site

**Detection**:
The offline pass that parses source into an AST and finds call sites. Never
touches the network.
_Avoid_: scan (see **Threat scan**), static analysis, code analysis

**Transform**:
An edit the CLI makes to a codebase file — injecting a base URL at a call site.
Every transform is backed by a backup.
_Avoid_: patch, fix, auto-fix, rewrite

**Backup**:
The copy of an original file, recorded by path in the repo config. Only
recorded backups are ever restored; the tree is never globbed for `*.bak`,
because that would clobber backups the developer made for their own reasons.

### Interception

**Interception**:
Routing a codebase's LLM traffic through PromptGuard. Exactly two modes exist
and a codebase is in one or the other, never both.

**Static transform mode**:
Interception performed before the app runs, by transforming each call site's
base URL in source. The default. Cannot reach unroutable call sites.
_Avoid_: proxy mode — both modes route through the proxy, so it names the one
property the modes share rather than what separates them. Also avoid: static
mode, transform mode.

**Runtime shim mode**:
Interception performed at process start, by importing a generated shim that
patches the provider's SDK client constructors. Reaches call sites static
transforms cannot, but only for providers that have a shim, and it requires
injecting an import into entry points.
_Avoid_: runtime mode, monkey-patch mode

**Shim**:
The generated file under `.promptguard/` that performs runtime interception for
a set of providers.

**Entry point**:
A source file the CLI injects a shim import into, chosen because the app starts
there (`main.py`, a `package.json` main, an `if __name__ == "__main__"` block).

### Tool calls

**MCP proxy**:
`promptguard mcp-proxy`, which wraps a stdio MCP server the developer already
runs and decides each tool call before the server sees it.
_Avoid_: MCP server (that is `promptguard mcp`, which serves PromptGuard's own
tools), firewall, gateway

**Destructive-action check**:
The local regex over a tool call's arguments: the earlier rule set's pattern, ported as is,
plus our own extensions for what it misses (split or long `rm` recursive/force
flags, `find -delete`, `find -exec rm`, `DROP SCHEMA`). Runs with no network,
so it is what still protects under the `mixed` fail mode during an outage.
_Avoid_: dangerous-pattern check (that is the backend's own list)

**Fail mode**:
What the MCP proxy does with a tool call the API gives no verdict on: `open`,
`closed` or `mixed` (default). ADR-0034 in the platform repo records why.

**Unchecked call**:
A tool call relayed without an API verdict because the fail mode allowed it.
Always logged to stderr as unchecked.

**Tool result redaction**:
The MCP proxy sending the text of a `tools/call` result to the guard endpoint
as output, and relaying the result with what it masked. Only strings change.
An unredacted result (relayed because the fail mode allowed it) is always
logged as unredacted.
_Avoid_: result scan, output scan

**Tool-list pin**:
The opaque hash the manifest-scan endpoint returns for a server's tool list,
stored in `~/.promptguard/mcp-pins.json` under a hash of the server's command
line and sent back on the next scan, so a changed list raises a rug-pull
finding. Kept until the developer removes it; a changed list never replaces it.
_Avoid_: tool hash (that is one tool's hash, kept beside the pin), checksum

**Withheld tool**:
A tool removed from a tool list because the scan found something `high` on
it. A call to it is refused.
_Avoid_: blocked tool (the API's validate-tool blocks calls, not tools)

### The project side

**Project**:
A PromptGuard project on the server: account-scoped, identified by a project
id, and the thing that owns guardrails and logs. One is marked active in the
global credentials.
_Avoid_: account, org, workspace, and never the local codebase

**Guardrails**:
A project's live, server-side configuration deciding what gets blocked or
redacted. The server holds the source of truth.
_Avoid_: rules, settings, policy (see below)

**Policy**:
A YAML file in the codebase expressing a project's guardrails so they can be
reviewed and versioned in git. A front end for guardrails, not a second
configuration system: applying a policy writes guardrails, exporting reads
them.
_Avoid_: policy config, guardrail file, rules file

**Threat scan**:
Sending content to the API to be classified as safe or hostile. A network call
about content, sharing only the word "scan" with detection, which is neither.
_Avoid_: scan (unqualified), content check, security scan

**Offline threat scan**:
A threat scan run on the machine (`scan --offline`) against the **local rule
pack**, with no network and no key. A local subset of what the API detects,
never a substitute for it, and it says so in its output. Still a threat scan,
not detection: it is about content, not call sites.
_Avoid_: local scan, offline mode (unqualified), lite scan

**Untrusted content**:
Content the operator did not write: tool output, retrieved documents, web
pages, memory. An offline threat scan runs its indirect-injection rules only
on content marked untrusted (`--untrusted`), because the same words are
normal in a system prompt.
_Avoid_: external content, third-party input

**Local rule pack**:
The regex rules an offline threat scan matches, vendored in
`src/local_rules/pack.json` and generated from upstream Go detectors. Never
hand-edited; regenerated, then re-pinned by its tests.
_Avoid_: ruleset, signatures, offline rules

**Decision**:
The API's verdict on scanned content — allow or block.
_Avoid_: result, verdict, outcome

**Threat type**:
The classification carried by a blocking decision. An open-ended string on the
wire, not an enumeration the CLI can exhaust.
_Avoid_: threat category, attack type

**Redaction**:
Replacing PII in content with placeholders. A separate API operation from a
threat scan, not a kind of decision.

**Red team run**:
Firing known adversarial prompts at a target to measure whether its guardrails
hold.
_Avoid_: test (that is a separate command and also means a Cargo test), attack
run, pentest
