"""What this image can do, without importing Docling."""
import importlib.util


def table_structure_available():
    """PDF table structure needs TableFormer, whose docling-ibm-models code
    imports cv2 at import time. The L3 image carries no OpenCV (every
    opencv-python wheel bundles FFmpeg, D65), so it is off there."""
    return importlib.util.find_spec("cv2") is not None
