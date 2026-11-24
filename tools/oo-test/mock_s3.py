#!/usr/bin/env python3
"""A tiny S3-compatible server for tests that cannot start a RustFS container.

Path-style only, one bucket, in memory. Every request is verified with an
independent Python implementation of AWS Signature V4 (stdlib hmac/hashlib), so
a green run proves the signatures `oo docs upload` and the engine send are
accepted by a second implementation. Supports PUT, HEAD, GET (with Range),
DELETE and ListObjectsV2 (prefix, delimiter, max-keys, continuation-token).

`read_only=True` answers every write with 403 AccessDenied (the credential can
read but not write), to test the clear upload error.

This is a test double, not a store: no multipart, no versioning, no ACLs.
"""

import hashlib
import hmac
import re
import threading
import urllib.parse
from datetime import datetime, timezone
from email.utils import formatdate
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from xml.sax.saxutils import escape


def _h(key, msg):
    return hmac.new(key, msg.encode(), hashlib.sha256).digest()


def sign(secret, method, path, query, headers, signed, payload, amz, region):
    canon_q = "&".join(
        f"{urllib.parse.quote(k, safe='-_.~')}={urllib.parse.quote(v, safe='-_.~')}"
        for k, v in sorted(urllib.parse.parse_qsl(query, keep_blank_values=True))
    )
    canon_h = "".join(f"{n}:{' '.join(headers.get(n, '').split())}\n" for n in signed)
    canon = "\n".join([method, path, canon_q, canon_h, ";".join(signed), payload])
    day = amz[:8]
    scope = f"{day}/{region}/s3/aws4_request"
    sts = "\n".join(["AWS4-HMAC-SHA256", amz, scope, hashlib.sha256(canon.encode()).hexdigest()])
    k = _h(("AWS4" + secret).encode(), day)
    for part in (region, "s3", "aws4_request"):
        k = _h(k, part)
    return hmac.new(k, sts.encode(), hashlib.sha256).hexdigest()


class MockS3:
    def __init__(self, bucket, access, secret, region="us-east-1", read_only=False):
        self.bucket, self.access, self.secret, self.region = bucket, access, secret, region
        self.read_only = read_only
        self.objects = {}  # key -> (bytes, last_modified)
        self.denied = 0
        self.lock = threading.Lock()
        outer = self

        class H(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *a):
                pass

            def _err(self, status, code):
                body = f"<?xml version='1.0'?><Error><Code>{code}</Code><Message>{code}</Message></Error>".encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/xml")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                if self.command != "HEAD":
                    self.wfile.write(body)

            def _auth(self):
                a = self.headers.get("Authorization", "")
                m = re.match(r"AWS4-HMAC-SHA256 Credential=([^/]+)/(\d{8})/([^/]+)/s3/aws4_request, SignedHeaders=([^,]+), Signature=([0-9a-f]+)$", a)
                if not m:
                    return "AccessDenied"
                ak, _day, region, signed, sig = m.groups()
                if ak != outer.access:
                    return "InvalidAccessKeyId"
                u = urllib.parse.urlsplit(self.path)
                headers = {k.lower(): v for k, v in self.headers.items()}
                want = sign(outer.secret, self.command, u.path, u.query, headers, signed.split(";"),
                            headers.get("x-amz-content-sha256", ""), headers.get("x-amz-date", ""), region)
                return None if hmac.compare_digest(want, sig) else "SignatureDoesNotMatch"

            def _body(self):
                n = int(self.headers.get("Content-Length", "0") or 0)
                return self.rfile.read(n) if n else b""

            def _key(self):
                u = urllib.parse.urlsplit(self.path)
                parts = urllib.parse.unquote(u.path).lstrip("/").split("/", 1)
                if parts[0] != outer.bucket:
                    return None, u
                return (parts[1] if len(parts) > 1 else ""), u

            def _dispatch(self):
                body = self._body()
                bad = self._auth()
                if bad:
                    outer.denied += 1
                    return self._err(403, bad)
                key, u = self._key()
                if key is None:
                    return self._err(404, "NoSuchBucket")
                if self.command in ("PUT", "DELETE") and outer.read_only:
                    outer.denied += 1
                    return self._err(403, "AccessDenied")
                if self.command == "PUT":
                    with outer.lock:
                        outer.objects[key] = (body, formatdate(usegmt=True))
                    etag = hashlib.md5(body).hexdigest()
                    self.send_response(200)
                    self.send_header("ETag", f'"{etag}"')
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                if self.command == "DELETE":
                    with outer.lock:
                        outer.objects.pop(key, None)
                    self.send_response(204)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                q = dict(urllib.parse.parse_qsl(u.query, keep_blank_values=True))
                if key == "" and self.command in ("GET", "HEAD"):
                    if self.command == "HEAD":
                        self.send_response(200)
                        self.send_header("Content-Length", "0")
                        self.end_headers()
                        return
                    return self._list(q)
                with outer.lock:
                    obj = outer.objects.get(key)
                if obj is None:
                    return self._err(404, "NoSuchKey")
                data, lm = obj
                etag = f'"{hashlib.md5(data).hexdigest()}"'
                status, out = 200, data
                rng = self.headers.get("Range")
                if rng and (m := re.match(r"bytes=(\d*)-(\d*)$", rng)):
                    a, b = m.groups()
                    start = int(a) if a else max(0, len(data) - int(b))
                    end = int(b) if (a and b) else len(data) - 1
                    out, status = data[start:end + 1], 206
                self.send_response(status)
                self.send_header("ETag", etag)
                self.send_header("Last-Modified", lm)
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Length", str(len(out)))
                self.end_headers()
                if self.command == "GET":
                    self.wfile.write(out)

            def _list(self, q):
                prefix, delim = q.get("prefix", ""), q.get("delimiter", "")
                maxk = int(q.get("max-keys", "1000"))
                with outer.lock:
                    keys = sorted(k for k in outer.objects if k.startswith(prefix))
                contents, prefixes = [], []
                for k in keys:
                    rest = k[len(prefix):]
                    if delim and delim in rest:
                        p = prefix + rest.split(delim, 1)[0] + delim
                        if p not in prefixes:
                            prefixes.append(p)
                    else:
                        contents.append(k)
                after = q.get("continuation-token") or q.get("start-after") or ""
                contents = [k for k in contents if k > after]
                trunc = len(contents) > maxk
                contents = contents[:maxk]
                xml = ["<?xml version='1.0' encoding='UTF-8'?><ListBucketResult xmlns='http://s3.amazonaws.com/doc/2006-03-01/'>",
                       f"<Name>{outer.bucket}</Name><Prefix>{escape(prefix)}</Prefix><KeyCount>{len(contents)}</KeyCount>",
                       f"<MaxKeys>{maxk}</MaxKeys><IsTruncated>{'true' if trunc else 'false'}</IsTruncated>"]
                if delim:
                    xml.append(f"<Delimiter>{escape(delim)}</Delimiter>")
                if trunc:
                    xml.append(f"<NextContinuationToken>{escape(contents[-1])}</NextContinuationToken>")
                for k in contents:
                    data, _ = outer.objects[k]
                    lm = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")
                    xml.append(f"<Contents><Key>{escape(k)}</Key><LastModified>{lm}</LastModified>"
                               f"<ETag>&quot;{hashlib.md5(data).hexdigest()}&quot;</ETag><Size>{len(data)}</Size>"
                               "<StorageClass>STANDARD</StorageClass></Contents>")
                for p in prefixes:
                    xml.append(f"<CommonPrefixes><Prefix>{escape(p)}</Prefix></CommonPrefixes>")
                xml.append("</ListBucketResult>")
                out = "".join(xml).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/xml")
                self.send_header("Content-Length", str(len(out)))
                self.end_headers()
                self.wfile.write(out)

            do_GET = do_PUT = do_HEAD = do_DELETE = _dispatch

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), H)
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    @property
    def endpoint(self):
        return f"http://127.0.0.1:{self.port}"

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.httpd.shutdown()
        self.httpd.server_close()
