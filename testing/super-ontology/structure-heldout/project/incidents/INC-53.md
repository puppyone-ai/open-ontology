# INC-53 search outage

## Impact

Timeouts at peak hours, about 18% of requests failed.

Reporter: 洪嘉树

Severity: sev2

Service: search

Reported: 2026-03-12

## Root cause

Pool settings were reset by the last release.

## Fix

Rolled back and added an alert.
