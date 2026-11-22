# INC-49 search outage

## Impact

Timeouts at peak hours, about 14% of requests failed.

Reporter: 闵思远

Severity: sev3

Service: search

Reported: 2026-02-19

## Root cause

Pool settings were reset by the last release.

## Fix

Rolled back and added an alert.
