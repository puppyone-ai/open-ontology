# Super Drive model configuration (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| the model starts unconfigured | pass |  |
| the engine holds a key reference, never the value | pass | {"base_url":"http://127.0.0.1:58726/v1","configured":true,"key_ref":"file:/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-model-WJ2jeE/puppyone-development/super-ontology/secrets/model-key","model":"recorded-model-1","provider":"openai-compatible","revision":2,"status":"ok"} |
| the key is a private file | pass | 600 |
| the engine has the model on for the Drive | pass | {"backend":"fs","bytes_total":4690,"config":{"root":"/private/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-real-engine-014942-oIMNVO/drive"},"counts":{"published":20},"created_at":"2026-11 |
| the engine called the recorded model once per section | pass | 2 calls |
| the engine reports the usage | pass | {"drives":[{"cache_hits":0,"calls":2,"day":"2026-11-19","drive_id":"97362ca1-d201-4c4c-8020-9aeda1e8c5a6","files_limited":0,"limit":2000,"tokens_in":200,"tokens |
| Settings shows the day's usage | pass | Today 2 / 2,000 calls |

Timings: {}

