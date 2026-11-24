import hashlib
import struct

from doc_parser import embed

# Same list and digest as crates/oo-docs/src/vector.rs (PARITY_TEXTS, parity_digest).
PARITY_TEXTS = [
    "",
    "ab",
    "Quarterly   budget review\tfor the Apollo project",
    "项目预算评审会议纪要",
    "Mixed 中文 and ASCII, ünïcode",
    "a" * 44,
]
PINNED = "8afb69c88427c67a6bcbdf0a4ac78a3e7986358861e3cba730c4f7918ac42e37"


def test_matches_the_rust_embedding_byte_for_byte():
    h = hashlib.sha256()
    for t in PARITY_TEXTS:
        for v in embed.embed(t):
            h.update(struct.pack(">h", v))
    assert h.hexdigest() == PINNED


def test_folding_and_empty():
    assert embed.embed("Apollo   Budget") == embed.embed("apollo budget")
    assert embed.embed("") == [0] * embed.DIM
