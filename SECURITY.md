# Security policy

NEXUS runs commands, reads your code and holds API keys, so security reports are taken seriously and handled before anything else.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately through GitHub: on the repository page, open **Security → Report a vulnerability** ([direct link](https://github.com/yazhiniyanMS/nexus/security/advisories/new)). Include what you found, how to reproduce it, the NEXUS version and your operating system. A small proof of concept helps; please do not include real credentials or other people's data.

What to expect:

- an acknowledgement within a few days;
- an honest assessment of severity, and a fix or a mitigation plan;
- credit in the release notes if you want it.

NEXUS is maintained by one person, so please allow reasonable time before disclosing publicly; 90 days is the default.

## Supported versions

| Version | Supported |
| --- | --- |
| Latest release and `main` | Yes |
| Older releases | No — please update |

## What is in scope

Anything that breaks the promises in [docs/PRIVACY.md](docs/PRIVACY.md) and the trust boundaries in [docs/audit/THREAT_MODEL.md](docs/audit/THREAT_MODEL.md), for example:

- a command running without approval, or at a less restricted level than the one shown;
- an agent writing outside its isolated workspace, or a change reaching your project without approval;
- reading files outside an imported project through the app;
- an API key leaving the system keychain, appearing in logs, errors or reports, or being sent to the wrong server;
- source code or credentials leaving the machine without the confirmation NEXUS promises;
- script injection into the app's window from project content or model output.

Known, documented limitations — such as Trusted commands running unsandboxed, or Windows having no sandbox backend — are listed in [docs/audit/SECURITY_AUDIT.md](docs/audit/SECURITY_AUDIT.md) and [docs/LIMITATIONS.md](docs/LIMITATIONS.md). Ideas for reducing them are welcome as normal issues.
