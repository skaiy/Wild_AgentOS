# Security Policy

## Supported Versions

Wild AgentOS is under active pre-1.0 development. Security fixes are applied to
the `main` branch and the most recent release.

| Version | Supported |
| ------- | --------- |
| `main`  | ✅ |
| latest release | ✅ |
| older releases | ❌ |

## Reporting a Vulnerability

**Do not open a public issue for security vulnerabilities.**

Report privately through either channel:

1. **GitHub Security Advisory** (preferred) — use the
   [Report a vulnerability](https://github.com/skaiy/wild_agentos/security/advisories/new)
   button on the Security tab.
2. **Email** — diaoguoliang@gmail.com with `[SECURITY]` in the subject line.

Please include:

- Affected component and version or commit SHA
- Reproduction steps or a proof-of-concept
- Impact assessment and any suggested mitigation

Do not include live credentials, tokens, or customer data in your report.

## Response Process

| Stage | Target |
| ----- | ------ |
| Acknowledgement | within 3 business days |
| Initial assessment | within 7 business days |
| Fix or mitigation plan | within 30 days for high/critical severity |

We will keep you informed throughout triage and credit you in the advisory
unless you prefer to remain anonymous. Please allow us to publish a fix before
disclosing the issue publicly.

## Scope

In scope:

- The `wild-agent-os-core` library and workspace crates
- The `wildcode` CLI and its TUI
- Sandbox escape, tool-execution guardrail bypass, and skill-graph security gate bypass
- Credential or secret leakage through logs, traces, or the knowledge graph
- Deployment manifests under `deploy/`

Out of scope:

- Vulnerabilities in third-party model providers or MCP servers
- Issues requiring a pre-compromised host or physical access
- Findings that only affect unsupported, modified, or forked builds
- Denial of service caused by intentionally misconfigured resource limits

## Security Features Already Enabled

- GitHub secret scanning with push protection
- Tool-execution guardrails (`guard_rules.example.json`)
- Skill-graph security gates
- Workspace filesystem monitor with path allowlisting
