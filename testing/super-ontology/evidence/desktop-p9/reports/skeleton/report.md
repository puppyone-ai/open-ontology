# Super Drive walking skeleton (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| the real engine starts and the pill is healthy | pass | 0.7s |
| D79 the local-data line is stated on the first-run page | pass | Your files and processing results always stay on this computer and are never uploaded to the puppyone cloud, unless you choose puppyone Cloud. |
| A1 the list shows all 20 files published | pass | {"published":20} |
| A1 within 60 s of registering | pass | 4.5s |
| the engine agrees: every file published | pass | 20 files |
| A6 the freshness indicator showed processing and then up_to_date | pass | up_to_date -> processing -> publishing -> syncing -> up_to_date |
| A6 the up_to_date text carries the ontology version | pass | Up to date · v1 · now |
| A4 the UI shows the engine as not connected | pass | restarting |
| A4 the engine reconnects by itself | pass | 6.1s, new daemon |
| A4 no state lost: every earlier file is unchanged | pass | 20 files unchanged |
| A4 the event head moved on after the reconnect (events were emitted after the cursor) | pass | 151 -> 165 |
| A4 the new file reached published in the list | pass | published |
| A4 the list has 21 published rows | pass | {"published":21} |
| A5 the edited file left published and came back within 30 s | pass | published > queued > parsed > indexed > published in 4.5s |
| A5 the engine ends in published for the edited file | pass |  |

Timings: {"engineStartSeconds":0.7,"acceptance1PublishedSeconds":4.5,"disconnectShownMs":52,"reconnectSeconds":6.1,"acceptance5RerunSeconds":4.5}

