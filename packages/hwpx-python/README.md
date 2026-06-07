# hwpx

Python bindings for HWP/HWPX document parser.

The package is backed by Rust and ships CPython 3.8+ `abi3` wheels. CPU-bound parse
and conversion APIs release the Python GIL while Rust code is running.

## Installation

```bash
pip install hwpxkit
```

Or install from source:

```bash
# Install maturin first
pip install maturin

# Build and install
cd packages/hwpx-python
maturin develop
```

## Build Wheel

```bash
cd packages/hwpx-python

# Using uv (recommended). Builds a CPython 3.8+ abi3 wheel.
uv tool run maturin build --release --locked --compatibility pypi --out dist

# Or using maturin directly
maturin build --release --locked --compatibility pypi --out dist
```

The built wheel will be in the `dist/` directory.

## Usage

### Parse a document

```python
import hwpx

# Parse from file path
doc = hwpx.parse_file("document.hwpx")

# Or parse from bytes
with open("document.hwpx", "rb") as f:
    doc = hwpx.parse(f.read())
```

### Convert to Markdown

```python
# Basic conversion
markdown = doc.to_markdown()
print(markdown)

# With options
markdown = doc.to_markdown(
    use_html=True,           # Use HTML tags for tables, etc.
    include_version=True,    # Include document version
    image_output_dir="./images"  # Save images to directory
)
```

### Convert to HTML

```python
html = doc.to_html()

# Save images to directory instead of base64
html = doc.to_html(image_output_dir="./images")
```

### Get plain text

```python
text = doc.get_text()
print(text)
```

### Convert to JSON

```python
json_str = doc.to_json()
print(json_str)
```

### Parser diagnostics

```python
report = doc.diagnostic_report()
print(report["summary"])
for item in report["items"]:
    print(item["severity"], item["category"], item["message"])
```

`doc.warnings` remains available for string compatibility.

### Working with untrusted documents

HWP/HWPX input is treated as untrusted. The Rust parser enforces HWPX ZIP, XML, and
section resource limits and returns structured diagnostics for unsupported or lossy
content. `hwpx.parse_file()` rejects source files larger than 512 MiB before reading
them into memory; callers that use `hwpx.parse(bytes)` are responsible for bounding the
bytes they pass in. See the repository security model for details.

### Document properties

```python
# Get document version
print(doc.version)  # e.g., "5.1.0.1"

# Get number of sections
print(doc.section_count)
```

## Supported Formats

- **HWP 5.0**: Binary format (Hangul Word Processor)
- **HWPX**: XML-based format (OWPML standard)

## License

MIT
