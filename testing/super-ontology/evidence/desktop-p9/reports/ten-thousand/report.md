# Super Drive 10000 files (T1, real engine)

Result: PASS  | input: synthetic Chromium

| check | result | detail |
|---|---|---|
| first paint of a 10000 file Drive is under 1000 ms | pass | 128 ms (first row after 115 ms) |
| the list knows all of them (rows announced to assistive technology) | pass | 10001 |
| at most 120 rows are mounted | pass |  |
| the whole list was scrolled to its end | pass | last mounted row index 10001 of 10001 |
| scrolling back up over the loaded list: frame interval p95 is at most 16.7 ms | pass | p50 8.3, p95 10, p99 16.7, max 18.1 ms over 998 frames |
| scrolling down with the pages loading: frame interval p95 is at most 16.7 ms | pass | p50 8.3, p95 10, p99 16.8, max 25 ms over 998 frames |
| no frame takes more than 100 ms | pass | down 25 ms, up 18.1 ms |
| 10000 state changes at once leave no frame over 100 ms | pass | p95 9.4, max 58.4 ms, 0 long tasks (longest 0 ms), first row updated after 175 ms |
| the rows show the new state | pass |  |

Timings: {"firstPaintMs":128,"idleBaseline":{"frames":600,"p50":8.3,"p95":9.6,"p99":10.3,"max":10.4},"scrollDown":{"frames":998,"p50":8.3,"p95":10,"p99":16.8,"max":25},"scrollUp":{"frames":998,"p50":8.3,"p95":10,"p99":16.7,"max":18.1},"longTasksMs":[71],"flood":{"frames":193,"p95":9.4,"max":58.4,"longTasks":0,"longestTaskMs":0,"firstRowMs":175}}

