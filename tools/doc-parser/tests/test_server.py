import json
import socket
import threading
import time

from conftest import TOKEN, sha


def upload(client, data):
    status, _ = client.put(data)
    assert status in (200, 201)
    return sha(data)


def test_healthz_is_open_and_the_rest_needs_the_token(make_service):
    _, c, _ = make_service()
    assert c.call("GET", "/healthz", token=None) == (200, {"status": "ok"})
    assert c.call("GET", "/v1/info", token=None)[0] == 401
    assert c.call("GET", "/v1/info", token="nope")[0] == 401
    assert c.put(b"x", token=None)[0] == 401
    assert c.post("/v1/parse", {}, token=None)[0] == 401
    assert c.call("GET", "/v1/other")[0] == 404


def test_info_shape(make_service):
    _, c, _ = make_service(workers=3)
    status, info = c.call("GET", "/v1/info")
    assert status == 200
    assert info["protocol"] == "oo.doc-parser/v1" and info["parser_id"] == "docling"
    assert info["adapter_version"] == "1" and info["formats"] == ["pdf", "docx", "pptx"]
    assert info["workers"] == 3 and info["busy"] == 0
    assert info["limits"]["max_blob_bytes"] == 209_715_200 and info["limits"]["ocr"] is False
    assert {"parser_version", "models", "limits"} <= set(info)


def test_blob_upload_rules(make_service):
    _, c, _ = make_service(scratch_bytes=100, max_blob_bytes=50)
    data = b"OK" + b"a" * 20
    assert c.put(data)[0] == 201
    assert c.put(data)[0] == 200
    assert c.put(data, digest=sha(b"other"))[0] == 409
    assert c.put(b"x" * 51)[0] == 400  # over the blob limit
    assert c.call("DELETE", f"/v1/blobs/{sha(data)}")[0] == 204
    assert c.post("/v1/parse", {"blob": sha(data), "format": "pdf"})[0] == 404
    # The scratch cap evicts the oldest, and a blob larger than the cap fails.
    big = b"OK" + b"b" * 48
    for i in range(3):
        assert c.put(big + bytes([i]) * 0)[0] in (200, 201)
    assert c.put(b"z" * 50 + b"1")[0] == 400


def test_short_body_is_a_bad_request(make_service):
    _, c, _ = make_service()
    s = socket.create_connection(("127.0.0.1", c.port))
    s.sendall(
        f"PUT /v1/blobs/{sha(b'abcdef')} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {TOKEN}\r\n"
        f"Content-Length: 6\r\n\r\nabc".encode()
    )
    s.shutdown(socket.SHUT_WR)
    assert b" 400 " in s.recv(4096).split(b"\r\n")[0]
    s.close()


def test_scratch_full_is_507(make_service):
    _, c, _ = make_service(scratch_bytes=10)
    assert c.put(b"x" * 11)[0] == 507


def test_parse_and_probe_happy_path(make_service):
    _, c, _ = make_service()
    h = upload(c, b"OK-document")
    status, body = c.post("/v1/parse", {"blob": h, "format": "pdf", "page_range": [1, 3], "options": {"ocr": False, "table_structure": True}})
    assert status == 200
    assert body["protocol"] == "oo.doc-parser/v1" and body["pages"] == [1, 3] and body["doc"]["table"] is True
    assert body["parser"]["id"] == "docling"
    status, body = c.post("/v1/parse", {"blob": h, "format": "docx", "options": {"table_structure": False}})
    assert status == 200 and body["doc"]["table"] is False
    status, body = c.post("/v1/probe", {"blob": h, "format": "pdf"})
    assert status == 200 and body["page_count"] == 3 and body["needs_password"] is False


def test_request_validation(make_service):
    _, c, _ = make_service()
    h = upload(c, b"OK-document")
    assert c.post("/v1/parse", {"blob": "zz", "format": "pdf"})[0] == 400
    assert c.post("/v1/parse", {"blob": h, "format": "xlsx"})[0] == 422
    assert c.post("/v1/probe", {"blob": h, "format": "docx"})[0] == 422
    assert c.post("/v1/parse", {"blob": h, "format": "pdf", "deadline_ms": 120001})[0] == 400
    assert c.post("/v1/parse", {"blob": h, "format": "pdf", "deadline_ms": 0})[0] == 400
    assert c.post("/v1/parse", {"blob": h, "format": "pdf", "page_range": [3, 1]})[0] == 400
    assert c.post("/v1/parse", {"blob": h, "format": "docx", "page_range": [1, 2]})[0] == 400
    status, body = c.post("/v1/parse", {"blob": h, "format": "pdf", "options": {"ocr": True}})
    assert status == 422 and body["error"]["code"] == "PARSER_UNSUPPORTED"
    assert c.call("POST", "/v1/parse", body=b"not json")[0] == 400


def test_typed_backend_errors(make_service):
    _, c, _ = make_service()
    enc = upload(c, b"ENC-document")
    status, body = c.post("/v1/parse", {"blob": enc, "format": "pdf"})
    assert (status, body["error"]["code"]) == (422, "PARSER_ENCRYPTED")
    big = upload(c, b"BIG-document")
    assert c.post("/v1/parse", {"blob": big, "format": "pdf"})[1]["error"]["code"] == "PARSER_OUTPUT_LIMIT"
    bad = upload(c, b"BAD-document")  # an unexpected exception is a worker failure
    status, body = c.post("/v1/parse", {"blob": bad, "format": "pdf"})
    assert (status, body["error"]["code"]) == (500, "PARSER_CRASH")


def test_worker_crash_is_isolated_and_counted(make_service):
    svc, c, _ = make_service()
    crash = upload(c, b"CRASH-document")
    status, body = c.post("/v1/parse", {"blob": crash, "format": "pdf"})
    assert (status, body["error"]["code"]) == (500, "PARSER_CRASH")
    assert "signal 11" in body["error"]["message"]
    assert svc.pool.stats["crash"] == 1
    # The service and its other requests are unaffected.
    ok = upload(c, b"OK-document")
    assert c.post("/v1/parse", {"blob": ok, "format": "pdf"})[0] == 200
    assert c.call("GET", "/v1/info")[1]["stats"]["crash"] == 1
    assert svc.pool.busy == 0


def test_sigkill_with_oom_counter_is_507_otherwise_500(make_service):
    svc, c, _ = make_service()
    plain = upload(c, b"KILL9-plain")
    status, body = c.post("/v1/parse", {"blob": plain, "format": "pdf"})
    assert (status, body["error"]["code"]) == (500, "PARSER_CRASH")
    oom = upload(c, b"KILL9OOM-x")
    status, body = c.post("/v1/parse", {"blob": oom, "format": "pdf"})
    assert (status, body["error"]["code"]) == (507, "PARSER_OOM")
    assert svc.pool.stats["oom"] == 1


def test_deadline_kills_the_worker_with_504(make_service):
    svc, c, _ = make_service()
    slow = upload(c, b"SLEEP-document")
    t0 = time.monotonic()
    status, body = c.post("/v1/parse", {"blob": slow, "format": "pdf", "deadline_ms": 400})
    assert (status, body["error"]["code"]) == (504, "PARSER_TIMEOUT")
    assert time.monotonic() - t0 < 5
    assert svc.pool.stats["timeout"] == 1 and svc.pool.busy == 0


def test_busy_is_429_and_frees_up(make_service):
    svc, c, _ = make_service(workers=1)
    slow = upload(c, b"SLEEP-document")
    ok = upload(c, b"OK-document")
    first = {}
    t = threading.Thread(
        target=lambda: first.update(r=c.post("/v1/parse", {"blob": slow, "format": "pdf", "deadline_ms": 1500}))
    )
    t.start()
    for _ in range(100):
        if svc.pool.busy == 1:
            break
        time.sleep(0.02)
    status, body = c.post("/v1/parse", {"blob": ok, "format": "pdf"})
    assert (status, body["error"]["code"]) == (429, "PARSER_BUSY")
    assert body["error"]["retry_after_ms"] == 2000
    assert c.call("GET", "/v1/info")[1]["busy"] == 1
    t.join()
    assert first["r"][0] == 504
    assert c.post("/v1/parse", {"blob": ok, "format": "pdf"})[0] == 200
    assert svc.pool.stats["busy_rejected"] == 1


def test_client_disconnect_cancels_the_worker(make_service):
    svc, c, _ = make_service()
    slow = upload(c, b"SLEEP-document")
    body = json.dumps({"blob": slow, "format": "pdf", "deadline_ms": 60000}).encode()
    s = socket.create_connection(("127.0.0.1", c.port))
    s.sendall(
        f"POST /v1/parse HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {TOKEN}\r\n"
        f"Content-Length: {len(body)}\r\n\r\n".encode() + body
    )
    for _ in range(100):
        if svc.pool.busy == 1:
            break
        time.sleep(0.02)
    assert svc.pool.busy == 1
    # shutdown, not just close: the forked test worker inherits this fd.
    s.shutdown(socket.SHUT_RDWR)
    s.close()
    for _ in range(100):
        if svc.pool.busy == 0:
            break
        time.sleep(0.05)
    assert svc.pool.busy == 0 and svc.pool.stats["cancelled"] == 1
