# Super Drive OSS, COS and R2 buckets (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| oss: the dialog shows the fields of that cloud and no endpoint requirement | pass |  |
| oss: the engine got the backend, the cloud fields, the policy and the bucket | pass | {"bucket":"team-oss","access_key_id":{"secret_ref":"file:/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-enterprise-buckets-2UJuEH/puppyone-development/super-ontology/secrets/s3-a366f86771ee5108d4546e5324c7206c-access_key_id"},"secret_access_key":{"secret_ref":"file:/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-enterprise-buckets-2UJuEH/puppyone-development/super-ontology/secrets/s3-a366f86771ee5108d4546e5324c7206c-secret_access_key"},"region":"cn-hangzhou","policy":{"deny":["Payroll"]}} |
| oss: no key reached the engine, only file references | pass |  |
| oss: the keys are in private files next to the state folder | pass | 2 files |
| oss: the list names the kind and bucket, says read-only, shows the policy, and shows no key | pass | Hangzhou OSS / OSS bucket · 0 files / Sync now /  / team-oss / Never: Payroll / Credentials saved · Uploading from the app is off / Read-only: puppyone never changes files here |
| oss: the object is listed as published, and uploading is off for the bucket | pass |  |
| oss: a changed object shows as outdated | pass |  |
| oss: and published again once read | pass |  |
| oss: a deleted object ends as deleted | pass |  |
| cos: the dialog shows the fields of that cloud and no endpoint requirement | pass |  |
| cos: the engine got the backend, the cloud fields, the policy and the bucket | pass | {"bucket":"team-cos","access_key_id":{"secret_ref":"file:/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-enterprise-buckets-2UJuEH/puppyone-development/super-ontology/secrets/s3-18755dc3a105a9c580dbd347f183820f-access_key_id"},"secret_access_key":{"secret_ref":"file:/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-enterprise-buckets-2UJuEH/puppyone-development/super-ontology/secrets/s3-18755dc3a105a9c580dbd347f183820f-secret_access_key"},"region":"ap-beijing","policy":{"deny":["Payroll"]}} |
| cos: no key reached the engine, only file references | pass |  |
| cos: the keys are in private files next to the state folder | pass | 4 files |
| cos: the list names the kind and bucket, says read-only, shows the policy, and shows no key | pass | Beijing COS / COS bucket · 0 files / Sync now /  / team-cos / Never: Payroll / Credentials saved · Uploading from the app is off / Read-only: puppyone never changes files here |
| cos: the object is listed as published, and uploading is off for the bucket | pass |  |
| cos: a changed object shows as outdated | pass |  |
| cos: and published again once read | pass |  |
| cos: a deleted object ends as deleted | pass |  |
| r2: the dialog shows the fields of that cloud and no endpoint requirement | pass |  |
| r2: the engine got the backend, the cloud fields, the policy and the bucket | pass | {"bucket":"team-r2","access_key_id":{"secret_ref":"file:/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-enterprise-buckets-2UJuEH/puppyone-development/super-ontology/secrets/s3-0d0bfa003b6040420816c8a69c289ee2-access_key_id"},"secret_access_key":{"secret_ref":"file:/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-enterprise-buckets-2UJuEH/puppyone-development/super-ontology/secrets/s3-0d0bfa003b6040420816c8a69c289ee2-secret_access_key"},"account_id":"0123456789abcdef0123456789abcdef","policy":{"deny":["Payroll"]}} |
| r2: no key reached the engine, only file references | pass |  |
| r2: the keys are in private files next to the state folder | pass | 6 files |
| r2: the list names the kind and bucket, says read-only, shows the policy, and shows no key | pass | Edge R2 / R2 bucket · 0 files / Sync now /  / team-r2 / Never: Payroll / Credentials saved · Uploading from the app is off / Read-only: puppyone never changes files here |
| r2: the object is listed as published, and uploading is off for the bucket | pass |  |
| r2: a changed object shows as outdated | pass |  |
| r2: and published again once read | pass |  |
| r2: a deleted object ends as deleted | pass |  |

Timings: {}

