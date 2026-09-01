# Refuse an environment or global API key against a repo-configured proxy host

The repo config is committed, so cloning someone's codebase means executing
their `proxy_url`. If the CLI resolved the API key from the environment or the
global credentials and then sent it to whatever host that file named, a
malicious repository would exfiltrate the developer's key on the first
`promptguard` command — no code execution required, just a JSON file. So
`resolve_session` refuses that specific combination and tells the user why.

The refusal is narrow on purpose: it fires only when the base URL came from the
repo config, the key did *not*, and the host is neither the default PromptGuard
host nor loopback. A key that came from the same repo config is no more exposed
by using it. A proxy on the developer's own machine cannot exfiltrate anything,
and loopback is matched through `url::Url::host` rather than a string compare,
because the `url` crate renders IPv6 loopback as the bracketed `"[::1]"` and a
naive comparison never matches it.

## Consequences

Self-hosting the proxy is deliberately not frictionless. A team pointing their
repo config at their own gateway hits the refusal, and has to either set
`PROMPTGUARD_BASE_URL` themselves — which makes trusting that host an act by the
person running the command, not by the file they cloned — or pass
`--allow-custom-proxy`. The error message names both.

This is why any command that sends the key to the resolved base URL must call
`resolve_session`, never `resolve_api_key` and `resolve_base_url` separately:
resolving them independently is exactly the bug this prevents, and it looks
correct at the call site.
