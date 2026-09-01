# Two interception modes, not one

Routing a codebase's LLM traffic through PromptGuard can happen at two moments,
and neither one covers every case, so the CLI implements both and a codebase is
in exactly one of them. **Static transform mode** (the default) edits each call
site's base URL in source before the app runs. **Runtime shim mode** generates a
shim into `.promptguard/`, injects an import into the entry points, and patches
the provider SDK's client constructors at process start.

## Why both

Static transforms are reviewable — the change lands in the developer's diff and
`git` is the undo — but they can only edit call sites whose arguments are
static. A call site passing `**cfg` or a spread object is left untouched
(`TransformResult::needs_manual_routing`), because injecting a base URL there
can collide with what those arguments already carry and raise a `TypeError` at
runtime. Those call sites stay unprotected.

The runtime shim catches them, because it patches the constructor rather than
the caller, and it survives base URLs assembled at runtime. It costs more: it
only exists for providers that have a shim written (`Provider::has_runtime_shim`
— OpenAI, Anthropic, Cohere, HuggingFace), it modifies entry points, and it
intercepts only the SDK client classes it knows about, so an unusual client
class silently escapes it.

## Consequences

Every lifecycle command branches on `runtime_mode`, and each mode leaves
different artifacts behind, so `disable` and `revert` have to undo both shapes —
restoring recorded backups for one, removing injected imports and `.promptguard/`
for the other. Shim injection also has to keep removing the *old* shim block
format (`PYTHON_SHIM_LEGACY_LINES`), because a codebase enabled by an earlier
CLI has no end-marker to delete up to.

Adding a provider is therefore two decisions, not one: recognising its SDK is
enough for static transform mode, while runtime shim mode additionally needs a
template. Shipping the first without the second is expected, not an oversight.
