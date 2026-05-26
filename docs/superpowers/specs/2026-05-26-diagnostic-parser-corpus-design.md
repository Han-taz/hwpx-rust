# Diagnostic Parser and Corpus Compatibility Design

## Context

`hwpx-rust` is a Rust workspace for parsing HWP/HWPX documents and converting them to
Markdown, HTML, JSON, and plain text. The project also ships Python bindings through
PyO3. The parser already supports many document features and has snapshot coverage for
existing fixtures, but its next open-source milestone should improve trust: users need
to know what was parsed, what was skipped, and where document meaning may have been
lost.

Current project constraints:

- HWP 5.0 binary support is maintained but frozen for new feature work.
- HWPX is the primary target for future parser improvements.
- `HwpDocument.warnings: ParseWarnings` already exists and is exposed through Rust,
  Python, and JSON.
- Existing snapshot tests and fixture tests should remain useful without large
  renderer rewrites.
- The repository metadata and public project files need to match the current remote
  repository before the project looks ready for external users.

## Goals

- Add structured parser diagnostics that distinguish unsupported features, recovered
  parse errors, invalid values, missing parts, skipped binary assets, and likely data
  loss.
- Keep existing `ParseWarnings` compatibility while adding a richer report for Rust,
  Python, and JSON consumers.
- Build a corpus compatibility harness over existing fixtures so parser reliability can
  be tracked document by document.
- Make HWPX parser reliability measurable through severity/category counts instead of
  relying on snapshot output alone.
- Add the minimum open-source baseline required for users to install, evaluate, and
  contribute to the project with accurate expectations.

## Non-Goals

- Do not add new HWP 5.0 binary parser features.
- Do not start a PDF converter.
- Do not perform a broad HTML/Markdown renderer rewrite.
- Do not treat visual conversion fidelity as the primary success metric for this
  milestone.
- Do not break existing `doc.warnings` or `HwpDocument.warnings` consumers.

## Recommended Approach

Use a diagnostic corpus approach:

1. Add a structured diagnostics module and attach a report to `HwpDocument`.
2. Instrument the HWPX parser at known fallback, skip, and unsupported-feature points.
3. Add a manifest-driven compatibility test over existing fixtures.
4. Update public repository metadata and baseline open-source files.

This keeps the first milestone focused on parser trust and project credibility. Renderer
quality improvements can use the same diagnostics and corpus harness in a later phase.

## Architecture

### New Module

Add `crates/hwp-core/src/diagnostics/`.

Core types:

```rust
pub struct DiagnosticReport {
    pub items: Vec<DiagnosticItem>,
}

pub struct DiagnosticItem {
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub message: String,
    pub context: DiagnosticContext,
    pub suggestion: Option<String>,
}

pub enum DiagnosticSeverity {
    Info,
    Warning,
    RecoveredError,
    Unsupported,
    DataLoss,
}

pub enum DiagnosticCategory {
    UnsupportedElement,
    UnsupportedAttribute,
    InvalidValue,
    MissingOptionalPart,
    RecoveredXml,
    SkippedBinary,
    RendererLossHint,
}
```

`DiagnosticContext` should capture only stable, useful fields:

- source path or stream name, for example `Contents/section0.xml`
- section index
- XML element name
- XML attribute name
- raw attribute value when useful and safe to expose
- approximate byte offset or XML reader buffer position when available
- parser component name, for example `hwpx::section`

### Document Integration

Add a structured report to `HwpDocument`:

```rust
pub diagnostics: DiagnosticReport
```

`DiagnosticReport` should implement `Default`, and the `diagnostics` field should use
`#[serde(default)]` so older serialized JSON without diagnostics can still deserialize
when consumers rely on `HwpDocument`.

Keep `warnings: ParseWarnings` for compatibility. During the first implementation, parser
sites that create diagnostics may also mirror important entries into `ParseWarnings`.
This avoids breaking the current Rust, Python, and JSON behavior while allowing new
consumers to move to structured diagnostics.

### Parser Integration

HWPX parser entry points should receive a mutable diagnostics report:

- `parser/hwpx/header.rs`
- `parser/hwpx/section.rs`
- `parser/hwpx/bindata.rs`
- `parser/hwpx/mod.rs`

The existing flow remains:

```text
HwpParser::parse(data)
  -> detect format
  -> open HWPX container
  -> parse header.xml
  -> parse section*.xml
  -> parse BinData/*
  -> resolve display texts
  -> return HwpDocument
```

The change is that non-fatal problems are accumulated in `document.diagnostics` instead
of being silently ignored or represented only as unstructured warning strings.

## Diagnostic Policy

Fatal errors should still return `Err(HwpError)`:

- ZIP container cannot be opened.
- Required HWPX files are missing. These are fatal only; they are not represented as
  document diagnostics because no complete `HwpDocument` is returned.
- XML is too malformed to continue.
- The document is encrypted or the format is unsupported.

Non-fatal diagnostics should be recorded and parsing should continue:

- unknown or unsupported HWPX element
- unsupported attribute on a known element
- invalid numeric or enum value recovered with a default
- referenced image or binary asset not found
- optional package part missing, recorded as `MissingOptionalPart`
- renderer likely cannot preserve a parsed feature

Severity meaning:

- `Info`: useful for corpus reporting but not a user-facing problem.
- `Warning`: unexpected condition with unclear data-loss impact.
- `RecoveredError`: malformed or invalid input was recovered with a fallback.
- `Unsupported`: parsed file contains a feature the library does not support.
- `DataLoss`: document meaning or assets were probably omitted from the model or output.

Do not use total warning count as the only quality metric. Corpus expectations should
track severity and category counts because unknown namespaces, repeated attributes, or
non-critical optional parts can otherwise create noisy failures.

## HWPX Instrumentation Rules

Start with focused knownlist instrumentation instead of flagging every unknown XML event.
The first pass should cover places where the parser already drops or defaults data.

Recommended first targets:

- invalid numeric attribute fallback in `header.rs` and `section.rs`
- unknown paragraph/control elements in `section.rs`
- unsupported field commands in field rendering/parser flow
- missing `BinData` files referenced from parsed metadata
- failed binary reads in `bindata.rs`
- optional HWPX package parts that are absent but useful to report

Avoid noisy diagnostics:

- Do not emit a warning for every namespace prefix variation.
- Do not emit repeated identical unsupported-element diagnostics without aggregation or
  count tracking.
- Do not classify renderer limitations as parser data loss unless parsed document data is
  actually omitted from the document model.

## Corpus Compatibility Harness

Add a manifest-driven corpus test under `crates/hwp-core/tests/corpus/`.

Example manifest shape:

```toml
[[documents]]
path = "../fixtures/test-hwpx.hwpx"
format = "hwpx"
expect_parse = "success"
max_data_loss = 0
allowed_unsupported = ["UnsupportedElement"]

[[documents]]
path = "../fixtures/password-12345.hwp"
format = "hwp"
expect_parse = "error"
expected_error = "Encrypted"
```

The manifest should support:

- fixture path
- expected format
- expected parse result: `success` or `error`
- expected error kind for failure cases
- maximum severity counts, especially `DataLoss` and `RecoveredError`
- allowed diagnostic categories
- optional expected document facts, such as section count or image count

The test should print clear failures:

```text
fixture test-hwpx.hwpx exceeded max_data_loss: expected <= 0, got 2
  - [DataLoss/SkippedBinary] missing BinData/BIN0001.jpg in Contents/section0.xml
```

General CI should run the fast corpus compatibility test as part of
`cargo test --workspace`. A separate ignored test or command may generate a full human
readable report when needed.

## Open-Source Baseline

Update public metadata and project files so the repository is accurate and approachable.

Required changes:

- Set Rust crate repository metadata to `https://github.com/Han-taz/hwpx-rust`.
- Set Python project URLs to `https://github.com/Han-taz/hwpx-rust`.
- Add or update `LICENSE` with MIT license text matching the existing Cargo/Python
  metadata. The copyright line should use the workspace author identity unless the
  repository owner chooses a different holder before implementation.
- Add `CHANGELOG.md`.
- Add `CONTRIBUTING.md`.
- Add `SECURITY.md`.
- Update README support status:
  - HWP 5.0: supported at current level, new feature development frozen.
  - HWPX: active development target.
  - Diagnostics: unsupported or lossy parsing should be reported rather than hidden.
- Document local verification commands:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test --workspace`

## Public API Exposure

Rust:

- Add `HwpDocument::diagnostics` as a serializable field.
- Re-export diagnostics types from `hwp_core`.
- Keep `HwpDocument::warnings` unchanged.

Python:

- Keep `doc.warnings` unchanged.
- Add `doc.diagnostic_report()` returning JSON-compatible structured data with an
  `items` list and summary counts by severity/category.
- Keep error types as `ValueError` initially unless a later API design introduces custom
  Python exceptions.

JSON:

- Include diagnostics in `to_json()` through `HwpDocument` serialization.
- Keep existing warnings field for compatibility.

## Testing Strategy

Unit tests:

- diagnostic item construction
- severity/category counting
- context attachment
- serialization shape
- conversion or mirroring between diagnostics and `ParseWarnings` if implemented

Parser regression tests:

- invalid numeric attributes produce `InvalidValue` and `RecoveredError`
- unknown supported-scope element produces `UnsupportedElement`
- missing binary reference produces `SkippedBinary` or `DataLoss`
- unsupported control or field command produces `Unsupported`

Corpus tests:

- parse each manifest fixture
- validate expected success/failure
- validate severity and category counts
- validate optional document facts where listed

Existing tests:

- keep snapshot tests passing unless diagnostics intentionally change JSON output
- when JSON snapshots change because diagnostics are added, review and accept only the
  expected structural addition

## Milestones

### 1. Diagnostics Foundation

- Add diagnostics module.
- Add diagnostics field to `HwpDocument`.
- Re-export diagnostics API.
- Add unit tests for report behavior.
- Preserve `ParseWarnings` compatibility.

### 2. HWPX Diagnostic Instrumentation

- Thread diagnostics through HWPX parser modules.
- Add diagnostics at key fallback and unsupported-feature points.
- Keep fatal error behavior unchanged.
- Add XML-fragment parser tests for diagnostic cases.

### 3. Corpus Compatibility Harness

- Add manifest format.
- Add fixture-driven compatibility test.
- Add concise failure output.
- Add optional ignored full report path if useful.

### 4. Open-Source Baseline

- Fix repository metadata.
- Update README support/status language.
- Add license, changelog, contributing, and security files.
- Ensure local verification commands match CI.

### 5. Public API Exposure

- Add Rust access to structured diagnostics.
- Add Python structured diagnostics access.
- Preserve old warnings behavior.
- Document examples.

## Acceptance Criteria

- `cargo test --workspace` passes.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- `cargo fmt --all -- --check` passes.
- Existing `doc.warnings` behavior remains available.
- `HwpDocument` JSON includes structured diagnostics.
- At least three HWPX diagnostic cases are covered by focused parser tests.
- Existing fixtures are represented in a corpus manifest or an initial curated subset.
- Corpus test failures identify the fixture, expected threshold, actual count, and the
  most relevant diagnostics.
- README and package metadata accurately point to the current repository and describe
  HWP/HWPX support status.

## Risks

- JSON snapshots may change when diagnostics are added to `HwpDocument`.
- Reporting every unknown XML element could produce too much noise.
- Adding diagnostics and warnings side by side can create duplicated user-visible
  messages if mirroring is not carefully scoped.
- Corpus thresholds can become brittle if they are too strict before diagnostics settle.

Mitigations:

- Start with targeted instrumentation rather than all unknown events.
- Use category/severity counts instead of exact full-report matching for corpus tests.
- Keep compatibility shims small and document the intended migration path.
- Review JSON snapshot changes explicitly.
