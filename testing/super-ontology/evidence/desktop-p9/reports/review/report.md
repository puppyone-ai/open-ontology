# Super Drive review queue (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| the engine accepts the recorded model | pass | 200 {"base_url":"http://127.0.0.1:58480/v1","configured":true,"key_ref":"file:/private/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-review-O9xbag/model-secrets/model-key","model":"recorded-model-1","provider":"openai-compatible","revision":2,"status":"ok"} {"configured":false,"revision":1,"status":"not_configured"} |
| the Drive uses the model | pass | 200 |
| the engine holds a pending item for a person | pass | low_confidence_fact |
| the sidebar counts the pending items | pass | 1 vs 1 |
| the queue is grouped by source file | pass |  |
| the page shows the count and a version | pass |  |
| a conflict has no whole-source accept (a person must choose a side) | pass |  |
| the result is one status line | pass |  |
| the engine's queue is empty | pass | 0 left |
| the engine recorded the decision as accepted | pass | 1 accepted |
| the page shows the engine's newest version | pass | v2 -> v2 |
| the page shows what the engine holds | pass | 0 pending in the engine |
| the page shows the empty queue | pass |  |

Timings: {}

