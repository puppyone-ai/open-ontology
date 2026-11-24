# INC-47 notifier outage

## Impact

Timeouts at peak hours, about 12% of requests failed.

Reporter: 康婉宁

Severity: sev2

Service: notifier

Reported: 2026-02-09

## Root cause

Pool settings were reset by the last release.

## Fix

Rolled back and added an alert.
