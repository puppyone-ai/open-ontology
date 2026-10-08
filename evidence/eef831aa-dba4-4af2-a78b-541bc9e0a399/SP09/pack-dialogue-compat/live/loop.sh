#!/bin/bash
# Compiled pack, development-local propose, then execute only after a local approval file.
set -u
EV=/evidence/live
WORK=/work
mkdir -p "$EV" "$WORK"
printf '%s\n' no > "$EV/execute-invoked.txt"
printf '%s\n' no > "$EV/approval-written.txt"

/bins/forward raw 127.0.0.1:5432 oo-a399-pg:5432 > "$EV/forward-pg.log" 2>&1 &
echo $! > "$EV/forward-pg.pid"
/bins/forward http 127.0.0.1:8317 host.docker.internal:8317 "$EV/model-response.txt" > "$EV/forward-http.log" 2>&1 &
echo $! > "$EV/forward-http.pid"
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if (echo >/dev/tcp/127.0.0.1/5432) >/dev/null 2>&1 && (echo >/dev/tcp/127.0.0.1/8317) >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.2
done
if [ "$ready" -ne 1 ]; then
  echo "forwarder did not listen" >&2
  printf '%s\n' 1 > "$EV/loop.exit"
  exit 1
fi

/bins/role-pack-bundle --request /evidence/live/pack-command.json --output "$WORK/bundle.json" > "$EV/bundle.stdout" 2> "$EV/bundle.stderr"
printf '%s\n' $? > "$EV/bundle.exit"
if [ "$(cat "$EV/bundle.exit")" -ne 0 ]; then
  printf '%s\n' 4 > "$EV/loop.exit"
  exit 4
fi
/bins/live-tool extract-pack "$WORK/bundle.json" "$WORK/pack.json" > "$EV/pack-extract.txt"
printf '%s\n' $? > "$EV/pack-extract.exit"
if [ "$(cat "$EV/pack-extract.exit")" -ne 0 ]; then
  printf '%s\n' 4 > "$EV/loop.exit"
  exit 4
fi

cat > "$WORK/question.json" <<'EOF'
{"protocol":"oo.ontology-dialogue.question/v1","text":"How many rows are stored in public.entry?"}
EOF
cat > "$WORK/propose.json" <<'EOF'
{
  "protocol": "oo.ontology-dialogue.propose-config/v1",
  "question_file": "/work/question.json",
  "pack_file": "/work/pack.json",
  "binding": {
    "role": "oo_r_clerk",
    "project": "shop",
    "policy_id": "e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e",
    "policy_version": "1",
    "knowledge_id": "know_a317",
    "knowledge_version": "1",
    "as_of": "2026-10-08T00:00:00Z"
  },
  "endpoint": {
    "origin": "http://127.0.0.1:8317",
    "path": "/v1/chat/completions",
    "model_id": "grok-4.7",
    "transport": "development_local"
  },
  "trust": {"ca_file": "/work/missing-ca.pem", "residency_approved": true},
  "secret": {"bearer_file": "/token/llm-token"},
  "budget": {
    "max_question_bytes": 8192,
    "max_context_bytes": 65536,
    "max_request_bytes": 65536,
    "max_response_bytes": 16384,
    "max_response_chunks": 32,
    "deadline_ms": 30000,
    "max_prompt_tokens": 20000,
    "max_completion_tokens": 1024,
    "max_total_tokens": 22000,
    "max_sql_bytes": 4096
  }
}
EOF

/bins/live-tool prepare "$WORK" > "$EV/prepare.txt"
printf '%s\n' $? > "$EV/prepare.exit"
if [ "$(cat "$EV/prepare.exit")" -ne 0 ]; then
  printf '%s\n' 1 > "$EV/loop.exit"
  exit 1
fi
/bins/install-column-acl < "$WORK/install.json" > "$EV/install.stdout" 2> "$EV/install.stderr"
printf '%s\n' $? > "$EV/install.exit"
/bins/provision-mapping-reader < "$WORK/provision.json" > "$EV/provision.stdout" 2> "$EV/provision.stderr"
printf '%s\n' $? > "$EV/provision.exit"
if [ "$(cat "$EV/install.exit")" -ne 0 ] || [ "$(cat "$EV/provision.exit")" -ne 0 ]; then
  printf '%s\n' 1 > "$EV/loop.exit"
  exit 1
fi

/bins/ontology-dialogue propose --config "$WORK/propose.json" > "$WORK/proposal.json" 2> "$EV/propose.stderr"
printf '%s\n' $? > "$EV/propose.exit"
if [ "$(cat "$EV/propose.exit")" -ne 0 ]; then
  printf '%s\n' no > "$EV/execute-invoked.txt"
  printf '%s\n' no > "$EV/approval-written.txt"
  printf '%s\n' "$(cat "$EV/propose.exit")" > "$EV/loop.exit"
  exit "$(cat "$EV/propose.exit")"
fi
/bins/live-tool scan-secret "$WORK/proposal.json" /token/llm-token > "$EV/proposal-secret.txt"
if [ "$(cat "$EV/proposal-secret.txt")" != "secret_absent" ]; then
  rm -f "$WORK/proposal.json"
  printf '%s\n' 1 > "$EV/loop.exit"
  exit 1
fi
cp "$WORK/proposal.json" "$EV/proposal.json"
QUERY_SHA=$(sha256sum /bins/query-pg | awk '{print $1}')
/bins/live-tool gateway "$WORK/proposal.json" /work/pack.json "$QUERY_SHA" "$WORK" > "$EV/gateway-tool.txt"
printf '%s\n' $? > "$EV/gateway-tool.exit"
if [ "$(cat "$EV/gateway-tool.exit")" -ne 0 ]; then
  printf '%s\n' no > "$EV/execute-invoked.txt"
  printf '%s\n' 4 > "$EV/loop.exit"
  exit 4
fi
printf '%s\n' yes > "$EV/approval-written.txt"
cp "$WORK/approval.json" "$EV/approval.json"
cp "$WORK/request.json" "$EV/request.json"
printf '%s\n' yes > "$EV/execute-invoked.txt"
/bins/ontology-dialogue execute --config "$WORK/execute.json" > "$EV/execute.stdout" 2> "$EV/execute.stderr"
printf '%s\n' $? > "$EV/execute.exit"
/bins/live-tool scan-secret "$EV/execute.stdout" /token/llm-token > "$EV/execute-secret.txt" || true
if [ -f "$EV/model-response.txt" ]; then
  /bins/live-tool scan-secret "$EV/model-response.txt" /token/llm-token > "$EV/model-secret.txt" || {
    rm -f "$EV/model-response.txt"
    printf '%s\n' secret_present > "$EV/model-secret.txt"
  }
fi
printf '%s\n' "$(cat "$EV/execute.exit")" > "$EV/loop.exit"
exit "$(cat "$EV/execute.exit")"
