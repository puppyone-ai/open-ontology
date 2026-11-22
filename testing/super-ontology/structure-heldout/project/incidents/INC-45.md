# INC-45 search outage

## Impact

Timeouts at peak hours, about 10% of requests failed.

Reporter: 洪嘉树

Severity: sev3

Service: search

Reported: 2026-02-26

## Root cause

Pool settings were reset by the last release.

## Fix

Rolled back and added an alert.
