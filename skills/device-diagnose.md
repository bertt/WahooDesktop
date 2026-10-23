---
name: wahoo-device-diagnose
description: Diagnose USB/MTP connection, cache and sync problems on a Wahoo ELEMNT using wahoo_cli.
---

# Wahoo Device Diagnose Skill

Goal: quickly determine why reading/sync does not work.

## Workflow

1. Check the connection:
   - `wahoo_cli status --json`
2. If not online:
   - Ask the user to use a data cable and switch the device on (not just charging).
   - Do not retry endlessly.
3. If online and data is outdated/missing:
   - `wahoo_cli sync`
4. Then read the cache-based status with `--json`:
   - `wahoo_cli --json info`
   - `wahoo_cli --json storage`
   - `wahoo_cli --json alerts`
   - `wahoo_cli --json debug` (share only with permission, for privacy)

## Rules

- Only sync when fresh data is needed.
- Never sync in parallel with the desktop app.
- On errors, return the exact command and stderr.

## Privacy

- Do not share serial numbers or full debug output publicly without explicit permission.

## Example prompts

- "Check why my Wahoo is not visible."
- "Run a diagnosis and give only the necessary next step."
