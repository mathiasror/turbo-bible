# Security Policy

## Supported versions

The latest released version of turbo-bible receives security fixes. Older
versions are not patched; please upgrade to the current release.

## Threat surface

turbo-bible is an offline terminal Bible reader. It has a small attack
surface:

- **Local only at runtime.** Verse data is read from SQLite files in
  `~/.local/share/turbo-bible/translations/`; the app makes no network
  requests while reading.
- **Asset downloads.** Translation and cross-reference databases are fetched
  over HTTPS from GitHub Releases on first use. Every downloaded file is
  verified against a SHA-256 recorded in the manifest embedded in the
  binary before it is written to disk. A compromised download that doesn't
  match the expected hash is rejected.
- **Update check.** The notify-only update check fetches the
  `releases/latest` redirect from GitHub over HTTPS via `curl`. It never
  downloads or installs anything; a failure is silently ignored.

## Reporting a vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report privately via one of:

- **GitHub Security Advisories:** <https://github.com/mathiasror/turbo-bible/security/advisories/new>
- **Email:** <mathiasror@gmail.com>

Include a description of the issue, steps to reproduce, and — if known — a
suggested fix or mitigation. I will acknowledge your report within a few
business days and work with you on a timeline for a fix and coordinated
disclosure.
