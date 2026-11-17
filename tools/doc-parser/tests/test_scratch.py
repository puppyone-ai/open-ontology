import io

import pytest
from conftest import sha

from doc_parser.scratch import Scratch, ScratchFull


def put(s, data):
    r = io.BytesIO(data)
    return s.put(sha(data), len(data), r.read)[0]


def test_put_exists_and_mismatch(tmp_path):
    s = Scratch(tmp_path, 1000)
    assert put(s, b"a" * 10) == "created"
    assert put(s, b"a" * 10) == "exists"
    bad = io.BytesIO(b"b" * 10)
    assert s.put(sha(b"zzz"), 10, bad.read)[0] == "mismatch"
    assert s.count() == 1
    assert not list(tmp_path.glob("*.part"))


def test_short_body_is_reported(tmp_path):
    s = Scratch(tmp_path, 1000)
    r = io.BytesIO(b"abc")
    assert s.put(sha(b"abcdef"), 6, r.read)[0] == "short"
    assert s.count() == 0 and s.total() == 0


def test_lru_eviction_spares_pinned_and_recent(tmp_path):
    s = Scratch(tmp_path, 100)
    a, b, c = b"a" * 40, b"b" * 40, b"c" * 40
    put(s, a)
    put(s, b)
    with s.use(sha(a)):  # a is pinned and now most recent
        assert put(s, c) == "created"  # evicts b, the least recent unpinned
        assert s.has(sha(a)) and not s.has(sha(b)) and s.has(sha(c))
    # Everything pinned: the cap cannot be met.
    with s.use(sha(a)), s.use(sha(c)):
        with pytest.raises(ScratchFull):
            put(s, b"d" * 40)
    assert s.total() <= 100


def test_restart_rebuilds_the_index_and_drops_partials(tmp_path):
    s = Scratch(tmp_path, 1000)
    put(s, b"x" * 5)
    (tmp_path / "junk.part").write_bytes(b"1")
    again = Scratch(tmp_path, 1000)
    assert again.has(sha(b"x" * 5)) and not (tmp_path / "junk.part").exists()


def test_delete_skips_pinned(tmp_path):
    s = Scratch(tmp_path, 1000)
    put(s, b"x" * 5)
    with s.use(sha(b"x" * 5)):
        assert s.delete(sha(b"x" * 5)) is False
    assert s.delete(sha(b"x" * 5)) is True and not s.has(sha(b"x" * 5))
