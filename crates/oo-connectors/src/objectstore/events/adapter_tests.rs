//! The pulling adapters against local fakes: a fake cloud answers from a closure, a tiny local HTTP
//! server stands in for the network. Nothing here reaches a real cloud and no account exists.

use std::time::Duration;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::cfqueues::CloudflareQueue;
use super::crypto::{amz_date, base64_decode, base64_encode};
use super::http::{HttpRequest, split_endpoint};
use super::mns::{MnsQueue, mns_authorization};
use super::queue::fake::{FakeCloud, reply};
use super::queue::{EventQueue, ack, pull};
use super::sqs::{AwsCredentials, SqsQueue, sigv4_authorization};
use super::{Deduper, Verdict};
use crate::objectstore::{ChangeOp, DocSourceError};

/// 2026-11-18T10:00:00Z.
fn t0() -> i64 {
    1_794_996_000_000
}

fn creds() -> AwsCredentials {
    AwsCredentials {
        access_key_id: "AKIDSYNTHETIC".into(),
        secret_access_key: Zeroizing::new("synthetic-secret".into()),
        session_token: None,
    }
}

fn sqs(cloud: FakeCloud) -> SqsQueue<FakeCloud> {
    let mut q = SqsQueue::new(
        cloud,
        "https://sqs.eu-west-1.amazonaws.com",
        "eu-west-1",
        "https://sqs.eu-west-1.amazonaws.com/000000000000/oo-events",
        creds(),
    )
    .unwrap();
    q.now_ms = t0;
    q
}

#[test]
fn sigv4_matches_the_get_vanilla_vector_of_the_aws_test_suite() {
    let c = AwsCredentials {
        access_key_id: "AKIDEXAMPLE".into(),
        secret_access_key: Zeroizing::new("wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into()),
        session_token: None,
    };
    let auth = sigv4_authorization(
        "GET",
        "/",
        "",
        &[
            ("Host".into(), "example.amazonaws.com".into()),
            ("X-Amz-Date".into(), "20150830T123600Z".into()),
        ],
        b"",
        "us-east-1",
        "service",
        &c,
        "20150830T123600Z",
        "20150830",
    );
    assert_eq!(
        auth,
        "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
         SignedHeaders=host;x-amz-date, \
         Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
    );
}

#[test]
fn endpoints_are_origins() {
    assert_eq!(
        split_endpoint("https://sqs.eu-west-1.amazonaws.com/x").unwrap(),
        (
            "https://sqs.eu-west-1.amazonaws.com".into(),
            "sqs.eu-west-1.amazonaws.com".into()
        )
    );
    assert!(split_endpoint("sqs.eu-west-1.amazonaws.com").is_err());
    assert!(split_endpoint("ftp://x").is_err());
    assert!(split_endpoint("https://user@x").is_err());
}

fn sqs_cloud() -> FakeCloud {
    let sns = include_str!("fixtures/sns-wrapped-s3.json");
    FakeCloud::new(move |req| {
        let target = req.header("x-amz-target").unwrap_or_default();
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        match target {
            "AmazonSQS.ReceiveMessage" => reply(
                200,
                &json!({"Messages": [
                    {"MessageId": "m1", "ReceiptHandle": "rh-1", "Body": sns},
                    {"MessageId": "m2", "ReceiptHandle": "rh-2", "Body": "this is not an event"},
                    {"MessageId": "m3", "ReceiptHandle": "rh-3",
                     "Body": json!({"Records": [{"eventName": "s3:TestEvent", "s3": {"object": {"key": "drive/t.md"}}}]}).to_string()},
                    {"MessageId": "m4", "ReceiptHandle": "rh-4",
                     "Body": json!({"Records": [{"eventName": "ObjectRemoved:Delete", "eventTime": "2026-11-18T09:59:30Z",
                        "s3": {"object": {"key": "other/x.md", "sequencer": "01"}}}]}).to_string()},
                ]})
                .to_string(),
            ),
            "AmazonSQS.DeleteMessageBatch" => {
                let n = body["Entries"].as_array().map_or(0, Vec::len);
                reply(200, &json!({"Successful": vec![json!({"Id": "0"}); n], "Failed": []}).to_string())
            }
            _ => reply(400, r#"{"__type":"InvalidAction"}"#),
        }
    })
}

#[tokio::test]
async fn sqs_pull_signs_parses_and_acknowledges_after_handling() {
    let cloud = sqs_cloud();
    let q = sqs(cloud.clone());
    let pulled = pull(&q, "drive/", t0(), 10, Duration::from_secs(20))
        .await
        .unwrap();
    assert_eq!(pulled.messages, 4);
    assert_eq!(
        pulled.hints.len(),
        1,
        "the SNS envelope is unwrapped, the test event and the other prefix give nothing"
    );
    assert_eq!(pulled.hints[0].raw_key, b"Projects/plan.md");
    assert_eq!(
        (pulled.hints[0].op, pulled.hints[0].version_id.as_deref()),
        (ChangeOp::Put, Some("v7"))
    );
    assert_eq!(
        pulled.receipts,
        ["rh-1", "rh-3", "rh-4"],
        "what parsed is acknowledged, a foreign prefix included"
    );
    assert_eq!(pulled.rejected.len(), 1);
    assert_eq!(
        pulled.rejected[0].0, "m2",
        "an unreadable body is left for the queue"
    );

    // The request is what SQS expects, and the signature is the one the documented algorithm gives.
    let req: HttpRequest = cloud.requests().remove(0);
    assert_eq!(
        (req.method, req.url.as_str()),
        ("POST", "https://sqs.eu-west-1.amazonaws.com/")
    );
    assert_eq!(
        req.header("content-type"),
        Some("application/x-amz-json-1.0")
    );
    assert!(req.header("host").is_none(), "the transport sets Host");
    let body: Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(
        (
            body["MaxNumberOfMessages"].as_u64(),
            body["WaitTimeSeconds"].as_u64()
        ),
        (Some(10), Some(20))
    );
    let (datetime, day) = amz_date(t0());
    assert_eq!(req.header("x-amz-date"), Some(datetime.as_str()));
    let again = sigv4_authorization(
        "POST",
        "/",
        "",
        &[
            ("content-type".into(), "application/x-amz-json-1.0".into()),
            ("host".into(), "sqs.eu-west-1.amazonaws.com".into()),
            ("x-amz-date".into(), datetime.clone()),
            ("x-amz-target".into(), "AmazonSQS.ReceiveMessage".into()),
        ],
        &req.body,
        "eu-west-1",
        "sqs",
        &creds(),
        &datetime,
        &day,
    );
    assert_eq!(req.header("authorization"), Some(again.as_str()));
    assert!(again.starts_with(
        "AWS4-HMAC-SHA256 Credential=AKIDSYNTHETIC/20261118/eu-west-1/sqs/aws4_request"
    ));
    assert!(!format!("{:?}", creds()).contains("synthetic-secret"));

    // Nothing is deleted before the caller says so.
    assert_eq!(cloud.requests().len(), 1);
    ack(&q, &pulled).await.unwrap();
    let del: Value = serde_json::from_slice(&cloud.requests()[1].body).unwrap();
    let handles: Vec<&str> = del["Entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["ReceiptHandle"].as_str().unwrap())
        .collect();
    assert_eq!(handles, ["rh-1", "rh-3", "rh-4"]);
}

#[tokio::test]
async fn sqs_deletes_in_batches_of_ten_and_reports_a_partial_failure() {
    let cloud = sqs_cloud();
    let q = sqs(cloud.clone());
    let receipts: Vec<String> = (0..23).map(|i| format!("rh-{i}")).collect();
    q.ack(&receipts).await.unwrap();
    let sizes: Vec<usize> = cloud
        .requests()
        .iter()
        .map(|r| {
            serde_json::from_slice::<Value>(&r.body).unwrap()["Entries"]
                .as_array()
                .unwrap()
                .len()
        })
        .collect();
    assert_eq!(sizes, [10, 10, 3]);

    let partial = sqs(FakeCloud::new(|_| {
        reply(
            200,
            r#"{"Successful":[],"Failed":[{"Id":"0","Code":"ReceiptHandleIsInvalid"}]}"#,
        )
    }));
    assert_eq!(
        partial.ack(&["x".into()]).await,
        Err(DocSourceError::Io("queue did not delete every message"))
    );
}

#[tokio::test]
async fn sqs_errors_are_the_closed_set() {
    for (status, body, want) in [
        (403, "{}", DocSourceError::PermissionDenied),
        (
            400,
            r#"{"__type":"com.amazonaws.sqs#QueueDoesNotExist","message":"x"}"#,
            DocSourceError::Protocol("queue replied unexpectedly"),
        ),
        (
            400,
            r#"{"__type":"AWS.SimpleQueueService.NonExistentQueue"}"#,
            DocSourceError::NotFound,
        ),
        (503, "", DocSourceError::Unavailable),
    ] {
        let q = sqs(FakeCloud::new(move |_| reply(status, body)));
        assert_eq!(
            q.receive(1, Duration::ZERO).await.unwrap_err(),
            want,
            "{status} {body}"
        );
    }
    let q = sqs(FakeCloud::new(|_| reply(200, "not json")));
    assert_eq!(
        q.receive(1, Duration::ZERO).await.unwrap_err(),
        DocSourceError::Protocol("queue reply is not json")
    );
    // A session token is signed too.
    let mut c = creds();
    c.session_token = Some(Zeroizing::new("tok".into()));
    let cloud = FakeCloud::new(|_| reply(200, r#"{"Messages":[]}"#));
    let mut q = SqsQueue::new(
        cloud.clone(),
        "https://sqs.eu-west-1.amazonaws.com",
        "eu-west-1",
        "https://sqs.eu-west-1.amazonaws.com/0/q",
        c,
    )
    .unwrap();
    q.now_ms = t0;
    assert!(q.receive(1, Duration::ZERO).await.unwrap().is_empty());
    let r = &cloud.requests()[0];
    assert_eq!(r.header("x-amz-security-token"), Some("tok"));
    assert!(
        r.header("authorization")
            .unwrap()
            .contains("x-amz-security-token")
    );
    assert!(
        SqsQueue::new(
            FakeCloud::new(|_| reply(200, "")),
            "https://x",
            "eu west",
            "https://q",
            creds()
        )
        .is_err()
    );
}

#[tokio::test]
async fn a_message_delivered_again_is_absorbed_by_the_deduper() {
    // At least once: the queue gives the same message on a second pull because nothing was deleted.
    let q = sqs(sqs_cloud());
    let first = pull(&q, "drive/", t0(), 10, Duration::ZERO).await.unwrap();
    let second = pull(&q, "drive/", t0() + 5000, 10, Duration::ZERO)
        .await
        .unwrap();
    let mut dedupe = Deduper::default();
    let v1: Vec<Verdict> = first.hints.iter().map(|h| dedupe.admit(h)).collect();
    let v2: Vec<Verdict> = second.hints.iter().map(|h| dedupe.admit(h)).collect();
    assert_eq!((v1, v2), (vec![Verdict::Fresh], vec![Verdict::Duplicate]));
}

fn cf(cloud: FakeCloud) -> CloudflareQueue<FakeCloud> {
    CloudflareQueue::new(
        cloud,
        "https://api.cloudflare.com",
        "acct1",
        "queue-1",
        Zeroizing::new("synthetic-token".into()),
    )
    .unwrap()
}

#[tokio::test]
async fn cloudflare_queues_pull_and_ack() {
    let cloud = FakeCloud::new(|req| {
        if req.url.ends_with("/messages/pull") {
            reply(
                200,
                &json!({"success": true, "errors": [], "result": {"message_backlog_count": 0, "messages": [
                    {"id": "a", "lease_id": "lease-a", "timestamp_ms": 1, "attempts": 1,
                     "body": {"account": "a", "action": "PutObject", "bucket": "b", "object": {"key": "drive/Notes/n.md", "size": 3}, "eventTime": "2026-11-18T09:59:00Z"}},
                    {"id": "b", "lease_id": "lease-b", "timestamp_ms": 2, "attempts": 1,
                     "body": json!({"action": "DeleteObject", "object": {"key": "drive/old.md"}, "eventTime": "2026-11-18T09:59:10Z"}).to_string()},
                    {"id": "c", "lease_id": "lease-c", "body": "hello"}
                ]}})
                .to_string(),
            )
        } else {
            reply(
                200,
                r#"{"success":true,"result":{"ackCount":2,"retryCount":0}}"#,
            )
        }
    });
    let q = cf(cloud.clone());
    let p = pull(&q, "drive/", t0(), 50, Duration::ZERO).await.unwrap();
    let got: Vec<(Vec<u8>, ChangeOp)> = p.hints.iter().map(|h| (h.raw_key.clone(), h.op)).collect();
    assert_eq!(
        got,
        [
            (b"Notes/n.md".to_vec(), ChangeOp::Put),
            (b"old.md".to_vec(), ChangeOp::Delete)
        ]
    );
    assert_eq!(p.receipts, ["lease-a", "lease-b"]);
    assert_eq!(p.rejected.len(), 1);
    ack(&q, &p).await.unwrap();
    let reqs = cloud.requests();
    assert_eq!(
        reqs[0].url,
        "https://api.cloudflare.com/client/v4/accounts/acct1/queues/queue-1/messages/pull"
    );
    assert_eq!(
        reqs[0].header("authorization"),
        Some("Bearer synthetic-token")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&reqs[0].body).unwrap()["batch_size"],
        50
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&reqs[1].body).unwrap(),
        json!({"acks": [{"lease_id": "lease-a"}, {"lease_id": "lease-b"}], "retries": []})
    );
    assert!(reqs[1].url.ends_with("/messages/ack"));
}

#[tokio::test]
async fn cloudflare_queues_refuses_what_it_cannot_trust() {
    let q = cf(FakeCloud::new(|_| {
        reply(200, r#"{"success":false,"errors":[{"code":1}]}"#)
    }));
    assert_eq!(
        q.receive(1, Duration::ZERO).await.unwrap_err(),
        DocSourceError::Protocol("queue reported a failure")
    );
    let q = cf(FakeCloud::new(|_| reply(401, "{}")));
    assert_eq!(
        q.receive(1, Duration::ZERO).await.unwrap_err(),
        DocSourceError::AuthFailed
    );
    let q = cf(FakeCloud::new(|_| reply(429, "{}")));
    assert!(matches!(
        q.receive(1, Duration::ZERO).await.unwrap_err(),
        DocSourceError::Throttled { .. }
    ));
    let mk = |acct: &str, queue: &str, tok: &str| {
        CloudflareQueue::new(
            FakeCloud::new(|_| reply(200, "")),
            "https://api.cloudflare.com",
            acct,
            queue,
            Zeroizing::new(tok.into()),
        )
        .is_err()
    };
    assert!(mk("a/b", "q", "t") && mk("a", "../q", "t") && mk("a", "q", "") && mk("", "q", "t"));
    assert!(!format!("{:?}", cf(FakeCloud::new(|_| reply(200, "")))).contains("synthetic-token"));
}

fn mns(cloud: FakeCloud) -> MnsQueue<FakeCloud> {
    let mut q = MnsQueue::new(
        cloud,
        "https://1234567890.mns.cn-hangzhou.aliyuncs.com",
        "oo-events",
        "LTAISYNTHETIC",
        Zeroizing::new("synthetic-secret".into()),
    )
    .unwrap();
    q.now_ms = t0;
    q
}

fn oss_body(key: &str, name: &str) -> String {
    json!({"events": [{"eventName": name, "eventTime": "2026-11-18T09:59:00.000Z", "oss": {"object": {"key": key}}}]}).to_string()
}

#[tokio::test]
async fn mns_receive_decodes_bodies_and_deletes_in_one_request() {
    let b64 =
        base64_encode(oss_body("drive/Meetings/m.docx", "ObjectCreated:PutObject").as_bytes());
    let plain = oss_body("drive/x.md", "ObjectRemoved:DeleteObject").replace('"', "&quot;");
    let xml = format!(
        "<?xml version=\"1.0\"?><Messages xmlns=\"http://mns.aliyuncs.com/doc/v1/\">\
         <Message><MessageId>M1</MessageId><ReceiptHandle>1-ODU4OTkzNDU5My0xNDM1&amp;7</ReceiptHandle><MessageBody>{b64}</MessageBody></Message>\
         <Message><MessageId>M2</MessageId><ReceiptHandle>2-AAAA</ReceiptHandle><MessageBody>{plain}</MessageBody></Message></Messages>"
    );
    let cloud = FakeCloud::new(move |req| match req.method {
        "GET" => reply(200, &xml),
        _ => reply(204, ""),
    });
    let q = mns(cloud.clone());
    let p = pull(&q, "drive/", t0(), 16, Duration::from_secs(30))
        .await
        .unwrap();
    let got: Vec<(Vec<u8>, ChangeOp)> = p.hints.iter().map(|h| (h.raw_key.clone(), h.op)).collect();
    assert_eq!(
        got,
        [
            (b"Meetings/m.docx".to_vec(), ChangeOp::Put),
            (b"x.md".to_vec(), ChangeOp::Delete)
        ]
    );
    assert_eq!(
        p.receipts,
        ["1-ODU4OTkzNDU5My0xNDM1&7", "2-AAAA"],
        "entities in a receipt handle are decoded"
    );
    ack(&q, &p).await.unwrap();

    let reqs = cloud.requests();
    assert_eq!(
        reqs[0].url,
        "https://1234567890.mns.cn-hangzhou.aliyuncs.com/queues/oo-events/messages?numOfMessages=16&waitseconds=30"
    );
    // The signature is the documented one over the request the adapter sent.
    let want = mns_authorization(
        "LTAISYNTHETIC",
        "synthetic-secret",
        "GET",
        "text/xml;charset=utf-8",
        "Wed, 18 Nov 2026 10:00:00 GMT",
        "/queues/oo-events/messages?numOfMessages=16&waitseconds=30",
    );
    assert_eq!(reqs[0].header("authorization"), Some(want.as_str()));
    assert_eq!(
        reqs[0].header("date"),
        Some("Wed, 18 Nov 2026 10:00:00 GMT")
    );
    assert_eq!(reqs[0].header("x-mns-version"), Some("2015-06-06"));
    assert_eq!(reqs[1].method, "DELETE");
    let del = String::from_utf8(reqs[1].body.clone()).unwrap();
    assert!(
        del.contains("<ReceiptHandle>1-ODU4OTkzNDU5My0xNDM1&amp;7</ReceiptHandle>")
            && del.contains("<ReceiptHandle>2-AAAA</ReceiptHandle>")
    );
    assert!(base64_decode("TVM=").is_some());
}

#[tokio::test]
async fn mns_empty_queue_and_faults() {
    let empty = mns(FakeCloud::new(|_| {
        reply(
            404,
            "<Error><Code>MessageNotExist</Code><Message>Message not exist.</Message></Error>",
        )
    }));
    assert!(empty.receive(1, Duration::ZERO).await.unwrap().is_empty());
    let gone = mns(FakeCloud::new(|_| {
        reply(404, "<Error><Code>QueueNotExist</Code></Error>")
    }));
    assert_eq!(
        gone.receive(1, Duration::ZERO).await.unwrap_err(),
        DocSourceError::NotFound
    );
    let denied = mns(FakeCloud::new(|_| reply(403, "")));
    assert_eq!(
        denied.receive(1, Duration::ZERO).await.unwrap_err(),
        DocSourceError::PermissionDenied
    );
    let half = mns(FakeCloud::new(|_| {
        reply(
            200,
            "<Messages><Message><MessageId>x</MessageId></Message></Messages>",
        )
    }));
    assert_eq!(
        half.receive(1, Duration::ZERO).await.unwrap_err(),
        DocSourceError::Protocol("queue message is incomplete")
    );
    assert!(
        MnsQueue::new(
            FakeCloud::new(|_| reply(200, "")),
            "https://x.example",
            "bad/queue",
            "id",
            Zeroizing::new("s".into())
        )
        .is_err()
    );
    assert!(!format!("{:?}", mns(FakeCloud::new(|_| reply(200, "")))).contains("synthetic-secret"));
}

/// The same SQS adapter over a real socket: `ReqwestTransport` to a local fake server (no cloud).
#[cfg(feature = "objectstore-s3")]
#[tokio::test]
async fn sqs_over_a_real_local_socket() {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::http::ReqwestTransport;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let cap = captured.clone();
    let server = tokio::spawn(async move {
        for n in 0..2 {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let (head_end, len) = loop {
                let k = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..k]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    break (i + 4, len);
                }
            };
            while buf.len() < head_end + len {
                let k = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..k]);
            }
            let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
            let body = String::from_utf8_lossy(&buf[head_end..head_end + len]).into_owned();
            cap.lock().unwrap().push((head, body));
            let out = if n == 0 {
                json!({"Messages": [{"MessageId": "m", "ReceiptHandle": "rh", "Body": json!({"hints": [{"key": "drive/a.md", "op": "put", "sequencer": "1", "at_ms": 5}]}).to_string()}]}).to_string()
            } else {
                r#"{"Successful":[{"Id":"0"}],"Failed":[]}"#.to_owned()
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/x-amz-json-1.0\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{out}",
                out.len()
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
        }
    });
    let transport = ReqwestTransport::new(Duration::from_secs(5)).unwrap();
    let mut q = SqsQueue::new(
        transport,
        &format!("http://127.0.0.1:{port}"),
        "eu-west-1",
        "http://127.0.0.1/000000000000/oo-events",
        creds(),
    )
    .unwrap();
    q.now_ms = t0;
    let p = pull(&q, "drive/", t0(), 10, Duration::ZERO).await.unwrap();
    assert_eq!(p.hints.len(), 1);
    ack(&q, &p).await.unwrap();
    server.await.unwrap();
    let c = captured.lock().unwrap();
    let head = c[0].0.to_ascii_lowercase();
    assert!(head.starts_with("post / http/1.1"), "{head}");
    assert!(head.contains(&format!("host: 127.0.0.1:{port}")), "{head}");
    assert!(
        head.contains("x-amz-target: amazonsqs.receivemessage"),
        "{head}"
    );
    assert!(head.contains("authorization: aws4-hmac-sha256 credential=akidsynthetic/20261118/eu-west-1/sqs/aws4_request"), "{head}");
    assert!(c[1].1.contains("\"ReceiptHandle\":\"rh\""));
}
