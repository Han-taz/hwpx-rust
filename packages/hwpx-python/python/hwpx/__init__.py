"""
hwpx - Python bindings for HWP/HWPX document parser

Example usage:
    >>> import hwpx
    >>>
    >>> # Parse from file path
    >>> doc = hwpx.parse_file("document.hwpx")
    >>>
    >>> # Or parse from bytes
    >>> with open("document.hwpx", "rb") as f:
    ...     doc = hwpx.parse(f.read())
    >>>
    >>> # Convert to markdown
    >>> markdown = doc.to_markdown()
    >>> print(markdown)
    >>>
    >>> # Convert to HTML
    >>> html = doc.to_html()
    >>>
    >>> # Get plain text
    >>> text = doc.get_text()
    >>>
    >>> # Convert to JSON
    >>> json_str = doc.to_json()
"""

from .hwpx import parse, parse_file, Document

__all__ = ["parse", "parse_file", "Document"]
__version__ = "0.1.0"
