"""Document/Agent DB comparison; JSON numbers are exact, scalar types stay typed."""
from decimal import Decimal
import json

from comparison import compare


def document(text):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate document key")
            result[key] = value
        return result

    def invalid(_):
        raise ValueError("nonfinite JSON number")

    value = json.loads(text, parse_float=Decimal, parse_int=Decimal,
                       parse_constant=invalid, object_pairs_hook=pairs)
    if not isinstance(value, dict):
        raise ValueError("full document must be an object")
    return value


def compare_documents(left, right, *, document_column, chunk_size=64, consistency="S0"):
    """Rows must be ordered by the same text _id; final envelope cells are JSON text."""
    def rows(source):
        previous = None
        for row in source:
            if not row or not isinstance(row[0], str) or (
                    previous is not None and row[0] <= previous):
                raise ValueError("document IDs must be unique, text and ordered")
            previous = row[0]
            cells = list(row)
            cells[document_column] = document(cells[document_column])
            yield cells

    return compare(rows(left), rows(right), key_columns=[0], chunk_size=chunk_size,
                   consistency=consistency)
