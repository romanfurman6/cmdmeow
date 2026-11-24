# CmdMeow design system

The one source of truth for how CmdMeow looks, moves and sounds. Every UI thread, PR and canvas follows it.

- **Tokens live in code** (`src/kit/glass.rs`: `sp::*`, `ty::*`, `R_*`, colour fns, `tint::*`). Use them, never raw numbers or hex outside `glass.rs`.
- **System first, then code (G1, Roman 2026-11-15).** A component or rule changes here first (doc + Design System canvas, Roman's OK), then in the code everywhere it is used, in one PR. Never change a component on one screen only.
- **Reuse (G2).** Build screens from the `kit` components. A new component is added only when no existing one fits; it goes into this doc and `glass.rs` before any screen uses it. No local copies of a component.
- **One owner (G3).** Changes go through the design-system thread. Other threads propose a rule there; they don't edit this file.
- **Canvas:** https://claude.ai/artifact/QarEbeJ1pB33366Kc8LsY7
- **Status:** ✅ approved by Roman · ⏳ proposed, waiting on Roman. A ⏳ rule is not built until it turns ✅.

Contents: 1 Tokens · 2 Colour · 3 Type · 4 Spacing and layout · 5 Surfaces · 6 Buttons and controls · 7 Popups and modals · 8 Hover, selection, focus · 9 Motion · 10 Sound · 11 Screen patterns · 12 Writing text · 13 Themes (read before 2)

---

## 1. Tokens

| Group | Tokens | Where |
|---|---|---|
| Surfaces | `BG` `CARD` `CARD_EDGE` `RAISED` `POPOVER` `SIDEBAR` `FOOTER` | glass.rs |
| Text | `TEXT` `TEXT_SOFT` .84 · `TEXT_DIM` .68 · `TEXT_FAINT` .56 · `TEXT_MUTED` .40 · `ty::caption_color` | glass.rs |
| AI | `AI()` (cyan) | glass.rs |
| Action | `ACTION` (lime fill; was `BLUE`) · `DANGER` (destructive button fill) · `ON_ACTION` · `ACCENT` (lime as text) | glass.rs |
| State | `GREEN` `AMBER` `RED` · `CHIP_ASK` `CHIP_RISK` | glass.rs |
| Group tints | `tint::CLEANUP` `HEALTH` `CAPTURE` `KEEP_AWAKE` `NEUTRAL` | glass.rs |
| Neutral | `GRAY` (`OTHER` alias) · `series::monitor(i)`, `series::clean(i)` · `letter_tile()` · `white(a)` ink overlay · `HAIRLINE` 15 · `HAIRLINE_STRONG` 26 | glass.rs |
| Spacing | `sp::XS` 4 · `S` 8 · `M` 12 · `L` 16 · `XL` 24 · `XXL` 32 | glass.rs |
| Radius | `R_CARD` 12 · `R_BUTTON` 10 · `R_SMALL` 7 | glass.rs |

Every colour answers for the theme (TH2). Theme comes from Settings › Theme; `CMDMEOW_THEME=light|dark|miami` overrides it for screenshots. ✅

## 2. Colour

Palette: B3 Moss & Lime ✅ (2026-11-11). Canvas for C1–C14: https://claude.ai/artifact/V9GF4FWnmFs2vjzs4UvfeU C1–C14 ✅ 2026-11-15.

| # | Rule | Status |
|---|------|---|
| C1 | No raw colours outside `glass.rs`. Every colour has a value for each theme (TH2). | ✅ |
| C2 | The action colour (lime; pink in Dark Miami) is only for what you press or select. Never data, except as Clean's group tint (C3). In light mode lime is never text or a thin stroke: use `ACCENT` (`lift()`). Dark Miami text in the action colour uses `ACCENT` #FF9DC6 (`lift()`), never #FF5FA2. A selected row's text may be `ACCENT`, its number included (H3). | ✅ |
| C3 | A tool uses its group tint for its icon and its main data: Clean lime, Monitor cyan, Create apricot, Utilities lavender (Dark Miami: TH5). | ✅ |
| C4 | `GREEN` / `AMBER` / `RED` only mean state. Never a category or chart series. A spark or bar chart is one series colour; a tall bar is never `AMBER`. | ✅ |
| C5 | Chart series: group tint first, then its next steps, at most 3. The rest is Other in `GRAY`. Free / idle / available is the empty track. (`series::*`) | ✅ |
| C6 | The same thing has the same colour and words on every screen. The disk line under the Disk Map list says what the start ring says ("1.35 TB free of 2.00 TB", disk glyph, `tint::DISK_MAP`). | ✅ |
| C7 | A legend square only when its colour is on a chart on the same screen. | ✅ |
| C8 | Contrast in every theme: body text 4.5:1, captions and section titles 3:1, icons and chart marks 3:1. | ✅ |
| C9 | Neutral marks (You, Other, an app with no icon) use `GRAY`. No system greys. | ✅ |
| C10 | One neutral letter tile (`letter_tile()`) for apps with no icon. | ✅ |
| C11 | Links use `ACCENT`, hover included. No system blue. | ✅ |
| C12 | Temperature: cyan under 68 °C, amber from 68, red from 90. Text under 68 °C is plain. | ✅ |
| C13 | Exceptions only: capture/record HUD, the recording dot, traffic lights, third-party brand marks, user content. What the app draws over user content follows C16. | ✅ |
| C14 | Names say the role: `BLUE`→`ACTION`; `INDIGO`, `PURPLE`, `TEAL`, `CLAUDE` go. | ✅ |
| C15 | AI (AI Check chip, reveal ring, agent marks) has its own token `AI()`, cyan (today the same value as `tint::HEALTH`). | ✅ |
| C16 | **On-picture overlays.** Anything the app draws over the user's picture or video frame (selection frame, handles, crop and drag-out outlines, blur/eraser previews, the edge of a pinned screenshot) uses fixed colours, never theme tokens: white with a black edge (`overlay::LINE`, `overlay::RING`), the same in both themes, so it shows on light and dark pictures. A selected mark's frame and handles sit 5 pt outside it (`overlay::OUTSET`). Frame: 1 pt white over a 2 pt black 40% band. Corners: 6 pt white squares with a 1 pt black ring, 8 pt on hover or drag. Sides draw no handle but still grab. Arrows: two 6 pt white dots with a dark ring just beyond the tips, no frame. Dashed lines (crop area, drag-out): 1.5 pt white over 3 pt black 50%. Grab zone 24 pt, nearest handle wins (`overlay::GRAB`); a side under 64 pt has no middle grab spot (`overlay::MID_MIN`). | ✅ |

**Light Classic** (from the light audit, https://claude.ai/artifact/4oYGWVYxcUrvtpw7TfLAJQ) ✅

| # | Rule |
|---|------|
| L1 | Light `TEXT_MUTED` ink .40→.50, caption ink 128→153, to meet C8. |
| L2 | Light `CARD_EDGE` ink 20→31 so white cards don't merge into the sage backdrop. |
| L3 | Hairlines and dividers use `white(HAIRLINE*)` only; never hard-coded black or white alpha. |
| L4 | Switch off track: light ink 56 (dark 41). Inactive pager dots: light ink 72 (dark 46). |
| L5 | Surfaces that stay dark in both themes (dictation pill, display OSD) use `glass::hud` and fixed light content, never theme ink. |
| L6 | Disabled or locked: alpha .45 in dark, .62 in light. |
| L7 | A popover over a card keeps a visible edge in light mode. |
| L8 | Light `ACCENT` #3F6B0A→#2F5208 (7.6–9:1 on light surfaces), so lime text reads as clearly as ink. |

Light and series values (✅ 2026-11-15): Clean #7DB519→#5B8A14, Monitor #1F8AA0→#177A8E, Amber/Create #C06A12→#A65A0E, Green #4B8625→#437A20, light junk ramp #5B8A14 #507E14 #467318 #3E6A1A #36601B #2F571C #284F1E, dark ramp last step #46824B→#4F8E4E. Monitor series dark #6FD6E8 / #B4EAF3 / #4FA9BC, light #177A8E / #0B4652 / #3A97AB.

## 3. Type ✅

Hero 28/600, number 22/600, lead 15/600, title 13/600, section 12/600, body 13/400, label 12/400, caption 11/400 (`ty::*`). Bold 11 and 12 are allowed for chips and small values. Mono 11 for paths. Only exceptions: the speed dial (52) and the app mark (40). Hero and number use the semibold weight, like titles. Light mode uses heavier weights (body 510, semibold and hero 680).

## 4. Spacing and layout ✅

**Scale:** 4, 8, 12, 16, 24, 32 (`sp::XS`…`sp::XXL`). Nothing in between.

| # | Rule | Token |
|---|------|-------|
| R1 | Content starts 24 pt from the window edge, header included. | `sp::PAGE_EDGE` |
| R2 | Cards: padding 16, gap 16, radius 12. Title, 12, content. A section title sits 12 above its card. Titles inside data cards (Mac Health, AI Usage, Fan Control) are 15 pt; when there's no room beside the big number a title drops to 13, never cut (R10). On an onboarding step the subtitle sits 24 (`sp::XL`) above the step's cards. | `sp::L`, `sp::M`, `R_CARD` |
| R3 | Two row heights: settings rows 40 (label left, control right, hairline between), list rows 44 (icon, name, detail). Exception: the Disk Map folder list, rows 36 with no bar, a dense list beside the map. Widget sheets (SP12): every list row 32 (`SHEET_ROW_H`), switch rows 40. | `sp::SETTINGS_ROW_H`, `sp::ROW_H` |
| R4 | Rows with a bar: 36 tall, name and value on one line, a 4 pt bar 8 pt under them. A value with no limit (AI Usage Today) shows as a number with no bar. Page lists with a bar (Mac Health, AI Usage Models and Projects) use the widget line (W5): name · bar · value on one 22 pt line, shared columns; R4 stays for start lists (SP10). | `sp::BAR_ROW_H` |
| R5 | Controls are 28 inside cards and 32 in page headers. | `sp::CONTROL_H`, `sp::HEADER_CONTROL_H` |
| R6 | One empty state: centred, 56 icon chip, lead title, body up to 400 wide in dim, one secondary button. Inside a card: one caption-size line in the middle. | `sp::XXL` around it |
| R7 | Chips say a state (Elevated, Charging, In menu bar), never an action: 11/600 on a 16% tint. A chip shows only when the state needs a look or changes what the number means (Elevated, Charging, Held at 80% limit, 1 low); the everyday state (Normal, Nominal, Good) gets no chip. | `chip()` |
| R8 | Numbers in lists and tables are right-aligned. | |
| R9 | A title with a detail line under it keeps 6 between the lines; such rows are 52 tall. Columns in a row sit 16 apart. | `sp::LINE_GAP`, `two_lines_y()`, `sp::ROW2_H`, `sp::COL_GAP` |
| R10 | Text the app writes is never cut with "…": it wraps or the layout changes. Only names and paths people bring may be cut. Find cut text with `CMDMEOW_FIT_LOG=1`. A project from a path shows its folder name, plus the parent when the name is an id or appears twice; the path is never cut with "…". | `text_wrap()` |
| R11 | A list's section title is 12/600 in a 44 row: 16 above, 12 below to the first row; its link sits on the same line. Table heads sit in a 36 row with the hairline at its bottom. | `section_title()`, `sp::SECTION_H`, `sp::TABLE_HEAD_H` |
| R12 | Scroll bars float in a 14 gutter on the right, taken only when the content scrolls. Content in a scroll area takes `ui.available_width()`. | `sp::SCROLL_GUTTER` |
| R14 | A master switch is the first row of its card. While it is off, the rows that depend on it show disabled (L6) and keep their values. | |
| R15 | A page grid of cards fills the width but not the height: each card row grows to at most 340 pt and the grid stays at the top; the rest of a tall window stays empty below. Charts and rings never stretch past what the card needs. Also AI Usage (grid max 2×340+16). | `BENTO_MAX_H` (Mac Health) |
| R16 | A setting that both Capture pages have (Screenshot, Video) uses the same label and the same choice words on both: "Then" (Show preview · Open editor · Nothing), "Hide preview after", Copy to clipboard, Folder, File name, History keeps. | |
| R17 | **Permission row** in a tool's Settings (Screenshot, Video, Dictation): one shared kit row, 40 pt per permission: state dot, name, "Allowed" / "Not allowed", and a 28 Primary "Allow" when missing. Settings › Permissions keeps its own list. | ⏳ code |

Fixed chrome: sidebar 210 wide, sidebar rows 28, page header 50, page footer 60, sheet header 58, sheet buttons 32.

## 5. Surfaces

| # | Rule | Status |
|---|------|---|
| S1 | Backdrop: radial moss gradient (dark) / sage (light), 1 pt `HAIRLINE_STRONG` edge. Menu bar panel background is solid, no blur. The glow ellipse starts at the top centre and is never smaller than the default window's: rx = max(w, `APP_WIDTH` 1180) × 0.75, ry = max(h, `APP_HEIGHT` 740) × 1.05, so the 380 pt panel reads as the middle slice of the window's glow. Every window (app, setup, menu bar panel) shows it as a beam of light down the centre to the bottom, darker at the sides: rx = w × 0.85, ry = max(that ry, h × 1.9). No native glass (NSGlassEffectView) on the panel or its cards. Dark Miami: TH6. | ✅ |
| S2 | Card: `CARD` fill, 1 pt `CARD_EDGE`, radius 12, flat. No frame inside a frame. A card that opens details (P8) is clickable as a whole: "›" in `TEXT_FAINT` after the title, `white(6)` hover fill, pointer cursor. | ✅ |
| S2w | **Widget card** (`kit::tile` only: panel widgets, the widget editor and gallery; the panel update banner and the Uninstaller inline confirm also use it): the same surface as the quiet keys, e.g. the panel header's Open / Hide / Quit buttons (B4, drawn from `key::Look::quiet(0.06, 0.15)`): ink 6% fill, see-through so the S1 panel glow shows; 1 pt top highlight `TILE_HI` (white 9% dark / 95% light), 1 pt base under the card `TILE_BASE` (black 35% dark / ink 14% light); 1 pt `CARD_EDGE` inside (white 8% dark / ink 12% light); no sheen, no bottom shade. Radius `R_TILE` 10 (selected ring and Dictation live ring follow it). Hover is a light touch: 40% of the key's step (fill toward ink 15%, edge toward ink 18%). A single-action card presses like `kit::key` (34 ms, down 1 pt, base to 0, spring back); a card with buttons or rows inside doesn't move, its inner items press. Page cards keep S2. **Light theme (cards only):** white 90% fill (hover white 98%), 1 pt hairline ink 9%, no ink base and no top highlight, soft shadow `0 1 3` black 7% + `0 4 14` black 5%; quiet keys keep their own surface (B4). Dark unchanged. | ⏳ code |
| S3 | **R13** A tool's start screen (before its first run) has no cards: content is centred on the backdrop. Cards are only for pages with live data or lists (R2 applies there). Cardless: Cleanup, Disk Map, Uninstaller start. With cards: Mac Health, AI Usage, Fan, Startup, Cleanup results. | ✅ |
| S4 | A list is one card with full-bleed rows and hairlines between them. No card per item. | ✅ |
| S5 | Cards and page content have no shadow. Floating surfaces get only the "Whisper" shadow (P2). | ✅ |
| S6 | Group tile (tool icon): group colour 12% fill, 28% edge, glyph in group colour. Neutral tools: `white(18)`. | ✅ |
| S7 | **Sample card**: illustrative content (a demo, like the AI Check sample in onboarding) is a card with no fill and a dashed edge (`glass::dashed_rect`, `white(56)`, r12), with an "Example" tag beside its title (11/600 `TEXT_DIM` on `white(20)`, r6, 18 tall, 8 side padding). Elsewhere a dashed edge means a drop zone or an empty slot; never on real data. | ✅ |

## 6. Buttons and controls ✅

Approved by Roman for the base system on 2026-11-15. Canvas: https://claude.ai/artifact/ND3dqKVXA3VNCfycyAge34 · source spec: `button-press/button-rules.md` in the project files (not in the repo).

| # | Rule |
|---|------|
| B1 | **Kinds.** Hero (`hero_button`, 44, lime): one per empty or first-visit screen. Primary (`pill_button` PRIMARY, lime): one per view or dialog. Danger (`pill_button` DANGER): destructive confirm only. Secondary (`pill_button` SECONDARY): quiet. Side (`side_button`): quiet, with keycaps. Text (`ghost_button`, `link`): no fill. Icon (`icon_button`, square: 32 in headers, 28 in cards and rows), Round (`round_button`, 28, sheet close and refresh), AI chip (28, one look `aicheck::ui::ai_look` for the AI Check chip and one-shot asks such as `agent_button`). Refresh (↻) is a round button in a sheet header beside close, and an `icon_pill` "↻ Refresh" in a page header (SP16); never a footer button. Code still has 30 and 26; the code audit moves them. Panel header: two Round 28 buttons, S apart: Quit (✕), then Open (▢) at the corner. Quit asks in place, never as a system alert: ✕ grows left into a Danger pill "Quit" (0.18 s), a second click quits (ignored for the first 0.35 s). Esc, a click elsewhere, hiding the panel, or 3 s without the pointer on it fold it back. Hide is gone (a click outside hides the panel). |
| B2 | **Heights:** 44 hero, 32 (page header, footer, dialogs, side, toggle chips), 28 (inside cards and rows). Nothing else: 30, 34, 36 → 32; 24, 26 → 28. |
| B3 | **Shape:** `R_BUTTON` 10, capped at h/2. Chips h/2. Segmented `R_SMALL` 7, thumb 6, 4 pt gap between segments. |
| B4 | **Raised-key look, one language for every pressable, the same surface as widget cards (S2w):** fill, a 1 pt top highlight, a 1 pt base under the face. Lime/red: highlight white 25%, base = tint mixed 62% to black. Quiet kinds: `TILE_HI` highlight, `TILE_BASE` base, 1 pt `CARD_EDGE` hairline (ink 18% on hover). |
| B5 | **Glow** (`kit::key`) on lime and red keys 32 pt and taller only (hero 44, primary/danger 32; `GLOW_MIN_H`): deep under the key and narrower than it, `0 10 26 -14`, the tint lifted 20% toward white (no olive on dark), lime 55%, red 50%; smaller while pressed, none when disabled. 28 pt keys never glow. |
| B6 | **Danger colour** is `DANGER()`: #F04438 dark, #D92D20 light, white text. Destructive buttons only; `RED()` stays the state colour (C4). |
| B7 | **Press:** every key face drops 1 pt onto its 1 pt base (like a widget card, S2w), text drops 1 pt with no base. Down 50 ms ease-out, release 160 ms spring `cubic-bezier(.34,1.8,.64,1)`. Pressing never darkens the fill. Switch knob widens 4 pt while held; checkbox check pops .5→1. Content a caller draws on a pill moves with the face (the helper returns the offset). |
| B8 | **States.** Hover: colour only, no movement (lime and lime-on fills mix 26% to white, red 22%; quiet fills ink 10–15% with full-ink text, secondary hairline 24%; ghost text gets an ink 7% backdrop; links lighten and underline; AI chip: idle no fill, AI edge 35% on a 1.5 pt AI base; hover and press 12% AI fill, edge 60%; Checking shows the spinner, "Checking…" and "Stop"; with tips 12% fill and a 45% edge, 18% on hover or while open; failed: amber edge; the agent's mark leads every state but Checking, never a sparkle (T12), and after a check the label is "N tips" or "All good"; unchecked check/radio edge 70% + 8% fill). Focus (keyboard only): 1 pt ring 2 pt outside, following the radius, in the button colour (lime, danger red, or ink). Disabled: alpha from one shared helper (L6), no hover, press or sound; the base stays. Loading: push buttons only, a 13 pt Core Animation spinner before the label, width locked, no press or sound. On/selected: segmented thumb; switch lime, knob right; check and radio lime; toggle and filter chips, recording shortcut field: lime tint + `ACCENT` edge. |
| B9 | **Choices:** segmented 32 header / 28 card, switch 30×18, checkbox and radio 16, filter chips 28 through one shared helper, tool toggle 28, shortcut recorder 28, action tile 40–44 (icon over label). |
| B10 | Rows, sidebar items and cards highlight only: no press, no sound. Exception: actions and rows inside a widget (SP13 W12). |
| B11 | Only `kit` helpers draw buttons: no plain `egui::Button`, no hand-drawn capsules. Every helper plays its own click; callers never do. |

Consts: `KEY_BASE` 3, `KEY_DROP` 2, `KEY_BASE_QUIET` 2, `KEY_DROP_QUIET` 1, `PRESS_IN` .05, `PRESS_OUT` .16.

## 7. Popups and modals ✅

Approved 2026-11-15 (PR #209, shadow "1 Whisper").

| # | Rule | Status |
|---|------|---|
| P1 | **Surface:** `POPOVER` fill (egui menus `BG`), 1 pt inside edge `POPOVER_EDGE` (dark `white(30)`, light ink ~20%). Radius: details popup 16 (P8), dialogs and sheets 14, popovers and bars 12, hover tip and menus 10, onboarding hint 8, toasts pill. | ✅ |
| P2 | **Floating shadow "Whisper"**, one token `float_shadow()`: two black layers, key (0,1) blur 2 at .30 dark / .06 light, ambient (0,4) blur 10 at .20 dark / .06 light. egui menus and tooltips: `float_shadow_egui()` one layer (0,3) blur 8 at .32 / .08. Only floating things get it (menus, tooltips, popovers, dialogs, sheets, toasts, floating bars, drag tags); cards, rows and page content never do. Native window shadows and the screenshot "Shadow" option stay. | ✅ |
| P3 | **Modal cover:** while any menu, popover or dialog is open, nothing under it hovers, shows a tip or takes a click (`kit::modal::shield`, `layer`, `pointer(ui)`). A click on the cover closes a menu or popover. | ✅ |
| P4 | **Dim** under dialogs, sheets and the loader: one `SCRIM()` over the page area, black 45% dark, ink 24% light. A click on the dim closes a sheet. Esc closes every dialog and sheet, except while work runs (loader). Destructive confirms never close from a click on the dim. | ✅ |
| P5 | Every removal that can take longer than 0.4 s shows the removal loader (`kit::loader::show`): card 340 wide, padding 24, mark 88, title (lead, verb + "…"), "x of y" (body dim), current path (mono 11 faint, cut in the middle), 28 buttons (Stop where safe; Done, Put back where undo exists). It appears after 0.4 s and stays at least 0.8 s. Scans and measuring keep `spinner()` and the bar. | ✅ |
| P6 | The loader mark is one shape with the ⌘ cut out; Done turns ⌘ into ✓. | ✅ |
| P7 | **Tip placement:** a hover tip opens on the side away from the edge its bar is docked to, 8 pt from the bar, and never covers another control. Tips of a top bar with a sub bar open under the sub bar (`glass::tip_below`). A row's tip in a rail or sidebar opens beside it, 8 pt out, centred on the row (`glass::tip_beside`), never at the pointer over the next row. | ✅ |
| P8 | **Details popup** (`kit::modal::details`; Mac Health, AI Usage): a card's details open over the page, never in a side sheet. 440 wide, centred, `SCRIM()` under it (P4), `POPOVER` fill, radius 16, height fits the content. Closes with ×, Esc or a click outside. Content sits straight on the popup on its 16 pt text column, groups split by a hairline; no cards or boxes inside (S2, no frame in a frame). | ✅ |
| P9 | **Permission card** (Screen Recording, Keychain): before macOS asks, an own floating window centred on screen at 30% height explains it. `POPOVER` fill, radius 14, `POPOVER_EDGE`, 16 inset. Tool chip 42 + title 15 bold + sub 13 dim; body 13; a hairline-split list of what is allowed (✓ `GREEN`) and what isn't (✕ faint); footer hint 12 faint on the left (amber "Denied" when refused), Secondary + Primary 32 on the right. Enter = primary, Esc = Not now. Hidden while the system dialog is up. | ✅ |
| MN1 | **Menus** ("⋯" and right-click) come only from `kit::menu` (`popup`, `context`, `item`, `choice`, `sub`, `note`, `separator`, `danger`); never `ui.button` in a menu. Frame: radius 10, 1 pt edge, Whisper shadow (P1, P2), 4 above and below the rows. | ✅ (canvas https://claude.ai/artifact/4C5Z47zFtr7PiiQD8qPtyv) |
| MN2 | Menu rows 28, body 13, text 12 in from the edge. A note (state, command) is caption 11 dim, wraps at 320, mono for commands. Hairline separators edge to edge with 4 above and below. | ✅ |
| MN3 | Menu rows hover with `ROW_HOVER` edge to edge (H2); no press, no sound (B10). | ✅ |
| MN4 | A picked choice is lime 16% with `ACCENT` text and a check (H3). Never the system blue. | ✅ |
| MN5 | A submenu row shows a dim chevron; the submenu opens 4 to the right of its menu, never over its rows. | ✅ |
| MN6 | A "⋯" menu opens 4 under its button, right edges lined up (above when there's no room), so it never covers the row it came from. Destructive rows use `DANGER` text (B6). In a list, a row's "⋯" shows while the row is hovered or its menu is open. | ✅ |

## 8. Hover, selection, focus

| # | Rule | Status |
|---|------|---|
| H1 | One hover fill for rows everywhere (lists, rails, popovers, sidebar, start lists): `ROW_HOVER` = `white(10)`. Edge to edge in lists and popovers (H2); inset radius 8 only in the sidebar and rail. In inset lists (sidebar, group rail, Clipboard) two lit rows never touch: lit fills sit 1 pt in from the row (`glass::LIT_INSET`) or rows step 2 apart, a 2 pt gap. Edge-to-edge lists (H2) have no gap. Inside a widget nothing hovers but the whole card (SP13 W12); inner clickables show the pointer cursor. | ✅ |
| H2 | **Lit rows in a card list** (hover, selected, AI Check ring, a state wash such as Startup's red) sit 4 pt in from the card's sides and 1 pt from the next row, radius 8 (`glass::lit_rect`, `LIT_R`), so they never meet the card's rounded corners. Content stays 12 pt inside (H9), 16 pt from the card. No divider touches a lit row, above or below (`glass::row_divider`). Popovers and menus keep edge-to-edge rows, the last row takes the popover's corners; start lists on the backdrop (SP1) stay edge to edge. | ✅ (AI Check, hover A) |
| H3 | Selected row: lime at 16% (`selected_fill`), text `ACCENT`. Sidebar and group rail: inset, radius 8 (H1). Content lists in cards such as the Uninstaller table: inset like hover (H2). | ✅ |
| H6 | Hierarchy by indent: a child row's icon starts under its parent's title text (sidebar items under the section title, bundle children 28 in). Icons in one list share one fixed box (16 in the sidebar), so the label never shifts with the glyph. | ✅ |
| H4 | Selected card: 1.5 pt lime ring inside. | ✅ |
| H5 | Reveal (`reveal::RING_W` 2, `FILL` .14, `HOLD` 1.8 s, `FADE` 1.4 s): a click on an AI suggestion closes the popover, scrolls to the item and rings it 2 pt cyan with a 12–14% cyan fill; it holds ~1.8 s and fades by ~3.2 s. Only items visible on the page are clickable. | ✅ (canvas https://claude.ai/artifact/WHbrjAR7nMt8RKFkuivn5y) |
| H7 | A picked or selected item has no hover, because a click on it does nothing: sidebar, group rail, picked segment, picked radio, picked card (H4), single-choice tiles. Items where a click changes state (checkbox, toggle chip, a widget card with its sheet open) keep their hover. | ✅ (PR #242) |
| H8 | A row's background shows only state: hover (H1) or selected (H3). Size or share never fills a row; show it as a number (R8), an R4 bar under the name, or on a chart or map. | ✅ (PR #242) |
| H9 | **Content never touches a lit fill.** The first and last element of a row (checkbox, icon, chip, value) sit at least 12 pt (`sp::M`) inside the hover or selected fill. Sidebar rows (28 pt) keep 8. | ⏳ code |

## 9. Motion

| # | Rule | Status |
|---|------|---|
| M1 | Decorative and looping animation runs on Core Animation layers (`kit::spin`). egui never repaints in a loop for it; it redraws on state change only. | ✅ |
| M2 | Every animation is finite or tied to visible work, and stops repainting when done. | ✅ |
| M3 | Press: down 50 ms ease-out, release 160 ms spring `cubic-bezier(.34,1.8,.64,1)` (B7). Hover fades via `animate_bool`. | ✅ |
| M4 | Loader: fill eases 0.6 s, wave loops 1.6 s linear, ⌘→✓ crossfade 0.25 s plus one 0.46 s hop. | ✅ |
| M5 | **Window open:** if the window's first frame isn't drawn 150 ms after it appears, show the S1 backdrop and the P6 loader mark (56 pt, centred, lime fill rising on a loop). Fades in over 120 ms; the first frame covers it and it stops (M2). No text, card or frame. Core Animation under egui's frame (M1). | ⏳ code |

## 10. Sound ✅

Approved for the base on 2026-11-15; fine-tuning later (files are swappable, levels live in code). One tonal family for clicks and cues.

| # | Rule |
|---|------|
| N1 | **Click:** plastic "higher" (~57 ms, 400 Hz high-pass), three takes, compiled in from `assets/sounds/click-1..3.wav`. Played on mouse down, always with the visual press, never alone. |
| N2 | **Three levels, not one per kind:** Emphasized (hero, primary, danger) 0 dB; Standard (secondary, side, icon, round, tool) −3 dB; Subtle (chips, segment, filter, switch, shortcut, check, radio, text) −6 dB; none for rows, sidebar, cards, hover. Relative to `CLICK_MASTER`, tuned by ear on the M1 (target ≈ −6 dB under cues; starts at 0 dB). |
| N3 | **Pitch per kind in whole intervals:** hero −2, primary 0, danger −3 semitones; standard +2/+3; subtle +2…+5. |
| N4 | **Variation:** ±3% pitch, ±6% level, never the same take twice, at most one click per 100 ms. |
| N5 | **Silent when:** disabled or busy, Settings › Sounds off (`sounds_on`), macOS "Play user interface sound effects" off, recording, screenshot mode, launch, any click the user didn't make. |
| N6 | **Playback:** `kit::sound::click(Click)` through AudioToolbox system sounds (low latency), never afplay. Files: WAV mono 44.1 kHz 16-bit, peak about −10 dBFS (levels are set in code), owned (Magnific / synthesized). |
| N7 | **Cues** in A major (A4 C#5 E5 A5), each opening with a soft plastic tick at −12 dB: Ready C#5→E5→A5 400 ms; Done A4 C#5 E5 A5 arpeggio 600 ms; Oops A4→G#4→E4 450 ms (not an alarm); Awake / Asleep E5→A5 / A5→E5 350 ms at −3 dB. |

## 11. Screen patterns

| # | Pattern | Status |
|---|------|---|
| SP1 | **Start screen** (before the first run): no cards (S3). Left: the disk ring or tool icon, the action button 32 below it. Right of a 1 pt hairline divider with a 48 gap: the list straight on the backdrop. Every start screen has this list: Cleanup its groups, Uninstaller what a scan sorts apps into (counts the listing knows), Disk Map the places to map instead of the disk (a click maps one). List rows hover edge to edge with no corners (H2); icon, name and value sit 12 pt in from the row edges (`glass::START_PAD`), the section title stays at the row edge. Group tips show beside the list (P7). | ✅ |
| SP2 | **Disk ring** (`kit`, one function, Cleanup and Disk Map start): 208 across, 12 stroke, track `white(16)`, used arc in the tool tint with a round cap from 12 o'clock clockwise. Centre: free space in hero 28/600 over "free of <total>" in label 12 `TEXT_DIM`. 12 below the ring: a 16 disk icon and the volume name in body `TEXT_SOFT`. Disk Map's own ring (r 64, "X of Y used") goes. The same `stat_ring` draws AI Usage limits, with a faint solid pace arc under the used arc (`glass::stat_ring_paced`); no caption in the ring when the card title names the number (T14). | ✅ |
| SP10 | **Start list rows on the backdrop** (Cleanup groups): a 44 head line with "Last scan 2 days ago" as a 12/600 section title and the total right; rows 44 with a 28 tinted icon circle and the name, hairlines between, `ROW_HOVER` on hover. A group's description shows on hover only. After a scan each row gets a 4 pt bar and a right-aligned size. | ✅ |
| SP11 | **Fan glyph:** one shape for every fan, turning or still (`kit::spin`): four rounded blades with the hub cut out, so the surface under it shows through (Magnific/Flaticon Premium icon 17711600). Tint `tint::FAN` (C3). It turns with the fan, one turn per 2400/rpm s, on Core Animation (M1). Fan page card: radius 38 on a 44 disc; panel row 12; widgets as before. With no fans it stands still in `white(40)`. `Icon::Fan` (sidebar, headers, tiles) is the same glyph, still, filled; it is the one tool icon that is not an SF Symbol. | ✅ |
| SP12 | **Widget sheets retired** (2026-11-23). A panel widget opens its tool's page; on Mac Health it opens that card's details popup (P8). Settings that lived in a sheet (charge limit and its switches) move to the page's Settings tab, in a section of their own. | ✅ |
| SP14 | **Header tips:** a tip's title is a short fact ("Memory pressure high", "Trash holds 3.2 GB"), no hover sentence (T1). Its button opens the card's details (P8) or the fix; a tip names what to do when there is a fix: "Restart recommended · up N days" with "Restart…" (macOS asks to confirm), "Trash is full · X" with "Empty Trash". Status chips name the service: "Claude has problems", "Claude is down" (not "Degraded", "Outage"). | ✅ |
| SP15 | **Chart hover readout** (`parts::day_chart`): above the bars, a row of stats, number (`ty::lead` 15/600, `TEXT`) over caption (`ty::caption`, `TEXT_DIM`), 24 apart (`sp::XL`); never one line of mixed sizes. The first caption names the day ("Today", "Mon Nov 3"). | ✅ |
| SP16 | **Page header controls.** From the right edge: [page action] 12 [tabs] 12 [search or other]. The page action shows on every tab and never hides; it is as wide as its widest label ("Pause 15 min" = "Resume", "Record screen" = "Stop · 0:12"), dimmed (not hidden) when it can't run, 32 tall (B2). Primary only for the page's main action (Capture area, Record screen, the Mac Health fix); the rest are Secondary. A refresh in a page header is a Secondary with a glyph and a label (`icon_pill`: "↻ Refresh", "↻ Rescan"); sheets keep the round ↻. Filters keep their slots (Startup always shows "Flagged 0"). Only exception: the Mac Health tip button exists only while the top tip has a fix, at the right edge. No status tags in headers that say nothing to do (no Startup "changed", Fan Control "Thermal pressure", Disk Map "Partial map"). | ⏳ code |
| SP3 | **Group rail** (results): a card 232 wide left of the content, 16 gap, 4 padding, rows 40 with a 24 group chip, name body 13, size right in dim; selection H3. A group shows its item count as a badge (T19), never a colour dot. The footer primary acts on what the rail shows: on a group it reads "Clean <group>" and removes only that group's ticked items; on All it reads "Clean <size>". After a one-group clean, the finished screen's primary is "Back to results" while anything is left. | ✅ |
| SP4 | **Bundles**: 3+ alike items fold into one row with an arrow and "N items"; children are 44 rows indented 28. Kept items fold into one "Kept" row. | ✅ |
| SP5 | **Empty state**: R6. | ✅ |
| SP6 | **Locked tool**: a permission screen per tool that names what is missing in the words of the fix (T16), not a dimmed page. | ✅ |
| SP7 | **Big state + control** (Keep Awake): centred scene, title hero 28, detail body dim in a fixed 18 line (kept when empty), then one segmented control 32 tall (Off · Auto · On). The control is the only switch, so state and control never disagree (T17). | ✅ |
| SP8 | **Hourly chart**: 24 columns, gap 4, 72 tall, stacked series in lavender steps (On = `tint::KEEP_AWAKE`, Auto = `series::util(1)`; never `GREEN`, C4) (top corners radius 3), dashed `white(26)` guides with 11 faint labels in a 36 right gutter, 00/06/12/18 under it. Hover a column → tooltip plus 1.5 outline, linked to the list below. Legend sits on the section title row (10 swatch, label 12 dim, value bold 12). | ✅ |
| SP9 | **Keep Awake widgets**: 1:1 (scene + one word) and 1:2 only. 1:2 is a Wide like the others (W3): the scene ring 50 on the left, then "Keep Awake" ("Sleeps in" while winding down), the state ("Awake", "Can sleep", the time left), and the mode with its detail ("Auto · Claude, Codex", "On · Since 13:07"). In both sizes a click turns On on or off; Off · Auto · On is picked on the page (SP7). All widgets also follow SP13. | ✅ |

### SP13 Widgets (W1–W12) ⏳ code

Approved by Roman 2026-11-17 (widget review canvas https://claude.ai/artifact/Gdvrk5REUpSfDMyhK9awnK). The card itself is S2w.

| # | Rule |
|---|------|
| W1 | Card. Widget card (S2w): the panel header buttons' surface (`key::Look::quiet`), ink 6% fill, 1 pt CARD_EDGE inside, see-through; one S1 panel glow, a beam down the centre, shows through all cards. Radius R_TILE 10, padding 12. TILE_HI 1 pt top highlight (white 9% dark / 95% light), TILE_BASE 1 pt base (black 35% dark / ink 14% light); 1 pt CARD_EDGE inside (white 8% dark / ink 12% light), no sheen or shade. Hover 40% of the key's step (fill toward ink 15%, edge toward ink 18%). Native Liquid Glass was tried and dropped (2026-11-18). A single-action card presses: down 1 pt, base to 0, spring back (key::PRESS_IN/OUT). Cards with inner buttons or rows don't move. Nothing inside has its own fill or border except buttons. | **Light theme (cards only):** white 90% fill (hover white 98%), 1 pt hairline ink 9%, no ink base and no top highlight, soft shadow `0 1 3` black 7% + `0 4 14` black 5%; quiet keys keep their own surface (B4). Dark unchanged.
| W2 | One visual. Metric = ring: 44 in 1:1, 50 in 1:2 and 1:4; track 10%, arc in group or state colour. Other widgets = group-coloured icon without a plate: 26 in a 40 box in 1:2 and 1:4 (text starts at 64 px), 20 in 1:1. |
| W3 | Text. Label 12 TEXT_DIM / value 15·600 / caption 11; always three lines in 1:2 and 1:4. Foreign text in the value slot (clip) 13·400 with fade. 2:4 main number hero 28·600, unit attached: "17%", "2 225 rpm". |
| W4 | Small 1:1. Metric: centred ring 44 + value 13·600. The value may drop its word when ring + hover tooltip explain it (Storage "1.44 TB", tooltip "free of 2 TB"; T14 exception). Action: group-coloured icon 20 top-left, bottom name 12·600, 3 pt, shortcut 11. Padding 12 top/bottom, 8–10 sides. |
| W5 | Row 1:4. The divider (HAIRLINE_STRONG) sits on one shared line in every Row, `ROW_SPLIT` = 12 + 50 + 12 + 86 + 12 = 172 pt from the left edge, and moves right only when the value is wider (the value is never cut). The left part is the same in every Row, Clipboard and Dictation included (ring 50 or glyph 26, label, value, caption). Right part: up to three lines, 12/400: name, bar, value. **Bars sit inline, the same in every widget size**: three shared columns, names at the widest name's width, values right-aligned at the widest value's width, and each bar starts at name column + 10 and ends at value column − 10. In 1:4 the columns are shared by every Row (one grid down the panel): the right part is 160 pt: name column `ROW_NAME` 70 (the same in every Row, so the grid holds when widgets change; fits "Applications", "Compressed" on SF), 10, the bar (at least 24 pt), 10, values right-aligned in `ROW_VALUE` 44 (wider only for a wider value). In 1:4 a device line shows its type (Mouse, Trackpad, Keyboard, AirPods), not its name; the full name stays in Medium, Large and the sheet. In Medium (2:4) and Large (4:4) lists (AI Usage limits, Fans temperatures, Mac Health lines) they are shared within the card (`tile::bar_cols`). Values are never cut. A line without a bar keeps its name at full width. Labels and captions that can be a name (device names) fade at the edge and never end in "…"; values keep their full width. Device lists end every bar on one x, the "100%" column, so they line up in Medium and Large. Instead of lines, the right part may show a graph with a labelled time. A line whose data has no limit shows a number without a bar (R4 exception). A name that doesn't fit fades and is never cut with "…". Device names drop the owner ("Roman's AirPods Pro" → "AirPods Pro"). Actions: no border or fill, equal shares with 4 inset, centred content, one HAIRLINE_STRONG divider at 88% height between them. A line may carry two values (AI Usage Models/Projects: tokens, then $). Each value has its own shared right-aligned column, 12 apart; the secondary value `TEXT_FAINT`, the main value `TEXT`. The unit shows once, never repeated per line ("81.3M  $1900", not "81.3M tok · $1900"). `tile::bar_cols_pair`, `tile::bar_line_pair`. Time captions under a history graph (`tile::spark_span`, 11 pt caption) sit `sp::S` (8) under the bars, the same as the Fan Control chart; a stats row under the captions is a block of its own, `sp::L` (16) below (`tile::SPAN_CAPTION_H`). |
| W6 | Medium 2:4, Large 4:4. Header 13·600, right side caption 11 or ACCENT link 12, no chips. Rows fill the height, hairlines between. A size with nothing to fill it is removed from the size list. |
| W7 | Buttons. 28 pt, quiet, from kit (glass::segmented, switch). Selected segment = raised thumb. No lime glowing buttons in the panel. |
| W8 | Words. A value says what it is: free, items, connected. One unit form: KB/s, 17%. Names as in the tool: AI Usage. App text never truncates with "…"; foreign text (clips, dictation) fades. Demo data follows R-examples (everyday, no code or personal URLs). |
| W9 | Loading. Static skeleton in the widget's shape with its icon. No 33 ms repaint (R-anim). |
| W10 | No permission. Only the glyph is dimmed; the widget name, what it needs (value line, e.g. "Bluetooth") and the caption "Needs access" stay readable. No lock badge, no "Locked". Tooltip "Needs Bluetooth" (T16). Click asks for the permission. |
| W11 | Empty. Header and buttons stay. Instead of data one centred line 12 TEXT_FAINT with icon: "No screenshots yet". No dashed frame. Action as ACCENT link: Resume, Connect. |
| W12 | Hover. Only the whole card hovers (CARD → RAISED 50%). Nothing inside a widget highlights: no list rows, actions, segments or links. Every clickable shows the pointer cursor; bars, graphs and captions don't. Inner press feedback: the pressed action or row dims icon and label to 50% and drops 1 pt while held, no fill (B7/B10 exception for widgets). Pages and menus keep H1, H2, B8. |

## 12. Writing text ✅

Approved by Roman on 2026-11-14, after the text audit of all 101 screen states. A Mac utility explains itself through its layout: names, values and controls. Text that a person has to read before using the screen is a bug. Every user-facing string is also checked with the Unslop audit.

### Cut it

| # | Rule | Instead of | Write |
|---|------|------------|-------|
| T1 | No reassuring caption under a status. The status or chip already says it. | "Normal" + "Within the normal range." | "Normal" |
| T2 | One fact appears once per screen. Pick the place where it is read, drop the rest. | "80% limit" in the header, the ring and a row | the row only |
| T3 | Never explain the interface: how to click, where a setting lives, what a list contains, what to do later. | "Click a widget to resize it", "Each one says which tool uses it", "Come back and press Check again" | nothing |
| T4 | A row gets a reason only when the reason changes the decision (Needs your OK, Will quit Zoom). Safe rows need none. | "Rebuilt automatically" on every Safe row | nothing |
| T5 | A setting or tool description is at most 5 words, says what it does, and has no period. New users need it, so keep it, but never as a sentence. | "Keeps your Mac awake while an agent works on a task." | "Keep Mac awake" |
| T6 | macOS notes (system shortcuts, "macOS asks once") show only when they apply, for example on a real shortcut conflict. | a footnote that is always visible | a note on conflict only |
| T7 | Empty states: a title and one action. No paragraph about what will appear here. | "Plug in a display to adjust its brightness and…" | "No external displays" |

### No AI tone

| # | Rule | Instead of | Write |
|---|------|------------|-------|
| T8 | No taglines or slogans inside the app. | "14 tools. One menu bar." | nothing |
| T9 | States are plain words or numbers, not moods or metaphors. | "Room to spare", "Ahead of pace", "Sailing", "Mostly lean" | "42%", "Charge again below 75%" |
| T10 | Don't give the app or the user a voice: no "its call", "sleep in", "nudge", "4 can go". | "unsaved work stays its call" | "Zoom will quit first" |
| T11 | No summary sentence above a list that the list already shows. | "Two big folders you can drop" above the two folders | the list |
| T12 | No sparkle or magic glyphs for AI. AI is a feature with a name, not a mood. | ✦ AI Check | AI Check |
| T13 | No demo text that sounds like an ad. Demo data looks like real use. | "Okay, this is my first dictation and it already works." | an ordinary sentence |

### Make it clear without adding text

When something is unclear, name it with one or two words. Don't explain it in a sentence: that turns an "unclear" bug into a T1–T13 bug. If the name doesn't fit, use a hover tooltip.

| # | Rule | Instead of | Write |
|---|------|------------|-------|
| T14 | Every number and time says what it is, with a word or a symbol, never a bare value. Rates are KB/s, MB/s, never /sec. In a 1:1 widget the ring and its hover tooltip may name the value ("1.44 TB", tip "free of 2 TB"). A big number may be plain ("16%") when the card title names it ("CPU load"). A limit's reset is 24 h everywhere (page, sheet, widget, tips): "Resets 20:29", "Resets Sun 08:52"; a limit card shows "Runs out / Fri 19:10" in red over "Resets / Sun 08:52"; a session with no reset yet shows "Not started". | "17:04", "4 / 8", "1/2" | "Updated 17:04", "↻ 19:53", or drop it |
| T15 | One column, one kind of data. Don't mix counts, shortcuts and states in the same place. | sidebar right column: "11", "⇧⌘2", "Set up" | shortcuts only |
| T16 | A locked or disabled thing says what is missing, in the words of the fix. | lock icon; "Full Disk Access · Finding files to clean up" | "Needs Full Disk Access" |
| T17 | A big state and its control never disagree. If something else holds the state, say who. | "Awake" with the switch off | "Awake · by Claude" |
| T18 | A setting's name says what it does, not a nickname or an internal step. | "Put the clipboard back", "Step 1 of 2 · Looking for junk" | "Restore clipboard after typing", "Scanning…" |
| T19 | No rows of small unlabeled icons. Use words, or drop them. | tool icons next to each permission | "Used by Cleanup, Disk Map" on hover |
| T20 | A control with no label gets one, or goes. | a radio button next to a switch | a column head ("Main") |
| T21 | Shortcuts are written the macOS way: ⌃⌥⇧⌘, then the key, with no spaces (⇧⌘2, ⌥Space). A mouse button reads Mouse N (3 middle, 4 and 5 the side buttons), with modifiers before it (⌥Mouse 4). Never Back, Forward or Middle: mice map their buttons differently. On keycaps there is one cap per modifier, and Mouse N is one cap. | ⌥ Back, Mouse4, M4 | ⌥Mouse 4 |

## 13. Themes ✅

Read this before §2: every colour rule there holds in each theme.

| # | Rule |
|---|------|
| TH1 | Three themes: **Light Classic**, **Dark Classic**, **Dark Miami**. One design system; themes change colours only. Spacing, type, radii, motion, sound and components stay the same in all three. |
| TH2 | Every colour token answers for the current theme: `pick3(dark, light, miami)`. `pick(dark, light)` stays only for neutrals that follow the ink (Dark Miami uses the dark value). No `is_light()` branches outside `glass.rs`/`key.rs`/`tile.rs`. |
| TH3 | Base: Light Classic is the light base (dark ink). Dark Classic and Dark Miami are the dark base (white ink, dark-mode weights, dark L-values). |
| TH4 | **Settings › General › Theme**: its own card on top, one row, segmented control `Auto · Light Classic · Dark Classic · Dark Miami`. Auto follows macOS light/dark (Light Classic / Dark Classic). Default Auto. A switch applies at once, no restart. Saved per user. |
| TH5 | Dark Miami palette: action and Clean tint hot pink #FF5FA2 (`ACCENT` #FF9DC6), Monitor and AI aqua #4FE3E0, Create burnt orange #FF9A4D, Utilities lavender #C3B0FF; `GREEN` #7EEBA6, `AMBER` #FFC06B, `RED` #FF6B5E, `GRAY` #A49CB8; junk ramp pink→violet #FF5FA2 … #7F63D0; Monitor series #4FE3E0 / #AEF4F2 / #2BB0AE. |
| TH6 | Dark Miami backdrop (S1): a dusk sky top to bottom, not the radial glow: cobalt #0B1240 → violet #2A1452 → magenta #5E1D57 → burnt-orange horizon #70283C. It replaces the whole glow beam (no rx/ry). Every window (app window, menu bar panel, setup, window-open cover) shows the full sky over its own height. The horizon keeps `ACCENT` text at 5.3:1 and pink/red marks at 3.6:1 (C8). |
| TH7 | Nothing from the game it is inspired by: no names, logos, fonts or art. The name is "Dark Miami". |
| TH8 | Every new screen and PR is checked in all three themes (C8 contrast, M1 test). Screenshots: `CMDMEOW_THEME=light|dark|miami`. |
| TH9 | Widgets follow the theme: the panel widget cards, the widget editor and the gallery draw with the same tokens, so Dark Miami changes them too. Fixed-colour surfaces stay as they are (C13, C16, L5). |
| TH10 | The desktop cat follows the theme. Its lime fur, stripes and shading take the theme's `ACTION()` (Dark Classic #C8F05A as drawn, Light Classic #A8D83A, Dark Miami #FF5FA2), with lightness kept relative to the pack's lime. Outline: Dark Miami ink #1A0B2E; Light and Dark keep the drawn #0A1009. Eyes, nose, mouth, tongue, bed, bowl and yarn keep their colours. Recoloured once per theme switch (no idle cost). The menu bar cat stays a template icon; the app icon stays lime. |
| TH11 | **Onboarding step "Pick a theme"**: right after Welcome, before Goals (one step dot). Title "Pick a theme" (bold 22). Three cards in one row, `sp::M` gap, 2:3, r12, filled by the theme's picture (cover), name top left (bold 17, dark ink on Light Classic, white on the dark two), check top right. The picked card gets the 1.5 action ring (H4), hover lifts 1 pt, and the cards fade in one by one like the Goal cards. The default pick is the theme drawn now (Auto shows as Light or Dark Classic). A click sets Settings › Theme. |
| TH12 | **Theme switch animation ("ripple through blur")**: the old theme is laid over the new one as a picture and blurs to 10 pt (0.48 s ease-out), then a soft round wave from the clicked card (feather 20% of its reach, 0.8 s ease-in-out) cuts it away. Core Animation only (R-anim), no egui frames. No animation with Reduce motion or in screenshots. Onboarding only for now; Settings switches at once. |
| TH13 | **Theme pictures**: one flat series (same cat, size and place, empty top third). Light Classic: day rooftops with dark roofs. Dark Classic: night rooftops. Dark Miami: dusk beach. The cat is in the theme's action colour with a soft glow. Files `assets/themes/{light,dark,miami}.jpg`, 1024×1536. |

Other rules in Dark Miami: B5 glow tint stays `ACTION()`, so Dark Miami keys glow pink. B6 `DANGER()`, P2 shadows and P4 `SCRIM()` use the dark values. C16 overlays and L5 HUD keep their fixed colours. L1–L8 are Light Classic rules.

---

## Decisions log

- 2026-11-15, Roman: menus MN1–MN6 approved (canvas https://claude.ai/artifact/4C5Z47zFtr7PiiQD8qPtyv).

2026-11-15, Roman: K1 one `SCRIM()` (P4) · K2 one `ROW_HOVER` white(10) (H1) · K3 lime glow stays (B5) · K4 heights 44/32/28 (B2) · K5 `AI()` token (C15) · K6 Esc closes every dialog except while work runs (P4) · K7 Keep Awake chart in lavender steps (SP8).

2026-11-15, Roman: whole system approved, including L1–L7, S4, H6, SP3/SP4 (count badge instead of the rail dot) and SP7–SP9. Details get tuned later through this file (G1).

2026-11-15, Roman: per-screen audit fixes approved (canvas https://claude.ai/artifact/XuR6xDoEmxzkmKMtCwcRmQ), including the new P7 tip placement.

2026-11-15, Roman: big numbers (hero, number) bold, as the rule says (B over thin, M1 comparison).

2026-11-17, Roman: no hover on a selected item (H7).

2026-11-17, Roman: H8 no share fill behind rows (Disk Map option D); Disk Map list rows stay 36.

2026-11-17, Roman: design review of Disk Map, Uninstaller, the Claude chip and lists (canvas https://claude.ai/artifact/FjuTLwroyGMkCmeRxzjPv7): disk line like the start ring (C6), content-list selection edge to edge with lime text (H3), one AI chip look (B1, B8), row tips beside the list (P7), a 2 pt gap between lit rows in inset lists (H1), darker light ACCENT (L8).

2026-11-17, Roman: fan glyph SP11 (canvas https://claude.ai/artifact/9bAg1MmQ2Dd5depsaAyKnC, option 2), everywhere incl. the Fan tool icon.

2026-11-17, Roman: widget sheets SP12 (canvas https://claude.ai/artifact/WN6VdfbDibWn1ZcZ9tA35t) with R3, B1 and T14 additions; R14 master switch (Settings › Cat); widget card S2w, R_TILE 10, B3 segment gap 4, no hover inside widgets (H1), R4 and T14 widget concessions (widget review canvas https://claude.ai/artifact/Gdvrk5REUpSfDMyhK9awnK). Green text in light checked against L8: sheets have no ACCENT text, state GREEN sits on cards at 4.9:1 (C9).

2026-11-18, Roman: C16 on-picture overlays (canvas https://claude.ai/artifact/GE56ikCenBuyEPhXSo2YTE, option B), for the photo editor and pinned screenshots.

2026-11-18, Roman: C16 handles are variant B4 (corner brackets, side pills), 5 pt outside the mark.

2026-11-18, Roman: Mac Health page review (canvas https://claude.ai/artifact/Gue4tzFYVgffpsduUJMkHu): R7 no chip for the everyday state, C4 sparks one colour, MN6 row "⋯" on hover.

2026-11-18, Roman: T21 shortcuts and mouse buttons (canvas https://claude.ai/artifact/Y1KGGgepf5m15MYkmCv6CX).

2026-11-18, Roman: S1 glow never smaller than the default window's, widget cards get CARD_EDGE, no TILE_SHADE (panel variant C, canvas https://claude.ai/artifact/XJi3CqUKt9j86WLFN1AHE7); W5 Row 1:4 on one shared divider line, ROW_SPLIT 172 (variant B, canvas https://claude.ai/artifact/Gdvrk5REUpSfDMyhK9awnK).

2026-11-18, Roman: R15 card rows stop at 340 pt in tall windows; big numbers plain under a naming title, "CPU load … 16%" (canvases https://claude.ai/artifact/WNxYgmBHLUs5aBFmAZS13K, https://claude.ai/artifact/MJ4ox66fzu6By8W8dTKb2J).

2026-11-18, Roman ("щоб було однаково"): W5 bars inline in every size (1:4 variant A, Medium/Large M-A), shared name / bar / value columns.

2026-11-18, Roman (live on his Mac, "хай вони будуть такі як ці кнопки"): widget cards S2w/W1 take the panel header buttons' quiet-key surface with a softer hover (40%); the S1 glow is a beam down the centre to the bottom, in the panel and the app window; native Liquid Glass dropped. Settings › General adds "Keep menu bar panel open" (off by default).

2026-11-18, Roman: SP9 Keep Awake 1:2 is a Wide like the others (variant A, canvas https://claude.ai/artifact/NgHchuWhYA8ERfC2M2pN5K); the control stays on the page.

2026-11-18, Roman: 1:4 name column 70, value 44 (names in full on SF), device lines show their type in 1:4.

2026-11-18, Roman ("щоб було консістент"): Uninstaller and Disk Map start screens follow SP1 like Cleanup, variant A (canvas https://claude.ai/artifact/EQCfEUPxzBcgPzX6mwRhJJ).

2026-11-18, Roman: buttons look like widgets (canvas https://claude.ai/artifact/ND3dqKVXA3VNCfycyAge34): 1 pt base everywhere, quiet keys use the widget surface (B4), glow only on main keys ≥ 32 pt, softer (B5); press drops 1 pt onto the base for every kind (B7).

2026-11-18, Roman: M5 window-open loader, look A (canvas https://claude.ai/artifact/KxiUnErRbP6J3sadUv6B1z).

2026-11-18, Roman: row content 12 pt inside a lit fill everywhere (H9), no hairlines at a lit row (H2), start list rows (SP1); canvas https://claude.ai/artifact/5UDhPQqitdkayGPa28m98i, variant A.

2026-11-18, Roman: selection follows the hover rules: in card lists every lit row is inset 4 pt, radius 8, with no dividers next to it (H2, H3; PR #276).

2026-11-18, Roman: button glow sits deep under the key, narrower than it, lighter tint (B5, variant 6; renders /mnt/project-files/button-glow/).

2026-11-18, Roman: C16 overlays white with a black edge instead of lime (selection frame variant C, PR #283); pin edge and corner grip follow.

2026-11-18, Roman: AI Usage redesign rules (PR #280): SP2 pace arc, R4 page lists use W5 lines, R10 project names, R15 grid, T14 24 h resets.

2026-11-18, Roman: light-theme data cards and widgets are a white sheet with a soft shadow (variant A, canvas https://claude.ai/artifact/JvUYUzsJhHwHPd2vd9QhCA); quiet keys stay as they are (S2w, W1).

2026-11-23, Roman: Mac Health v3 (canvas https://claude.ai/artifact/RLBUbQ54MYxGDAYufjnZhy, PR #292): details popup P8, clickable cards (S2), card titles 15/13 (R2), widget sheets retired (SP12), header tips SP14.

2026-11-23, Roman: details popup content has no inner boxes (P8, PR #295).

2026-11-24, Roman: three themes Light Meow, Dark Meow, Miami (canvas https://claude.ai/artifact/EfWjmLd5axdU3BGKNzKRmB): §13 TH1–TH9, C1/C2/C3/C8/S1 wording.

2026-11-24, Roman: TH10 desktop cat per theme (canvas https://claude.ai/artifact/2UCHdgZAiUXjxCu2k1FzSd, L1 + M1).

2026-11-24, Roman: themes renamed Light Classic, Dark Classic, Dark Miami (were Light Meow, Dark Meow, Miami).

2026-11-24, Roman: onboarding theme picker TH11–TH13 (canvas https://claude.ai/artifact/Mc61YS2APhuPzWLE691Bm3; animation H, Light art L2, softest Dark Miami glow).

2026-11-24, Roman: W5 two-value lines and SP15 chart readout (PR #311, canvas https://claude.ai/artifact/6Vfbmwffoeys9HTvzdG6bd); S7 sample card and onboarding subtitle gap 24 (R2, PR #310).

2026-11-24, Roman ("merge all"): P9 permission card with `POPOVER` fill (Screen Recording card moves to 32 buttons, 16 inset; PR #313); SP3 footer acts on the picked group (canvas https://claude.ai/artifact/YEwNFRvzq6TUSm3Ppcwtgv); B1 panel header "…" (PR #274).

2026-11-24, Roman: history graph captions 8 under the bars, stats row 16 below (W5, PR #324).

2026-11-24, Roman: R16 same setting words on both Capture pages, R17 one shared permission row in tool Settings (Screenshot, Video, Dictation) (canvas https://claude.ai/artifact/5C9qp5rfHB3uF27JQ1taf4).

2026-11-24, Roman: panel header Quit + Open, Quit confirms in place (B1, variant A, branch claude/project-thread-otzc8q).

2026-11-24, Roman: SP16 page header controls (action right, never hides, fixed width), refresh as icon pill in headers, SP14 tips with a fix, provider-named status chips (canvas https://claude.ai/artifact/8UbQsaY3dRZffn58yyo2Eo).
