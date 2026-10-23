---
name: wahoo-device-diagnose
description: Diagnose USB/MTP connectie, cache en sync-problemen op een Wahoo ELEMNT met wahoo_cli.
---

# Wahoo Device Diagnose Skill

Doel: snel vaststellen waarom uitlezen/sync niet werkt.

## Werkwijze

1. Controleer verbinding:
   - `wahoo_cli status --json`
2. Als niet online:
   - Vraag gebruiker om datakabel te gebruiken en device aan te zetten (niet alleen laden).
   - Niet eindeloos retrien.
3. Als online en data verouderd/ontbreekt:
   - `wahoo_cli sync`
4. Lees daarna cache-gebaseerde status uit met `--json`:
   - `wahoo_cli --json info`
   - `wahoo_cli --json storage`
   - `wahoo_cli --json alerts`
   - `wahoo_cli --json debug` (alleen delen met toestemming i.v.m. privacy)

## Beslisregels

- Sync alleen uitvoeren als verse data nodig is.
- Geen parallelle sync met desktop app.
- Bij fouten: geef exact commando + stderr terug.

## Privacy

- Deel geen serienummers of volledige debug-output publiek zonder expliciete toestemming.

## Handige prompts

- "Controleer waarom mijn Wahoo niet zichtbaar is."
- "Doe een diagnose en geef alleen de noodzakelijke vervolgstap."