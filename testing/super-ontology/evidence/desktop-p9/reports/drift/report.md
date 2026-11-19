# Super Drive drift and refresh (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| the engine mounted the share | pass | 200 active |
| the share starts active | pass |  |
| the share list shows 'Needs refresh' with the counts | pass |  |
| the toolbar and the sidebar carry a drift count | pass |  |
| the engine agrees: drifted | pass |  |
| the preview says what changed | pass |  |
| the old share ends as refreshed | pass | refreshed |
| a new active share links back to it | pass | active |
| the list shows the chain note on the new share | pass |  |
| the folder now holds the new share's BRIEF with what changed since the last version | pass | 977 bytes |
| the folder's SHARE.json names the new share | pass | e8f71545-3506-416b-8171-d2e11d8071e1 |

Timings: {"editToDriftSeconds":4.7}

