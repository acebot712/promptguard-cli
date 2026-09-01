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
