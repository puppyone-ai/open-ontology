"""hashgram/1: the offline text embedding behind POST /v1/embed (10 11.3, S13).

Integer arithmetic only, so the result is byte-identical to crates/oo-docs/src/vector.rs:
lowercase ASCII, whitespace runs collapse to one space, every window of three characters (the whole
text when shorter) is hashed with FNV-1a 64 into 256 signed buckets, counts clamp to int16.
"""

MODEL_VERSION = "hashgram/1"
DIM = 256
_SPACE = frozenset(" \t\n\r\x0b\x0c　")


def _fnv1a(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def embed(text):
    chars = []
    gap = True
    for c in text:
        if c in _SPACE:
            if not gap:
                chars.append(" ")
            gap = True
        else:
            chars.append(chr(ord(c) + 32) if "A" <= c <= "Z" else c)
            gap = False
    while chars and chars[-1] == " ":
        chars.pop()
    acc = [0] * DIM
    if len(chars) == 0:
        grams = []
    elif len(chars) <= 2:
        grams = ["".join(chars)]
    else:
        grams = ["".join(chars[i:i + 3]) for i in range(len(chars) - 2)]
    for g in grams:
        h = _fnv1a(g.encode("utf-8"))
        acc[h % DIM] += 1 if h >> 63 == 0 else -1
    return [max(-32768, min(32767, a)) for a in acc]
