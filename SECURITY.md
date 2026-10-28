# Security Policy

AlphaOS is an experimental operating-system project and is not intended for
use in production or to protect sensitive data. Security fixes are considered
for the current development branch; older releases and snapshots are not
maintained.

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| Latest development branch (`main`) | :white_check_mark: |
| Older releases and snapshots      | :x:                |

If the project begins publishing versioned releases, this table will be
updated to identify the supported release lines.

## Reporting a Vulnerability

Please do not report security vulnerabilities in public issues or pull
requests. Use GitHub's private vulnerability reporting for this repository:

1. Open the repository's **Security** tab.
2. Select **Advisories**, then **Report a vulnerability**.
3. Include the affected commit or version, the relevant component, the impact,
   and concise steps to reproduce the issue. Redact personal information and
   any secrets from logs or examples.

If the private reporting option is unavailable, contact the repository
maintainer through their GitHub profile to request a private reporting
channel. Do not post exploit details publicly while arranging contact.

You can expect an acknowledgement within 7 calendar days and an initial
assessment or status update within 14 calendar days. The maintainer will
investigate the report and, if it is accepted, work on a fix and coordinate
public disclosure after a fix or mitigation is available. If it is declined,
the maintainer will explain why when possible. Response times may be longer
because this is an experimental project maintained on a best-effort basis.
