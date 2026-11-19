# Super Drive without Docker (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| without Docker the setup check ends at 'stopped', ready to start | pass | Project A
No Git
0
Project files
Super Drive
No Drive
Register new Drive…
Files
Review
Suggestions
Shares
Engine
Settings
Super Drive
Stopped
Local engine is stopped

Super Drive runs on this computer |
| the checklist has no Docker item and the page does not say the word | pass |  |
| the engine was started as `oo serve --runtime host` | pass | ["serve","--state-dir","/var/folders/15/d96j9n314fd9w_9dy15qwyrr0000gn/T/puppyone-super-drive-docker-free-1dSD96/puppyone-development/super-ontology/state","--port","0","--json","--runtime","host"] |
| and no compose project, port or Docker argument was given | pass |  |
| the healthy engine page names the built-in database and the sidecar, and says it runs without Docker | pass | Running without Docker, on the built-in database. A change applies the next time the engine starts. |
| the page still has no Docker item | pass |  |
| choosing Docker brings the Docker check back | pass |  |
| and the built-in runtime takes it away again | pass |  |

Timings: {"engineToFirstScreenMs":1685}

