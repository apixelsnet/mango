# 07 — Themes

Design direction "Tankōbon": manga-print look. Heavy ink lines, hard offset shadows, halftone dots, one spot colour.

## Tokens

Everything derives from three:

| Token | Role |
|---|---|
| `paper` | Background |
| `ink` | Text, borders, shadows |
| `spot` | Accent fills: primary button, badges, highlights |

Derived: `surface` (cards, inputs), `muted` (secondary text), `on-spot` (text on a spot fill).

## Presets

| Name | paper | ink | surface |
|---|---|---|---|
| Paper | `#F1F1EC` | `#111111` | `#FFFFFF` |
| Newsprint | `#E2E1DA` | `#1A1A18` | `#F2F1EA` |
| Sepia | `#EDE1C9` | `#2A2017` | `#F7EEDB` |
| Midnight | `#151513` | `#F1F1EC` | `#232320` |
| Ink blue | `#0F1A2B` | `#E9EEF7` | `#18263B` |
| OLED black | `#000000` | `#F5F5F0` | `#141414` |

Spot colours: Mango `#FFC400`, Tangerine `#FF8A1F`, Guava `#FF6B8B`, Mint `#6EE7B7`, Sky `#6CC4FF`, Lilac `#B9A2FF`.

## Rules

- Dark themes are paper and ink swapped. Nothing else changes.
- Hard shadows take the ink colour, so they stay visible on dark paper.
- `on-spot` is chosen automatically, dark or light, for contrast with the spot colour.
- Custom colours: the user picks paper, ink and spot. If ink on paper is below 7:1 contrast, warn and do not save silently.
- Borders 3px ink. No gradients. Square corners; pills only for chips and badges.
- Never signal state by colour alone.
- Fonts: Dela Gothic One (display), Zen Kaku Gothic New (body).

## MVP

- The six presets, the six spot colours, custom colours with the contrast check.
- Day, Night, or follow the system's light/dark setting, with one preset assigned to each. Day is the default, the first paint included; a reader who picked System or Night keeps it.
- Toggles: hard shadows, halftone dots.
- A theme is a small JSON object; import and export.

## Result (2026-10-10) — built

Code: `mango-reader/ui/src/lib/theme.svelte.ts`, `ui/src/app.css`, `routes/Settings.svelte`.

- The theme is one JSON object: `follow`, a day and a night palette (a preset name or `Custom`), `spot`, `custom` paper and ink, `shadows`, `dots`. It is kept on the server, so every device gets it, and mirrored in the browser so the first paint is already right.
- Custom colours below 7:1 are not saved on the first press: the screen says why and asks again. The same goes for an imported theme.
- A custom palette's `surface` is worked out from its paper and ink.
- Import and export are by copy and paste of the JSON.
- The reader keeps its own dark ground in every theme.
- Settings are split into groups (Appearance, Reader, Library, Downloads, Sources, Account, About), one on screen at a time. The group is in the address, `#/settings/<group>`, so it can be linked to and the back button works.

Checked in a browser: Paper, Midnight, Ink blue with another spot colour, and a custom dark palette that was still there after a reload. Not checked: every screen in each of the six presets, by eye.

## Done when

Every screen is readable in all six presets, and a custom theme survives a restart.
