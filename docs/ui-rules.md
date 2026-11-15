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
| AI | `AI()` (cyan) | glass.rs |
| Action | `ACTION` (lime fill; was `BLUE`) · `DANGER` (destructive button fill) · `ON_ACTION` · `ACCENT` (lime as text) | glass.rs |
| State | `GREEN` `AMBER` `RED` · `CHIP_ASK` `CHIP_RISK` | glass.rs |
| Group tints | `tint::CLEANUP` `HEALTH` `CAPTURE` `KEEP_AWAKE` `NEUTRAL` | glass.rs |
| Neutral | `GRAY` (`OTHER` alias) · `series::monitor(i)`, `series::clean(i)` · `letter_tile()` · `white(a)` ink overlay · `HAIRLINE` 15 · `HAIRLINE_STRONG` 26 | glass.rs |
| Spacing | `sp::XS` 4 · `S` 8 · `M` 12 · `L` 16 · `XL` 24 · `XXL` 32 | glass.rs |
| Radius | `R_CARD` 12 · `R_BUTTON` 10 · `R_SMALL` 7 | glass.rs |

Every colour is a `pick(dark, light)` pair. Theme follows the system; `CMDMEOW_THEME=light|dark` overrides it for screenshots. ✅

## 2. Colour

Palette: B3 Moss & Lime ✅ (2026-11-11). Canvas for C1–C14: https://claude.ai/artifact/V9GF4FWnmFs2vjzs4UvfeU C1–C14 ✅ 2026-11-15.

| # | Rule | Status |
|---|------|---|
| C1 | No raw colours outside `glass.rs`. Every colour has a dark and a light value (`pick()`). | ✅ |
| C2 | Lime is only for what you press or select. Never data, except as Clean's group tint (C3). In light mode lime is never text or a thin stroke: use `ACCENT` (`lift()`). | ✅ |
| C3 | A tool uses its group tint for its icon and its main data: Clean lime, Monitor cyan, Create apricot, Utilities lavender. | ✅ |
| C4 | `GREEN` / `AMBER` / `RED` only mean state. Never a category or chart series. | ✅ |
| C5 | Chart series: group tint first, then its next steps, at most 3. The rest is Other in `GRAY`. Free / idle / available is the empty track. (`series::*`) | ✅ |
| C6 | The same thing has the same colour on every screen. | ✅ |
| C7 | A legend square only when its colour is on a chart on the same screen. | ✅ |
| C8 | Contrast in both modes: body text 4.5:1, captions and section titles 3:1, icons and chart marks 3:1. | ✅ |
| C9 | Neutral marks (You, Other, an app with no icon) use `GRAY`. No system greys. | ✅ |
| C10 | One neutral letter tile (`letter_tile()`) for apps with no icon. | ✅ |
| C11 | Links use `ACCENT`, hover included. No system blue. | ✅ |
| C12 | Temperature: cyan under 68 °C, amber from 68, red from 90. Text under 68 °C is plain. | ✅ |
| C13 | Exceptions only: capture/record HUD, the recording dot, traffic lights, third-party brand marks, user content. | ✅ |
| C14 | Names say the role: `BLUE`→`ACTION`; `INDIGO`, `PURPLE`, `TEAL`, `CLAUDE` go. | ✅ |
| C15 | AI (AI Check chip, reveal ring, agent marks) has its own token `AI()`, cyan (today the same value as `tint::HEALTH`). | ✅ |

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

Light and series values (✅ 2026-11-15): Clean #7DB519→#5B8A14, Monitor #1F8AA0→#177A8E, Amber/Create #C06A12→#A65A0E, Green #4B8625→#437A20, light junk ramp #5B8A14 #507E14 #467318 #3E6A1A #36601B #2F571C #284F1E, dark ramp last step #46824B→#4F8E4E. Monitor series dark #6FD6E8 / #B4EAF3 / #4FA9BC, light #177A8E / #0B4652 / #3A97AB.

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
| S3 | **R13** A tool's start screen (before its first run) has no cards: content is centred on the backdrop. Cards are only for pages with live data or lists (R2 applies there). Cardless: Cleanup, Disk Map, Uninstaller start. With cards: Mac Health, AI Usage, Fan, Startup, Cleanup results. | ✅ |
| S4 | A list is one card with full-bleed rows and hairlines between them. No card per item. | ⏳ |
| S5 | Cards and page content have no shadow. Floating surfaces get only the "Whisper" shadow (P2). | ✅ |
| S6 | Group tile (tool icon): group colour 12% fill, 28% edge, glyph in group colour. Neutral tools: `white(18)`. | ✅ |

## 6. Buttons and controls ✅

Approved by Roman for the base system on 2026-11-15. Canvas: https://claude.ai/artifact/ND3dqKVXA3VNCfycyAge34 · source spec: `button-press/button-rules.md` in the project files (not in the repo).

| # | Rule |
|---|------|
| B1 | **Kinds.** Hero (`hero_button`, 44, lime): one per empty or first-visit screen. Primary (`pill_button` PRIMARY, lime): one per view or dialog. Danger (`pill_button` DANGER): destructive confirm only. Secondary (`pill_button` SECONDARY): quiet. Side (`side_button`): quiet, with keycaps. Text (`ghost_button`, `link`): no fill. Icon (`icon_button`, square: 32 in headers, 28 in cards and rows), Round (`round_button`, 28, sheet close and refresh), AI chip (`agent_button`, 28). Code still has 30 and 26; the code audit moves them. |
| B2 | **Heights:** 44 hero, 32 (page header, footer, dialogs, side, toggle chips), 28 (inside cards and rows). Nothing else: 30, 34, 36 → 32; 24, 26 → 28. |
| B3 | **Shape:** `R_BUTTON` 10, capped at h/2. Chips h/2. Segmented `R_SMALL` 7, thumb 6, 2 pt gap between segments. |
| B4 | **Raised-key look, one language for every pressable:** fill, a 1 pt top highlight (white 38% on lime/red; quiet: white 75% light, 8% dark), a darker base under the face. Lime/red base 3 pt (tint mixed 62% to black). Quiet kinds base 2 pt ink 14%, plus a 1 pt hairline ink 10% (18% on hover). |
| B5 | **Glow** on lime and red only: `0 6 18 -5`, lime 70%, red 65%; smaller while pressed, none when disabled. |
| B6 | **Danger colour** is `DANGER()`: #F04438 dark, #D92D20 light, white text. Destructive buttons only; `RED()` stays the state colour (C4). |
| B7 | **Press:** lime/red face drops 2 pt, quiet 1 pt, text drops 1 pt with no base. Down 50 ms ease-out, release 160 ms spring `cubic-bezier(.34,1.8,.64,1)`. Pressing never darkens the fill. Switch knob widens 4 pt while held; checkbox check pops .5→1. Content a caller draws on a pill moves with the face (the helper returns the offset). |
| B8 | **States.** Hover: colour only, no movement (lime and lime-on fills mix 26% to white, red 22%; quiet fills ink 10–15% with full-ink text, secondary hairline 24%; ghost text gets an ink 7% backdrop; links lighten and underline; AI chip 12% cyan tint, 60% edge; unchecked check/radio edge 70% + 8% fill). Focus (keyboard only): 1 pt ring 2 pt outside, following the radius, in the button colour (lime, danger red, or ink). Disabled: alpha from one shared helper (L6), no hover, press or sound; the base stays. Loading: push buttons only, a 13 pt Core Animation spinner before the label, width locked, no press or sound. On/selected: segmented thumb; switch lime, knob right; check and radio lime; toggle and filter chips, recording shortcut field: lime tint + `ACCENT` edge. |
| B9 | **Choices:** segmented 32 header / 28 card, switch 30×18, checkbox and radio 16, filter chips 28 through one shared helper, tool toggle 28, shortcut recorder 28, action tile 40–44 (icon over label). |
| B10 | Rows, sidebar items and cards highlight only: no press, no sound. |
| B11 | Only `kit` helpers draw buttons: no plain `egui::Button`, no hand-drawn capsules. Every helper plays its own click; callers never do. |

Consts: `KEY_BASE` 3, `KEY_DROP` 2, `KEY_BASE_QUIET` 2, `KEY_DROP_QUIET` 1, `PRESS_IN` .05, `PRESS_OUT` .16.

## 7. Popups and modals ✅

Approved 2026-11-15 (PR #209, shadow "1 Whisper").

| # | Rule | Status |
|---|------|---|
| P1 | **Surface:** `POPOVER` fill (egui menus `BG`), 1 pt inside edge `POPOVER_EDGE` (dark `white(30)`, light ink ~20%). Radius: dialogs and sheets 14, popovers and bars 12, hover tip and menus 10, onboarding hint 8, toasts pill. | ✅ |
| P2 | **Floating shadow "Whisper"**, one token `float_shadow()`: two black layers, key (0,1) blur 2 at .30 dark / .06 light, ambient (0,4) blur 10 at .20 dark / .06 light. egui menus and tooltips: `float_shadow_egui()` one layer (0,3) blur 8 at .32 / .08. Only floating things get it (menus, tooltips, popovers, dialogs, sheets, toasts, floating bars, drag tags); cards, rows and page content never do. Native window shadows and the screenshot "Shadow" option stay. | ✅ |
| P3 | **Modal cover:** while any menu, popover or dialog is open, nothing under it hovers, shows a tip or takes a click (`kit::modal::shield`, `layer`, `pointer(ui)`). A click on the cover closes a menu or popover. | ✅ |
| P4 | **Dim** under dialogs, sheets and the loader: one `SCRIM()` over the page area, black 45% dark, ink 24% light. A click on the dim closes a sheet. Esc closes every dialog and sheet, except while work runs (loader). Destructive confirms never close from a click on the dim. | ✅ |
| P5 | Every removal that can take longer than 0.4 s shows the removal loader (`kit::loader::show`): card 340 wide, padding 24, mark 88, title (lead, verb + "…"), "x of y" (body dim), current path (mono 11 faint, cut in the middle), 28 buttons (Stop where safe; Done, Put back where undo exists). It appears after 0.4 s and stays at least 0.8 s. Scans and measuring keep `spinner()` and the bar. | ✅ |
| P6 | The loader mark is one shape with the ⌘ cut out; Done turns ⌘ into ✓. | ✅ |

## 8. Hover, selection, focus

| # | Rule | Status |
|---|------|---|
| H1 | One hover fill for rows everywhere (lists, rails, popovers, sidebar, start lists): `ROW_HOVER` = `white(10)`. Edge to edge in lists and popovers (H2); inset radius 8 only in the sidebar and rail. | ✅ |
| H2 | Row hover runs edge to edge with no inset and no corners of its own; the container clips it (popover 12 corners, card 12). Hairlines stay. | ✅ (AI Check, hover A) |
| H3 | Selected row (sidebar, group rail): lime at 16% (`with_alpha(ACTION, .16)`), radius 8, text `ACCENT`. | ✅ |
| H6 | Hierarchy by indent: a child row's icon starts under its parent's title text (sidebar items under the section title, bundle children 28 in). Icons in one list share one fixed box (16 in the sidebar), so the label never shifts with the glyph. | ⏳ (Roman 2026-11-15 comment) |
| H4 | Selected card: 1.5 pt lime ring inside. | ✅ |
| H5 | Reveal (`reveal::RING_W` 2, `FILL` .14, `HOLD` 1.8 s, `FADE` 1.4 s): a click on an AI suggestion closes the popover, scrolls to the item and rings it 2 pt cyan with a 12–14% cyan fill; it holds ~1.8 s and fades by ~3.2 s. Only items visible on the page are clickable. | ✅ (canvas https://claude.ai/artifact/WHbrjAR7nMt8RKFkuivn5y) |

## 9. Motion

| # | Rule | Status |
|---|------|---|
| M1 | Decorative and looping animation runs on Core Animation layers (`kit::spin`). egui never repaints in a loop for it; it redraws on state change only. | ✅ |
| M2 | Every animation is finite or tied to visible work, and stops repainting when done. | ✅ |
| M3 | Press: down 50 ms ease-out, release 160 ms spring `cubic-bezier(.34,1.8,.64,1)` (B7). Hover fades via `animate_bool`. | ✅ |
| M4 | Loader: fill eases 0.6 s, wave loops 1.6 s linear, ⌘→✓ crossfade 0.25 s plus one 0.46 s hop. | ✅ |

## 10. Sound ✅

Approved for the base on 2026-11-15; fine-tuning later (files are swappable, levels live in code). One tonal family for clicks and cues.

| # | Rule |
|---|------|
| N1 | **Click:** plastic "higher" (~57 ms, 400 Hz high-pass), three takes, source `button-press/clicks/final/7-higher-1..3.wav` in the project files (not in the repo). Played on mouse down, always with the visual press, never alone. |
| N2 | **Three levels, not one per kind:** Emphasized (hero, primary, danger) 0 dB; Standard (secondary, side, icon, round, tool) −3 dB; Subtle (chips, segment, filter, switch, shortcut, check, radio, text) −6 dB; none for rows, sidebar, cards, hover. Relative to `CLICK_MASTER`, tuned by ear on the M1 (target ≈ −6 dB under cues; starts at 0 dB). |
| N3 | **Pitch per kind in whole intervals:** hero −2, primary 0, danger −3 semitones; standard +2/+3; subtle +2…+5. |
| N4 | **Variation:** ±3% pitch, ±6% level, never the same take twice, at most one click per 100 ms. |
| N5 | **Silent when:** disabled or busy, Settings › Sounds off (`sounds_on`), macOS "Play user interface sound effects" off, recording, screenshot mode, launch, any click the user didn't make. |
| N6 | **Playback:** `kit::sound::click(Click)` through AudioToolbox system sounds (low latency), never afplay. Files: WAV mono 44.1 kHz 16-bit, peak −1 dBFS, owned (Magnific / synthesized). |
| N7 | **Cues** in A major (A4 C#5 E5 A5), each opening with a soft plastic tick at −12 dB: Ready C#5→E5→A5 400 ms; Done A4 C#5 E5 A5 arpeggio 600 ms; Oops A4→G#4→E4 450 ms (not an alarm); Awake / Asleep E5→A5 / A5→E5 350 ms at −3 dB. |

## 11. Screen patterns

| # | Pattern | Status |
|---|------|---|
| SP1 | **Start screen** (before the first run): no cards (S3). Left: the disk ring or tool icon, the action button 32 below it. Right of a 1 pt hairline divider with a 48 gap: the list straight on the backdrop. | ✅ |
| SP2 | **Disk ring** (`kit`, one function, Cleanup and Disk Map start): 208 across, 12 stroke, track `white(16)`, used arc in the tool tint with a round cap from 12 o'clock clockwise. Centre: free space in hero 28/600 over "free of <total>" in label 12 `TEXT_DIM`. 12 below the ring: a 16 disk icon and the volume name in body `TEXT_SOFT`. Disk Map's own ring (r 64, "X of Y used") goes. | ✅ |
| SP10 | **Start list rows on the backdrop** (Cleanup groups): a 44 head line with "Last scan 2 days ago" as a 12/600 section title and the total right; rows 44 with a 28 tinted icon circle and the name, hairlines between, `ROW_HOVER` on hover. A group's description shows on hover only. After a scan each row gets a 4 pt bar and a right-aligned size. | ✅ |
| SP3 | **Group rail** (results): a card 232 wide left of the content, 16 gap, 4 padding, rows 40 with a 24 group chip, name body 13, size right in dim; selection H3. | ⏳ |
| SP4 | **Bundles**: 3+ alike items fold into one row with an arrow and "N items"; children are 44 rows indented 28. Kept items fold into one "Kept" row. | ⏳ |
| SP5 | **Empty state**: R6. | ✅ |
| SP6 | **Locked tool**: a permission screen per tool that names what is missing in the words of the fix (T16), not a dimmed page. | ✅ |
| SP7 | **Big state + control** (Keep Awake): centred scene, title hero 28, detail body dim in a fixed 18 line (kept when empty), then one segmented control 32 tall (Off · Auto · On). The control is the only switch, so state and control never disagree (T17). | ⏳ |
| SP8 | **Hourly chart**: 24 columns, gap 4, 72 tall, stacked series in lavender steps (On = `tint::KEEP_AWAKE`, Auto = `series::util(1)`; never `GREEN`, C4) (top corners radius 3), dashed `white(26)` guides with 11 faint labels in a 36 right gutter, 00/06/12/18 under it. Hover a column → tooltip plus 1.5 outline, linked to the list below. Legend sits on the section title row (10 swatch, label 12 dim, value bold 12). | ⏳ |
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

## Decisions log

2026-11-15, Roman: K1 one `SCRIM()` (P4) · K2 one `ROW_HOVER` white(10) (H1) · K3 lime glow stays (B5) · K4 heights 44/32/28 (B2) · K5 `AI()` token (C15) · K6 Esc closes every dialog except while work runs (P4) · K7 Keep Awake chart in lavender steps (SP8).
