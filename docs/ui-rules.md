# CmdMeow design system

The one source of truth for how CmdMeow looks, moves and sounds. Every UI thread, PR and canvas follows it.

- **Tokens live in code** (`src/kit/glass.rs`: `sp::*`, `ty::*`, `R_*`, colour fns, `tint::*`). Use them, never raw numbers or hex outside `glass.rs`.
- **System first, then code (G1, Roman 2026-11-15).** A component or rule changes here first (doc + Design System canvas, Roman's OK), then in the code everywhere it is used, in one PR. Never change a component on one screen only.
- **Reuse (G2).** Build screens from the `kit` components. A new component is added only when no existing one fits; it goes into this doc and `glass.rs` before any screen uses it. No local copies of a component.
- **One owner (G3).** Changes go through the design-system thread. Other threads propose a rule there; they don't edit this file.
- **Canvas:** https://claude.ai/artifact/QarEbeJ1pB33366Kc8LsY7
- **Status:** ✅ approved by Roman · ⏳ proposed, waiting on Roman. A ⏳ rule is not built until it turns ✅.

Contents: 1 Tokens · 2 Colour · 3 Type · 4 Spacing and layout · 5 Surfaces · 6 Buttons and controls · 7 Popups and modals · 8 Hover, selection, focus · 9 Motion · 10 Sound · 11 Screen patterns · 12 Writing text

---

## 1. Tokens

| Group | Tokens | Where |
|---|---|---|
| Surfaces | `BG` `CARD` `CARD_EDGE` `RAISED` `POPOVER` `SIDEBAR` `FOOTER` | glass.rs |
| Text | `TEXT` `TEXT_SOFT` .84 · `TEXT_DIM` .68 · `TEXT_FAINT` .56 · `TEXT_MUTED` .40 · `ty::caption_color` | glass.rs |
| Action | `BLUE` (lime fill, renamed `ACTION` ⏳) · `ON_ACTION` · `ACCENT` (lime as text) | glass.rs |
| State | `GREEN` `AMBER` `RED` · `CHIP_ASK` `CHIP_RISK` | glass.rs |
| Group tints | `tint::CLEANUP` `HEALTH` `CAPTURE` `KEEP_AWAKE` `NEUTRAL` | glass.rs |
| Neutral | `GRAY` (`OTHER` ⏳) · `white(a)` ink overlay · `HAIRLINE` 15 · `HAIRLINE_STRONG` 26 | glass.rs |
| Spacing | `sp::XS` 4 · `S` 8 · `M` 12 · `L` 16 · `XL` 24 · `XXL` 32 | glass.rs |
| Radius | `R_CARD` 12 · `R_BUTTON` 10 · `R_SMALL` 7 | glass.rs |

Every colour is a `pick(dark, light)` pair. Theme follows the system; `CMDMEOW_THEME=light|dark` overrides it for screenshots. ✅

## 2. Colour

Palette: B3 Moss & Lime ✅ (2026-11-11). Canvas for C1–C14: https://claude.ai/artifact/V9GF4FWnmFs2vjzs4UvfeU

| # | Rule | Status |
|---|------|---|
| C1 | No raw colours outside `glass.rs`. Every colour has a dark and a light value (`pick()`). | ⏳ |
| C2 | Lime is only for what you press or select. Never data. In light mode lime is never text or a thin stroke: use `ACCENT` (`lift()`). | ⏳ |
| C3 | A tool uses its group tint for its icon and its main data: Clean lime, Monitor cyan, Create apricot, Utilities lavender. | ⏳ |
| C4 | `GREEN` / `AMBER` / `RED` only mean state. Never a category or chart series. | ⏳ |
| C5 | Chart series: group tint first, then its next steps, at most 3. The rest is Other in `GRAY`. Free / idle / available is the empty track. (`series::*`) | ⏳ |
| C6 | The same thing has the same colour on every screen. | ⏳ |
| C7 | A legend square only when its colour is on a chart on the same screen. | ⏳ |
| C8 | Contrast in both modes: body text 4.5:1, captions and section titles 3:1, icons and chart marks 3:1. | ⏳ |
| C9 | Neutral marks (You, Other, an app with no icon) use `GRAY`. No system greys. | ⏳ |
| C10 | One neutral letter tile (`letter_tile()`) for apps with no icon. | ⏳ |
| C11 | Links use `ACCENT`, hover included. No system blue. | ⏳ |
| C12 | Temperature: cyan under 68 °C, amber from 68, red from 90. Text under 68 stays plain. | ⏳ |
| C13 | Exceptions only: capture/record HUD, the recording dot, traffic lights, third-party brand marks, user content. | ⏳ |
| C14 | Names say the role: `BLUE`→`ACTION`; `INDIGO`, `PURPLE`, `TEAL`, `CLAUDE` go. | ⏳ |
| C15 | AI (AI Check chip, reveal ring, agent marks) uses cyan `tint::HEALTH`. | ⏳ conflict K5 |

**Light mode** (from the light audit, https://claude.ai/artifact/4oYGWVYxcUrvtpw7TfLAJQ) ⏳

| # | Rule |
|---|------|
| L1 | Light `TEXT_MUTED` ink .40→.50, caption ink 128→153, to meet C8. |
| L2 | Light `CARD_EDGE` ink 20→31 so white cards don't merge into the sage backdrop. |
| L3 | Hairlines and dividers use `white(HAIRLINE*)` only; never hard-coded black or white alpha. |
| L4 | Switch off track: light ink 56 (dark 41). Inactive pager dots: light ink 72 (dark 46). |
| L5 | Surfaces that stay dark in both themes (dictation HUD pill) use fixed light content, never theme ink. |
| L6 | Disabled or locked: alpha .45 in dark, .62 in light. |
| L7 | A popover over a card keeps a visible edge in light mode. |

Proposed light values (C-thread): Clean #7DB519→#5B8A14, Monitor #1F8AA0→#177A8E, Amber/Create #C06A12→#A65A0E, Green #4B8625→#437A20, light junk ramp #5B8A14…#284F1E. Monitor series dark #6FD6E8 / #B4EAF3 / #4FA9BC, light #177A8E / #0B4652 / #3A97AB. ⏳

## 3. Type ✅

Hero 28/600, number 22/600, lead 15/600, title 13/600, section 12/600, body 13/400, label 12/400, caption 11/400 (`ty::*`). Bold 11 and 12 are allowed for chips and small values. Mono 11 for paths. Only exceptions: the speed dial (52) and the app mark (40). Light mode uses heavier weights (body 500, hero 500).

## 4. Spacing and layout ✅

**Scale:** 4, 8, 12, 16, 24, 32 (`sp::XS`…`sp::XXL`). Nothing in between.

| # | Rule | Token |
|---|------|-------|
| R1 | Content starts 24 pt from the window edge, header included. | `sp::PAGE_EDGE` |
| R2 | Cards: padding 16, gap 16, radius 12. Title, 12, content. A section title sits 12 above its card. | `sp::L`, `sp::M`, `R_CARD` |
| R3 | Two row heights: settings rows 40 (label left, control right, hairline between), list rows 44 (icon, name, detail). | `sp::SETTINGS_ROW_H`, `sp::ROW_H` |
| R4 | Rows with a bar: 36 tall, name and value on one line, a 4 pt bar 8 pt under them. | `sp::BAR_ROW_H` |
| R5 | Controls are 28 inside cards and 32 in page headers. | `sp::CONTROL_H`, `sp::HEADER_CONTROL_H` |
| R6 | One empty state: centred, 56 icon chip, lead title, body up to 400 wide in dim, one secondary button. Inside a card: one caption-size line in the middle. | `sp::XXL` around it |
| R7 | Chips say a state (Normal, Charging, In menu bar), never an action: 11/600 on a 16% tint. | `chip()` |
| R8 | Numbers in lists and tables are right-aligned. | |
| R9 | A title with a detail line under it keeps 6 between the lines; such rows are 52 tall. Columns in a row sit 16 apart. | `sp::LINE_GAP`, `two_lines_y()`, `sp::ROW2_H`, `sp::COL_GAP` |
| R10 | Text the app writes is never cut with "…": it wraps or the layout changes. Only names and paths people bring may be cut. Find cut text with `CMDMEOW_FIT_LOG=1`. | `text_wrap()` |
| R11 | A list's section title is 12/600 in a 44 row: 16 above, 12 below to the first row; its link sits on the same line. Table heads sit in a 36 row with the hairline at its bottom. | `section_title()`, `sp::SECTION_H`, `sp::TABLE_HEAD_H` |
| R12 | Scroll bars float in a 14 gutter on the right, taken only when the content scrolls. Content in a scroll area takes `ui.available_width()`. | `sp::SCROLL_GUTTER` |

Fixed chrome: sidebar 210 wide, sidebar rows 28, page header 50, page footer 60, sheet header 58.

## 5. Surfaces

| # | Rule | Status |
|---|------|---|
| S1 | Backdrop: radial moss gradient (dark) / sage (light), 1 pt `HAIRLINE_STRONG` edge. Menu bar panel background is solid, no blur. | ✅ |
| S2 | Card: `CARD` fill, 1 pt `CARD_EDGE`, radius 12, flat. No frame inside a frame. | ✅ |
| S3 | A tool's start screen (before the first run) has no cards: content is centred on the backdrop. Cards are for pages with live data and lists. | ⏳ (R13 proposal) |
| S4 | A list is one card with full-bleed rows and hairlines between them. No card per item. | ⏳ |
| S5 | No drop shadows on anything floating (menus, tooltips, popovers, dialogs, sheets, toasts, drag tags). Native window shadows and the screenshot frame "Shadow" option stay. | ✅ |
| S6 | Group tile (tool icon): group colour 12% fill, 28% edge, glyph in group colour. Neutral tools: `white(18)`. | ✅ |

## 6. Buttons and controls

Canvas: https://claude.ai/artifact/ND3dqKVXA3VNCfycyAge34

| # | Rule | Status |
|---|------|---|
| B1 | Kinds: Hero (44, lime, one per empty or first-visit screen) · Primary (lime, one per view or dialog) · Danger (red, destructive confirm only) · Secondary (ink 4% fill, 1 pt ink 14% edge) · Text / link (no fill) · Icon 30 square · Round 26 (sheet close, refresh) · AI chip 26. | ⏳ |
| B2 | Push buttons have three heights: 44 hero, 32 (page header, footer, dialogs), 28 (inside cards and rows). | ⏳ conflict K4 |
| B3 | Radius `R_BUTTON` 10 (capped at h/2). Chips h/2. Segmented `R_SMALL` 7 with a 6 thumb. | ✅ |
| B4 | No glow under buttons. A darker base under the face gives the depth. | ⏳ |
| B5 | Press "key": the face sinks into a darker base and springs back. Lime and red: base 3, drop 2. Quiet kinds: base 2, drop 1. Text: drops 1, no base. | ✅ feel · ⏳ numbers |
| B6 | Hover changes colour only (lime and red lighten 12%). Disabled: alpha .45 (.62 light, L6), no press, no sound. | ⏳ |
| B7 | Controls: segmented 32 header / 28 card, switch 30×18, checkbox and radio 16, filter chips 26 through one shared helper. | ⏳ |
| B8 | Rows, sidebar items and cards highlight only: no press, no sound. | ⏳ |
| B10 | States for every kind: default, hover (colour only), pressed (B5), focus (2 pt `ACCENT` ring set 3 pt out), disabled (L6 alpha, no hover, press or sound), loading (push buttons only: a spinner before the label, width locked), on/selected for choices. | ⏳ |
| B9 | Only `kit` helpers draw buttons; no plain `egui::Button` and no hand-drawn capsules. | ⏳ |

Planned consts: `KEY_BASE` 3, `KEY_DROP` 2, `KEY_BASE_QUIET` 2, `KEY_DROP_QUIET` 1, `PRESS_IN` .05, `PRESS_OUT` .16.

## 7. Popups and modals

| # | Rule | Status |
|---|------|---|
| P1 | Surface: `POPOVER` fill, 1 pt inside edge `POPOVER_EDGE` (dark `white(30)`, light ink ~20%, so white popovers stay apart from white cards; covers L7), no shadow. Radius 14 dialogs and sheets, 12 popovers and bars, 10 small menus and tips. | ✅ · ⏳ light edge (PR #209) |
| P2 | While any menu, popover or dialog is open, nothing under it hovers, shows a tip or takes a click (`kit::modal::shield`). Clicking outside closes a menu or popover. Views that read the pointer use `kit::modal::pointer(ui)`. | ✅ |
| P3 | Dialogs and sheets dim the page under them: one scrim token for both themes. | ⏳ conflict K1 |
| P4 | A click on the dim closes a sheet. Destructive confirms close only with Cancel or Esc. | ⏳ conflict K6 |
| P5 | Every removal that can take longer than 0.4 s shows the removal loader (`kit::loader::show`): card 340 wide, padding 24, mark 88, title (lead, verb + "…"), "x of y" (body dim), current path (mono 11 faint, cut in the middle), 28 buttons (Stop where safe; Done, Put back where undo exists). It appears after 0.4 s and stays at least 0.8 s. Scans and measuring keep `spinner()` and the bar. | ✅ |
| P6 | The loader mark is one shape with the ⌘ cut out; Done turns ⌘ into ✓. | ✅ |

## 8. Hover, selection, focus

| # | Rule | Status |
|---|------|---|
| H1 | One hover fill for rows everywhere (lists, rails, popovers, sidebar): `ROW_HOVER` = `white(10)`. | ⏳ conflict K2 |
| H2 | Row hover runs edge to edge with no inset and no corners of its own; the container clips it (popover 12 corners, card 12). Hairlines stay. | ✅ (AI Check, hover A) |
| H3 | Selected row (sidebar, group rail): lime at 16% (`with_alpha(BLUE, .16)`), radius 8, text `ACCENT`. | ✅ |
| H4 | Selected card: 1.5 pt lime ring inside. | ✅ |
| H5 | Reveal (`reveal::RING_W` 2, `FILL` .14, `HOLD` 1.8 s, `FADE` 1.4 s): a click on an AI suggestion closes the popover, scrolls to the item and rings it 2 pt cyan with a 12–14% cyan fill; it holds ~1.8 s and fades by ~3.2 s. Only items visible on the page are clickable. | ⏳ (canvas https://claude.ai/artifact/WHbrjAR7nMt8RKFkuivn5y) |

## 9. Motion

| # | Rule | Status |
|---|------|---|
| M1 | Decorative and looping animation runs on Core Animation layers (`kit::spin`). egui never repaints in a loop for it; it redraws on state change only. | ✅ |
| M2 | Every animation is finite or tied to visible work, and stops repainting when done. | ✅ |
| M3 | Press: down 50 ms ease-out, release 160 ms spring `cubic-bezier(.34,1.8,.64,1)`. Hover fades via `animate_bool`. | ⏳ |
| M4 | Loader: fill eases 0.6 s, wave loops 1.6 s linear, ⌘→✓ crossfade 0.25 s plus one 0.46 s hop. | ✅ |

## 10. Sound

| # | Rule | Status |
|---|------|---|
| N1 | Pressables play one mechanical-key take on mouse down; never the same take twice, ±3% pitch. Each kind has its own rate and gain (hero .9/1.0 … text 1.25/.4). Rows: silent. | ⏳ |
| N2 | Muted by Settings › Sounds and by macOS "Play user interface sound effects". Off while recording and in screenshot mode. | ⏳ |
| N3 | Played through AudioToolbox system sounds (low latency), not afplay. Sound files are synthesized or Magnific-generated, owned, fine for the public repo. | ⏳ |

## 11. Screen patterns

| # | Pattern | Status |
|---|------|---|
| SP1 | **Start screen** (before the first run): no cards (S3), content centred: disk ring or icon, one Hero button, the list below it on the backdrop with 44 rows and hairlines. | ⏳ |
| SP2 | **Disk ring** (Cleanup and Disk Map start): 208 across, 12 stroke, used in the group tint, track `white(16)`. Inside: free space at hero 28/600 over "free of 2.00 TB" in label dim. Under it: disk icon and "Macintosh HD" in body soft. One shared component. | ⏳ |
| SP3 | **Group rail** (results): a card 232 wide left of the content, 16 gap, 4 padding, rows 40 with a 24 group chip, name body 13, size right in dim; selection H3. | ⏳ |
| SP4 | **Bundles**: 3+ alike items fold into one row with an arrow and "N items"; children are 44 rows indented 28. Kept items fold into one "Kept" row. | ⏳ |
| SP5 | **Empty state**: R6. | ✅ |
| SP6 | **Locked tool**: a permission screen per tool that names what is missing in the words of the fix (T16), not a dimmed page. | ✅ |
| SP7 | **Big state + control** (Keep Awake): centred scene, title hero 28, detail body dim in a fixed 18 line (kept when empty), then one segmented control 32 tall (Off · Auto · On). The control is the only switch, so state and control never disagree (T17). | ⏳ |
| SP8 | **Hourly chart**: 24 columns, gap 4, 72 tall, stacked series (top corners radius 3), dashed `white(26)` guides with 11 faint labels in a 36 right gutter, 00/06/12/18 under it. Hover a column → tooltip plus 1.5 outline, linked to the list below. Legend sits on the section title row (10 swatch, label 12 dim, value bold 12). | ⏳ |
| SP9 | **Widgets**: 1:1 (scene + one word, tap toggles) and 1:2 (scene + two lines + control) only. | ⏳ |

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
| T14 | Every number and time says what it is, with a word or a symbol, never a bare value. | "17:04", "4 / 8", "1/2" | "Updated 17:04", "↻ 19:53", or drop it |
| T15 | One column, one kind of data. Don't mix counts, shortcuts and states in the same place. | sidebar right column: "11", "⇧⌘2", "Set up" | shortcuts only |
| T16 | A locked or disabled thing says what is missing, in the words of the fix. | lock icon; "Full Disk Access · Finding files to clean up" | "Needs Full Disk Access" |
| T17 | A big state and its control never disagree. If something else holds the state, say who. | "Awake" with the switch off | "Awake · by Claude" |
| T18 | A setting's name says what it does, not a nickname or an internal step. | "Put the clipboard back", "Step 1 of 2 · Looking for junk" | "Restore clipboard after typing", "Scanning…" |
| T19 | No rows of small unlabeled icons. Use words, or drop them. | tool icons next to each permission | "Used by Cleanup, Disk Map" on hover |
| T20 | A control with no label gets one, or goes. | a radio button next to a switch | a column head ("Main") |

---

## Open conflicts (for Roman)

| # | Conflict | Proposed pick |
|---|------|---|
| K1 | Dim under dialogs in light mode: dialogs use black 45% in both themes; the loader uses ink 24% in light. | One `SCRIM()`: black 45% dark, ink 24% light. |
| K2 | Row hover is `white(10)` on Cleanup start, `white(12)` on the group rail and sidebar, `white(13)` r8 inset on Keep Awake, a 4 pt inset pill in popovers. | One `ROW_HOVER` = `white(10)` (already in PR #211, the value Roman saw in hover A), edge to edge in lists and popovers (H2); inset r8 only in the sidebar and rail. |
| K3 | Lime glow under primary buttons: the B3 tokens have it, the shadow rule and the key press drop it. | No glow (B4). |
| K4 | Button heights: B2 says 44/32/28, but side buttons and toggle chips are 34, action tiles 40–44. | Side and toggle chips go to 32; action tiles are tiles, not push buttons. |
| K5 | AI uses Monitor cyan, so AI marks look like Monitor data on Clean screens (C3). | Keep cyan as the AI colour (C15) under its own name, `AI()`. |
| K6 | Esc closes the Uninstaller confirm but not Disk Map or Cleanup confirms. | Esc closes every dialog except while work runs. |
| K7 | Keep Awake's chart stacks On in `GREEN` and Auto in lavender: a state colour used as a series (C4). | On = lavender, Auto = a second lavender step (`series::util(1)`). |
