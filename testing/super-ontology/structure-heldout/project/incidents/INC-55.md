# INC-55 notifier outage

## Impact

Timeouts at peak hours, about 20% of requests failed.

Reporter: 康婉宁

Severity: sev3

Service: notifier

Reported: 2026-03-22

## Root cause

Pool settings were reset by the last release.

## Fix

Rolled back and added an alert.
