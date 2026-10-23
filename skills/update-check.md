---
name: wahoo-update-check
description: Check firmware and map updates for a Wahoo ELEMNT using wahoo_cli (internet required).
---

# Wahoo Update Check Skill

Goal: determine whether firmware or maps are outdated.

## Workflow

1. Firmware: `wahoo_cli --json firmware`
2. Maps (pack level): `wahoo_cli --json map-updates`
3. Single tile: `wahoo_cli --json map-updates --tile <x>/<y>`
4. Pack overview: `wahoo_cli --json map-packs` (`--all` for everything)

## Rules

- These commands need internet; when offline, report it clearly and do not retry.
- Keep CDN requests to a few dozen at most.
- `map-download` is a large download: ask for confirmation first. It only writes locally, never to the device.
- Maps are never written to the device.

## Example prompts

- "Is there new firmware for my Wahoo?"
- "Are my maps up to date?"
