# Security Policy

## Supported Versions

Security fixes are accepted for the current `main` branch.

## Reporting a Vulnerability

Please report vulnerabilities by opening a private security advisory on GitHub when
available, or by contacting the repository owner. Do not include sensitive documents in
public issues.

Please include the smallest reproducer you can share, the affected API, the platform,
and whether the issue occurs in Rust, Python, or both. If the reproducer is a document,
remove private content before sharing it.

## Document Safety

HWP/HWPX files are untrusted input. Parser bugs that can cause crashes, excessive
resource use, or unsafe file writes are treated as security relevant.

Security-relevant reports include:

- parser panics, hangs, or uncontrolled memory growth on malformed documents
- path traversal or unsafe file writes during image extraction
- ZIP or XML resource limit bypasses
- dependency vulnerabilities affecting runtime code
- silent omission of security-relevant document content without diagnostics

The implementation security model, resource limits, and CI security controls are
documented in [docs/security-model.md](docs/security-model.md).
