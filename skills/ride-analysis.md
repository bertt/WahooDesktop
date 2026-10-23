---
name: wahoo-ride-analysis
description: Analyze ride history and statistics of a Wahoo ELEMNT using wahoo_cli (cache, offline).
---

# Wahoo Ride Analysis Skill

Goal: summarize and compare rides from the local cache.

## Workflow

1. Make sure the cache exists; if not, run `wahoo_cli status --json` first, then `wahoo_cli sync` (only if the device is online).
2. Get recent rides:
   - `wahoo_cli --json history -n 20`
3. Ride details (id from `history`; quote ids containing spaces):
   - `wahoo_cli --json activity "<id>"`
4. Totals and records:
   - `wahoo_cli --json stats`

## Rules

- Always use `--json` and read from the cache (offline, fast).
- Only sync when fresh data is needed; never in parallel with the desktop app.
- Summarize: distance, time, elevation gain, average/max power and heart rate where available.

## Example prompts

- "Summarize my last 5 rides."
- "Compare my longest ride this month with the previous one."
