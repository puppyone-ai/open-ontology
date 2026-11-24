"""What this image can do, without importing Docling."""
import importlib.util


def table_structure_available():
    """PDF table structure needs TableFormer, whose docling-ibm-models code
    imports cv2 at import time. The image carries an OpenCV built from source
    without FFmpeg (D65, Dockerfile stage `opencv`), so it is on there; a dev
    host without cv2 reports it off and the adapter warns
    DOCS_PARSE_TABLE_STRUCTURE_OFF."""
    return importlib.util.find_spec("cv2") is not None
