# INC-41 search outage

## Impact

Timeouts at peak hours, about 6% of requests failed.

Reporter: 闵思远

Severity: sev2

Service: search

Reported: 2026-02-06

## Root cause

Pool settings were reset by the last release.

## Fix

Rolled back and added an alert.
