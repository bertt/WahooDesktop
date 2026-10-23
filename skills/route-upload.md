---
name: wahoo-route-upload
description: Safely add a GPX or FIT route to a Wahoo ELEMNT using wahoo_cli add-route.
---

# Wahoo Route Upload Skill

Goal: put a route on the device (the only write action of the app).

## Workflow

1. Check the file: `.gpx` or `.fit`; for GPX at most ~10,000 track points.
2. Check the connection: `wahoo_cli status --json` (if not online, ask the user to connect/switch on the device; do not retry).
3. **Ask the user for confirmation** before uploading.
4. Upload: `wahoo_cli add-route <file>`
5. Verify: `wahoo_cli sync`, then `wahoo_cli --json routes`.

## Rules

- Never run `add-route` without explicit permission.
- Never sync in parallel with the desktop app.
- The device may need a moment or a restart to show the route.
- On errors, return the exact command and stderr.

## Example prompts

- "Put this GPX route on my Wahoo."
- "Is my route on the device yet?"
