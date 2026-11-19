# Super Drive Cloud proxy mode (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| settings start in local mode, with the Cloud host named and no URL shown | pass |  |
| the data statement is the local one | pass |  |
| Cloud mode says where Shares go and that the files stay here | pass |  |
| the data statement turns to the Cloud wording | pass |  |
| without a Cloud session the panel says to sign in instead of asking for a project | pass |  |
| the choice is saved by the main process | pass |  |
| and it is still Cloud after the page reloads | pass |  |
| a Share in Cloud mode is refused with 'sign in' | pass | Sign in to puppyone Cloud to continue |
| and nothing of it reached the local engine or the Cloud host | pass | engine: [], cloud: [] |
| in local mode the Share goes to the local engine | pass | [{"method":"POST","url":"/v1/docs/context-packs:compile"},{"method":"POST","url":"/v1/docs/shares"},{"method":"POST","url":"/v1/docs/shares/00000000-0000-4000-8000-0000000000e1:revoke"}] |
| and the Cloud host still saw no request | pass |  |

Timings: {}

