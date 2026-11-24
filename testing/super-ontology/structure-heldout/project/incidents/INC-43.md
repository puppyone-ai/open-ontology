# INC-43 notifier outage

## Impact

Timeouts at peak hours, about 8% of requests failed.

Reporter: 陆承宇

Severity: sev3

Service: notifier

Reported: 2026-02-16

## Root cause

Pool settings were reset by the last release.

## Fix

Rolled back and added an alert.
