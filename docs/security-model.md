# Security Model

This project treats every HWP and HWPX document as untrusted input. The parser and
converters are designed to extract document structure and render text, images, HTML,
Markdown, and JSON without executing document-provided code.

## Trust Boundaries

- Input bytes passed to Rust or Python parsing APIs are untrusted.
- ZIP entry names, XML text, XML attributes, embedded binary data, and declared sizes
  are untrusted.
- Output directories supplied for extracted images are trusted caller-controlled
  destinations; document-provided names are not used as filesystem paths.
- Generated HTML is an output format, not a browser sandbox. Consumers that embed it in
  web applications should still apply their normal content-security policy.

## HWPX Container Limits

HWPX files are ZIP containers. The container reader enforces fixed limits before
parsing document XML:

| Resource | Limit |
| --- | ---: |
| Archive byte size | 512 MiB |
| Archive entries | 8,192 |
| Single entry uncompressed size | 128 MiB |
| Single entry compression ratio | 1,000:1 |
| Total archive uncompressed size | 512 MiB |

ZIP entry paths are rejected when they are empty, absolute, contain backslashes or
drive separators, contain empty path components, or contain `.`/`..` components.
Duplicate entry paths are rejected after canonicalizing trailing directory slashes,
so `Contents` and `Contents/` cannot coexist as ambiguous archive entries. This
prevents ambiguous archive layouts and path traversal issues before any entry is read.
Only Stored and Deflated ZIP entries are accepted; unsupported or unexpected
compression methods are rejected during archive metadata validation.
Encrypted ZIP entries are rejected during archive metadata validation because
HWPX parsing APIs do not accept document passwords and should not defer
password-required failures until individual parts are read.

## XML and Section Limits

The HWPX XML parser applies resource budgets to avoid unbounded CPU or memory use:

| Resource | Limit |
| --- | ---: |
| XML events per parsed XML file | 1,000,000 |
| XML nesting depth | 256 |
| XML attribute value bytes per attribute | 1 MiB |
| Character shapes in `Contents/header.xml` | 65,536 |
| Paragraph shapes in `Contents/header.xml` | 65,536 |
| Paragraphs per section | 200,000 |
| Text bytes per section | 64 MiB |
| Text runs per paragraph | 65,536 |
| Table rows per section | 65,535 |
| Table cells per section | 500,000 |
| Paragraphs per table cell | 32,767 |

When a limit is exceeded, parsing fails with `HwpError::ResourceLimitExceeded` and
includes the resource name, source path, limit, and observed value.

HWPX XML declarations are allowed, but DTD/DOCTYPE declarations and processing
instructions are rejected. The parser does not need document-provided DTDs,
stylesheets, or tool instructions, and rejecting them removes an unnecessary XML attack
surface before document content is interpreted.

Malformed XML attributes and duplicate attributes on the same element are rejected
rather than silently ignored. Numeric attribute values that are syntactically valid XML
but semantically invalid for HWPX are still treated as recoverable parser diagnostics
where the parser has a safe default. Malformed numeric attribute entity references are
treated as XML parse errors instead of being recovered through those defaults.
Numeric ID references are parsed to the width used by the destination document
structure, so out-of-range values record diagnostics and fall back to safe defaults
instead of wrapping during narrower integer conversion.
Header character and paragraph shapes are preserved for both explicit end-tag
and self-closing `charPr`/`paraPr` forms, avoiding data loss from common XML
shorthand.
Self-closing header scope containers such as `charProperties` and `paraProperties`
do not leave parser scope open for following sibling elements.
Paragraph text character counts are checked before conversion to the `ParaHeader`
document model width, so oversized internal text builders fail instead of wrapping.
Header character shape heights are semantically validated as positive values;
invalid non-positive sizes record diagnostics and fall back to the default font
height instead of propagating hostile font sizes into renderers.
Table row and column span attributes are semantically validated as at least 1;
zero spans record diagnostics and fall back to a single-cell span instead of
propagating invalid table geometry into renderers.
Table row counts are checked before conversion to the `TableAttributes` document
model width, so oversized internal table builders fail instead of wrapping to zero.
Implicit table column address advancement uses checked arithmetic; overflow records
a diagnostic and resets the implicit cursor instead of panicking or wrapping. Computed
table column counts are checked before conversion to the document model width and
saturate at `u16::MAX` with a diagnostic instead of wrapping to a smaller value.
Table cell paragraph counts are limited to the `ListHeader` model width and reject
oversized cells instead of wrapping the signed 16-bit paragraph count negative.
String attributes that carry parser-interpreted values, such as image
`binaryItemIDRef` references, hyperlink control values, header color values, and
header style enum values, are decoded through XML attribute unescaping before
interpretation. Malformed string attribute entities are treated as XML parse
errors instead of being matched or normalized as raw entity text.
Malformed XML text entity references are rejected instead of being converted to
empty text, so invalid text content cannot silently disappear during parsing.

## File Selection and Binary Data

- ZIP entry paths are byte-bounded before they are cached or interpreted.
- Section files are accepted only when they match `Contents/section<N>.xml`.
- Section files are sorted by numeric section index instead of lexicographic order.
- When `Contents/content.hpf` defines section order, duplicate direct spine `idref`
  entries are rejected before any section body is parsed.
- When distinct `content.hpf` manifest ids resolve to the same section file and are
  both listed in the spine, the duplicate section href is rejected before any section
  body is parsed.
- `content.hpf` manifest ids, manifest hrefs, spine idrefs, and section media types
  are length-bounded before they are stored or interpreted.
- Binary item references are normalized before matching document references to
  `BinData` entries.
- HWPX `BinData` item indexes are checked before conversion to the `WORD` document
  model width, so oversized internal item lists fail instead of wrapping to zero.
- Nested or non-canonical binary data archive entries are ignored for indexing.
- Image extraction uses sanitized extensions from `BinData` metadata or falls back to
  decoded image magic bytes. Unsafe metadata extensions cannot affect output paths.
- Embedded HTML and Markdown image data URIs are emitted only after normalizing
  standard base64 payloads. Malformed payloads become inert `#` destinations instead
  of raw link or CSS URL data.
- Embedded Markdown image data URIs additionally require magic bytes to identify a
  supported image type before a `data:` destination is emitted.

## Python Binding Safety

The Python package exposes a native PyO3 extension with CPython 3.8+ `abi3` wheels.
CPU-bound parse and conversion APIs release the Python GIL while running Rust code:

- `parse`
- `parse_file`
- `Document.to_markdown`
- `Document.to_html`
- `Document.to_json`
- `Document.get_text`

`hwpx.parse_file()` checks the source file size before reading and rejects files larger
than 512 MiB. It also performs a capped read to handle files that grow after metadata
inspection. The `hwpx.parse(bytes)` API assumes the caller already owns and has bounded
the provided byte buffer.

The wheel validation script rejects missing type marker files, generated Python cache
artifacts, local macOS extension artifacts, non-`abi3` native extension tags on Unix,
and version drift between package metadata and `hwpx.__version__`.

## Diagnostics

Unsupported or lossy parsing should be surfaced through structured parser
diagnostics. Silent data loss is considered a correctness and security-review issue
because users need to know when a conversion omitted or recovered content.
Unsupported section element diagnostics are aggregated by canonical local element
name, not raw XML namespace prefix, so attacker-controlled prefix churn cannot
create unbounded distinct unsupported-element diagnostic keys.
Diagnostic reports are capped at 10,000 items. When the cap is reached, the final
stored item is a `DiagnosticLimit` warning that indicates additional parser
diagnostics were suppressed. This keeps adversarial documents from turning
recoverable parse errors into an unbounded diagnostic allocation.
Legacy parser warnings are also capped at 10,000 items. When the cap is reached,
the final stored warning says that additional parser warnings were suppressed,
preserving the existing warnings list API while preventing unbounded warning
allocation.

## Dependency and CI Controls

CI gates include:

- Rust tests, formatting, and clippy with `-D warnings`.
- LCOV coverage report generation with `cargo-llvm-cov`.
- Criterion benchmark target compilation in test mode.
- Fuzz target builds for parser entrypoints.
- `cargo audit` for known RustSec advisories.
- `cargo audit --file fuzz/Cargo.lock` for fuzz-only dependencies.
- `cargo deny --locked check` for dependency policy, license policy, duplicate-version
  policy, source policy, and blocked native TLS/git dependencies.
- CodeQL analysis for Rust and Python.
- GitHub Dependency Review on pull requests.
- Weekly Dependabot updates for GitHub Actions, Cargo, and Python packaging inputs.
- Python wheel build, wheel content validation, and import smoke tests.
- PyO3 GIL-release checker and checker tests for CPU-bound Python APIs.
- Release metadata validation across Rust workspace metadata, `Cargo.lock`, Python
  package metadata, and `hwpx.__version__`.
- PyPI Trusted Publishing with GitHub OIDC instead of long-lived API tokens.

## Non-Goals

- The library does not sanitize arbitrary user-authored HTML after conversion for all
  possible embedding environments.
- The library does not promise to preserve every unsupported HWP/HWPX feature; missing
  or lossy support should be represented as diagnostics.
- The library does not execute document scripts, macros, or embedded active content.

## Vulnerability Handling

Report suspected vulnerabilities privately. Include the smallest possible reproducer
and avoid sharing private documents. Parser crashes, hangs, uncontrolled resource use,
path traversal, unsafe file writes, dependency vulnerabilities, and silent security
relevant data loss are all security-relevant reports.
