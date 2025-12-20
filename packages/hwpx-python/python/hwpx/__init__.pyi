"""Type stubs for hwpx module"""
from typing import Optional

class Document:
    """HWP/HWPX Document wrapper"""

    @property
    def version(self) -> str:
        """Get document version as string (e.g., '5.1.0.1')"""
        ...

    @property
    def section_count(self) -> int:
        """Get number of sections in the document"""
        ...

    def to_markdown(
        self,
        use_html: bool = True,
        include_version: bool = True,
        image_output_dir: Optional[str] = None,
    ) -> str:
        """
        Convert document to markdown format.

        Args:
            use_html: Whether to use HTML tags (default: True)
            include_version: Whether to include version info (default: True)
            image_output_dir: Directory to save images. If None, embeds as base64.

        Returns:
            Markdown string representation of the document.
        """
        ...

    def to_html(self, image_output_dir: Optional[str] = None) -> str:
        """
        Convert document to HTML format.

        Args:
            image_output_dir: Directory to save images. If None, embeds as base64.

        Returns:
            HTML string representation of the document.
        """
        ...

    def to_json(self) -> str:
        """
        Convert document to JSON format.

        Returns:
            JSON string representation of the document structure.
        """
        ...

    def get_text(self) -> str:
        """
        Get plain text content from the document.

        Returns:
            Plain text content with paragraphs separated by newlines.
        """
        ...

def parse(data: bytes) -> Document:
    """
    Parse HWP/HWPX file from bytes.

    Args:
        data: File content as bytes.

    Returns:
        Parsed Document object.

    Raises:
        ValueError: If the file format is invalid or parsing fails.
    """
    ...

def parse_file(path: str) -> Document:
    """
    Parse HWP/HWPX file from file path.

    Args:
        path: Path to the HWP/HWPX file.

    Returns:
        Parsed Document object.

    Raises:
        ValueError: If the file cannot be read or parsing fails.
    """
    ...
