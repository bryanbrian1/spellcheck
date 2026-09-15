---
name: spellcheck
description: A dark, flat, document-like system where a statistic and a rule never look alike; shared by the Tauri app (src/styles.css) and the website (site/site.css).
colors:
  ground: "#0d0f13"
  panel: "#14171d"
  raised: "#1b1f26"
  ink-well: "#07080a"
  edge-soft: "#232830"
  edge-mid: "#2e343e"
  edge-strong: "#3a424e"
  slate-dot: "#4a515c"
  faint-app: "#6d7683"
  faint-doc: "#7e8895"
  muted: "#a2abb8"
  body: "#cfd6df"
  title: "#e8ecf2"
  line-hair: "#23272f"
  line: "#2e343e"
  teal: "#2fd4c4"
  teal-hi: "#6ce8dc"
  teal-ring: "#1f8a80"
  teal-deep: "#14554f"
  teal-well: "#0d2f2c"
  teal-on: "#04211f"
  measured-blue: "#5b9cf8"
  measured-blue-dim: "#16283f"
  argued-amber: "#f0b429"
  argued-amber-dim: "#3a2c0f"
  red: "#f2555a"
  green: "#46c98b"
typography:
  display:
    fontFamily: "JetBrains Mono, ui-monospace, SF Mono, Menlo, Consolas, monospace"
    fontSize: "clamp(56px, 9vw, 88px)"
    fontWeight: 700
    lineHeight: 0.92
    letterSpacing: "-0.04em"
    fontFeature: "tabular-nums"
  headline:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, system-ui, Helvetica Neue, sans-serif"
    fontSize: "26px"
    fontWeight: 600
    lineHeight: 1.15
    letterSpacing: "-0.02em"
  title:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, system-ui, Helvetica Neue, sans-serif"
    fontSize: "16px"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  body:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, system-ui, Helvetica Neue, sans-serif"
    fontSize: "16px"
    fontWeight: 400
    lineHeight: 1.55
  body-app:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, system-ui, Helvetica Neue, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.45
  label:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, system-ui, Helvetica Neue, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.09em"
  mono:
    fontFamily: "JetBrains Mono, ui-monospace, SF Mono, Menlo, Consolas, monospace"
    fontSize: "0.92em"
    fontWeight: 400
    fontFeature: "tabular-nums"
rounded:
  asset: "3px"
  control: "5px"
  panel: "7px"
  window: "10px"
  rail: "1px"
  dot: "999px"
spacing:
  hair: "2px"
  xs: "4px"
  sm: "8px"
  md: "12px"
  base: "16px"
  lg: "24px"
  xl: "36px"
  section: "44px"
  column: "56px"
components:
  button-primary:
    backgroundColor: "{colors.teal}"
    textColor: "{colors.teal-on}"
    rounded: "{rounded.control}"
    padding: "11px 14px 11px 12px"
    typography: "{typography.title}"
  button-primary-hover:
    backgroundColor: "{colors.teal-hi}"
    textColor: "{colors.teal-on}"
  button-secondary:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.title}"
    rounded: "{rounded.control}"
    padding: "11px 14px 11px 12px"
  button-secondary-hover:
    backgroundColor: "{colors.edge-soft}"
    textColor: "{colors.title}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.body}"
    rounded: "{rounded.control}"
    height: "28px"
    padding: "0 10px"
  button-ghost-hover:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.title}"
  tag-neutral:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.muted}"
    rounded: "{rounded.asset}"
    padding: "3px 6px"
    typography: "{typography.label}"
  tag-measured:
    backgroundColor: "{colors.measured-blue-dim}"
    textColor: "{colors.measured-blue}"
    rounded: "{rounded.asset}"
    padding: "3px 6px"
  tag-argued:
    backgroundColor: "{colors.argued-amber-dim}"
    textColor: "{colors.argued-amber}"
    rounded: "{rounded.asset}"
    padding: "3px 6px"
  card:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.body}"
    rounded: "{rounded.control}"
    padding: "12px 14px"
  tooltip:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.body}"
    rounded: "{rounded.control}"
    padding: "8px 10px"
  input:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.title}"
    rounded: "{rounded.control}"
    height: "34px"
    padding: "0 12px"
  nav-item:
    backgroundColor: "transparent"
    textColor: "{colors.muted}"
    rounded: "{rounded.asset}"
    padding: "6px 10px"
  nav-item-active:
    backgroundColor: "transparent"
    textColor: "{colors.title}"
---

# Design System: spellcheck

## Overview

**Creative North Star: "The Honest Patch Note"**

spellcheck is a small window that sits beside a game, and its visual system is built to be believed rather than admired. Everything is set on one cool near-black ground with hierarchy coming from type and space, not from boxes inside boxes. The single decorative commitment is a colour contract: blue means a measurement with its sample beside it, amber means an argument the app made, and teal is the one action. The system exists in two files that share every token by copy (`src/styles.css` for the app window, `site/site.css` for the website) because neither has a build step; the site is the same world applied to a document at reading size, not a sibling brand.

Density differs by host. The app is a 13px utility surface where every number is monospace so columns align; the site is a 16px document with a 66ch measure, a sticky index rail and a version number as its only display element. Both are dark only, flat by default, and move only in response to hover or focus, at 110ms.

Confirmed rejections: no gradients, no glass, no glow, no framework, no light theme, no card-in-rail-in-card nesting. Amber is never used for a statistic, and neither provenance colour is ever a fill.

**Key Characteristics:**
- One flat dark ground; panels and raised surfaces are one and two steps up the same neutral ramp.
- Provenance is a colour contract, carried by a 2px rail and a small tag, never a fill.
- Teal appears on exactly one primary action per view and on links.
- Numbers, versions, file names and paths are monospace with tabular figures; prose is Inter.
- Motion is hover-only, 110ms, one ease-out; nothing moves on load.

## Colors

A twelve-step cool near-black neutral ramp, one teal accent family for action, and two reserved hues that carry meaning rather than decoration.

### Primary
- **Teal** (`teal`): the one action. Primary button fill, links, the active index marker, the status dot, text selection tint. Exactly one teal-filled control per view.
- **Teal High** (`teal-hi`): hover state of anything teal.
- **Teal Ring** (`teal-ring`): focus ring outer colour on the site; input focus border in the app.
- **Teal Deep** (`teal-deep`): app focus ring, `::selection` background on the site, the update bar's bottom edge.
- **Teal Well** (`teal-well`): the darkest teal; the app's keystone rune tile and update-bar background.
- **Teal On** (`teal-on`): ink on a teal fill (9.1:1 on teal).

### Secondary
- **Measured Blue** (`measured-blue`): a statistic that has its sample beside it. Used as a 2px rail, the tag text, the sample-count figures, and a legend title. 6.9:1 on the ground.
- **Measured Blue Dim** (`measured-blue-dim`): the measured tag's chip background and border only.

### Tertiary
- **Argued Amber** (`argued-amber`): a rule the app applied to this game. Used as a 2px rail, the bold item name inside a suggestion, the tag text, the app's rule-tile border and preview-status dot. 10.3:1 on the ground.
- **Argued Amber Dim** (`argued-amber-dim`): the argued tag's chip background and border only.
- **Red** (`red`): a lane that is behind, a losing matchup, a bad lane note. Never for provenance.
- **Green** (`green`): a favourable matchup sub-line. Never for provenance.

### Neutral
- **Ink Well** (`ink-well`, n-0): the screenshot backing colour.
- **Ground** (`ground`, n-1): the page and the window.
- **Panel** (`panel`, n-2): one step up; sample rows, the closing panel, the app's suggestion dropdown, index hover.
- **Raised** (`raised`, n-3): two steps up; tags, code, tiles, inputs, tooltips, the secondary install button, ordered-list counters.
- **Edge Soft / Edge Mid / Edge Strong** (n-4, n-5, n-6): secondary-button hover fill, the unmarked change rail, scrollbar thumb and its hover.
- **Slate Dot** (n-7): the offline status dot, pipeline station dots, disabled button text.
- **Faint (app)** (`faint-app`, n-8): the app's quietest text at 13px and under, captions and placeholders. 4.2:1 on the ground.
- **Faint (document)** (`faint-doc`): the site's quietest text. Raised off the ramp so 12–13px captions, legal text and the index heading clear 4.5:1 (5.3:1 measured).
- **Muted** (n-9): secondary prose, reasons, index links, block labels.
- **Body** (n-10): running text.
- **Title** (n-11): headings, bold, item names, values in the facts table.
- **Line Hair** (`line-hair`): the default 1px divider between sections, rows and inside components.
- **Line** (`line`): the stronger 1px border for panels, inputs, the banner and legal footer edges.

### Named Rules
**The Provenance Rule.** Blue is a measurement and amber is an argument. Amber is never applied to a percentage, a sample count or anything a reader could mistake for a statistic; blue is never applied to a reason.

**The Rail-Not-Fill Rule.** Blue and amber travel as a 2px left rail plus an 11px tag. Neither saturated hue is ever a background. The only tinted surfaces are the tag chips, which use the dim variants.

**The One Teal Rule.** A view has one teal-filled control. When two install buttons sit together, the visitor's own platform keeps the fill and the other drops to the raised neutral.

## Typography

**Display Font:** JetBrains Mono (bundled latin subset, site only; the app falls back to the system mono stack)
**Body Font:** Inter variable, latin subset, bundled in both hosts (with -apple-system, system-ui, Helvetica Neue, sans-serif)
**Label/Mono Font:** JetBrains Mono on the site; `ui-monospace, SF Mono, Menlo, Consolas` in the app

**Character:** Inter carries every sentence; mono is reserved for anything a reader compares or copies: win rates, game counts, versions, dates, file names, sizes, paths and code. The version number is the site's only display-scale element, and it is set in mono because it is a number.

### Hierarchy
- **Display** (700, `clamp(56px, 9vw, 88px)`, 0.92, −0.04em, tabular): the current version in the banner. Nothing else on either host reaches this size.
- **Headline** (600, 26px, 1.15, −0.02em, balanced wrap): site section headings.
- **Title** (600, 16–18px, 1.15–1.2, −0.01em): site sub-headings and the app's 18px champion name; 15px for install-button labels and legend titles.
- **Body** (400, 16px/1.55 on the site; 13px/1.45 in the app): running text, 66ch measure on the site, `text-wrap: pretty`. The lede is 17px/1.5 at 56ch.
- **Body Small** (400, 13–14px/1.4–1.45, muted): reasons under a change, tooltip copy, captions, legal text.
- **Label** (600, 11px on the site / 10px in the app, 0.09em, uppercase, muted or faint): block titles and the index heading. The only uppercase in the system.
- **Mono** (400–600, 0.92em of context, tabular figures): inline numbers and paths; 13px meta rows on the site, 11px in the app.

### Named Rules
**The Mono-Is-A-Number Rule.** A string is set in mono only if it is a number, a version, a date, a file name, a path, or code. A sentence about state, even a technical one, takes Inter.

**The Reading-Size Rule.** The app reads at 13px because it is a utility window; a document reads at 16px. Both keep the same ramp, tokens and label size ratio, and neither borrows the other's base.

## Layout

The site is a single `max-width` document of `212px + 56px + 680px` centred with a fluid gutter (`clamp(16px, 4vw, 48px)`). The banner is one column in reading order (mark, version, lede, then the install pair side by side at a 640px maximum with the fine print in two matching columns beneath) with a 1px line under it; below it a two-column grid of a sticky index rail (top 24px) and a notes column, 56px apart. Sections are separated by 44px of padding, 44px of margin and a hairline. Rhythm inside prose is 14px between paragraphs, 10px between list items, 26px above a sub-heading.

Breakpoints, observed: at 1000px the rail narrows to 180px and the column gap to 36px; at 860px the install pair goes full width, the index becomes a horizontal scroll row with a fade at its right edge and the "Earlier" list hides; at 560px the legend, platforms and closing panel collapse to one column and the version drops to 56px.

The app is a single scrolling column inside a fixed window: 16px horizontal padding, 24px between blocks, a two-column `2px 1fr` grid per block so the provenance rail column always exists even when unpainted. Rows, not tiles, for anything with a name and a number.

## Elevation & Depth

Flat by default, with tonal layering: ground, panel, raised are three consecutive steps of the neutral ramp separated by hairlines, and that is how nearly all depth is conveyed. Shadows exist in exactly three places and are all soft, dark and short: the site's primary install button at rest, the site's tag tooltip, and the app's suggestion dropdown. There are no glows, no coloured shadows and no inset highlights beyond the dropdown's 1px top edge.

### Shadow Vocabulary
- **Button lift** (`box-shadow: 0 1px 0 rgba(0,0,0,.25), 0 6px 14px -8px rgba(0,0,0,.6)`): the teal install button only; removed on the secondary variant.
- **Floating panel** (`box-shadow: 0 6px 12px -6px rgba(0,0,0,.7), 0 1px 3px rgba(0,0,0,.5)`): the provenance tooltip.
- **Dropdown** (`box-shadow: inset 0 1px 0 rgba(255,255,255,.035), 0 6px 18px rgba(0,0,0,.5)`): the app's champion search list.
- **Focus ring** (site: `outline: 2px solid teal-ring` with a 2px offset, so a component's own shadow can never hide it; app: `0 0 0 2px teal-deep` box-shadow): focus-visible only, never on hover.

### Named Rules
**The Three-Steps Rule.** Depth is ground, panel, raised and nothing beyond; a fourth surface tone is a mistake. Anything that must float uses one of the three listed shadows.

**The Hover-Only Motion Rule.** Transitions run 110ms on `cubic-bezier(.2,.7,.3,1)` for background, border, colour and box-shadow, on hover and focus only. Nothing animates on load or scroll; the tooltip's 3px rise is the one entrance and it is a response to the pointer.

## Shapes

Three radii, chosen by what the thing is: assets and tags are 3px, controls and panels-that-act-like-controls are 5px, framed panels and screenshots are 7px, and the app window is 10px. Rails are 2px wide with a 1px radius. Dots (status, pipeline stations, list counters) are fully round. Borders are 1px and neutral: hairline inside, line outside. The only offset edge is the 2px bottom border on `kbd` keys. Underlines on links are 1px, offset 0.16em, tinted teal-deep at rest and teal on hover.

## Components

### Buttons
Quiet and rectangular; the fill announces the one action, everything else is an outline or a raised neutral.
- **Shape:** control radius (5px), 1px border matching the fill.
- **Primary (site install):** teal fill, teal-on ink, 600 at 15px, three-column grid (icon, label stack, size) with `11px 14px 11px 12px` padding; file name and size in mono under and beside the label at 11–12px, 78–85% opacity. Carries the button-lift shadow.
- **Hover / Focus:** fill goes to teal-hi at 110ms; active nudges 1px down; focus draws the ring.
- **Secondary (`.install.other`, and the second `.install` in a group by default):** the visitor's other platform. Raised fill, title ink, line border, no shadow; hover steps to edge-soft with an edge-strong border. The stylesheet demotes the second button on its own; the script promotes the visitor's platform to the front, so with no script or an unknown platform the pair still reads "mac, then Windows" with one teal. Phones get no promotion and a copy-link line instead.
- **Ghost (app `.btn`):** transparent, 1px line border, 28px tall, 12px/500; hover fills raised. Disabled is raised with a hairline border and slate-dot text, including when the button is primary.

### Tags (provenance)
- **Style:** 11px/600, 0.03em, raised fill, hairline border, 3px radius, `3px 6px` padding; `cursor: help`. Measured tint is blue text on blue-dim; argued tint is amber text on amber-dim.
- **State:** the tag is a button. Hover, focus-visible or `aria-expanded="true"` reveals a tooltip sibling above it; an expanded tag wears the focus ring. Once the page is one column (≤860px) the definition opens inline under the sentence instead of floating. In the app the same class is a neutral 10px/500 descriptive chip beside an item name, with no coloured variants.

### Tooltip (`.def`)
Anchored bottom-left above its tag, 8px clear, raised fill, line border, control radius, 13px/1.4 body ink, floating-panel shadow, max 320px or the viewport minus the gutter. Uses `display`, not `visibility`, so a hidden definition cannot widen the page. The bold lead word takes the tag's colour.

### Change row (`.change`)
The site's unit of content: a `2px 1fr` grid with 14px gap. The rail is edge-soft by default and paints blue for `.measured` or amber for `.argued`; the lead sentence is bold title ink (amber when argued); the reason follows as 14px muted text. The app's `.block` and `.sug` are the same idea at 13px, where only the amber rail paints.

### Sample row (`.row-sample`)
One row of the app reproduced as text: panel fill, hairline border, control radius, `12px 14px`, an uppercase label left and a mono `47.6% · 1,893 games` right. The `.rule` variant switches to the rail grid, paints amber, and shows an item name in amber with a plain-prose reason and no percentage.

### Legend
Two panel cards in a 14px grid, each with a rail down the left inside the border (rail radius on the outer corners only) and a 15px title in its hue; body 14px muted.

### Facts table
Full-width to 66ch, collapsed borders, 15px; row headers are 500 muted at 38% width, values are title ink, mono values at 14px, a 13px faint note under a value. Rows divide with hairlines; the last row has none.

### Pipeline (`.pipe`)
Four equal columns with a 1px vertical line between stations starting 26px down, not four cards. Each station is a 13px/600 title with a 6px slate dot, then 13px muted description. Two columns at 860px and the wires drop.

### Index rail (navigation)
Sticky at 24px. An 11px uppercase faint heading, then 14px muted links at `6px 10px` outdented 10px with a transparent 2px left border; hover goes title-ink on panel fill; the current section is 600 title ink with a teal left border, set by an intersection observer. An "Earlier" list of mono versions and dates sits beneath a hairline. On narrow screens the list becomes a horizontal chip row with the teal marker moving to the bottom edge.

### Inputs (app)
Raised fill, line border, title ink, 34px tall, 15px, control radius; focus swaps the border to teal-ring with a 2px teal-deep ring. Placeholder is faint. The compact variant is 28px at 13px.

### Tiles and status (app)
32px square item and portrait tiles at asset radius on raised fill with hairline borders; the keystone rune tile is 44px on teal-well with a teal-ring border; a rule tile takes an amber border, never an amber fill. Status is a 6px dot plus a word, never a filled capsule: teal live, amber preview, slate-dot offline.

### Closing panel (`.again`)
Panel fill, line border, panel radius, `22px 24px`; a 28px/700 mono version with a 12px faint sub-line on the left and the install pair on the right.

## Do's and Don'ts

### Do:
- **Do** copy the token block verbatim between `src/styles.css` and `site/site.css` when a value changes; neither file imports the other and the values must stay identical except for the three recorded document adjustments (`faint-doc`, JetBrains Mono, 16px body).
- **Do** set every number, version, path and file name in mono with `font-variant-numeric: tabular-nums`, and right-align columns of numbers so they read down.
- **Do** carry provenance as a 2px rail plus a tag; leave the rail column in the grid even when it does not paint.
- **Do** keep one teal-filled control per view and demote its sibling to the raised neutral.
- **Do** put the sample count beside every percentage; a percentage with no sample is not blue.
- **Do** keep hierarchy in type and spacing: hairline dividers, three surface steps, 44px between sections on the site and 24px between blocks in the app.
- **Do** use the 110ms ease-out for hover and focus only, and zero it under `prefers-reduced-motion`.
- **Do** treat "the client is not running" as a first-class state with faint text and a slate dot, never as an error colour.

### Don't:
- **Don't** fill a surface with blue or amber; the dim tints exist for the tag chip and nothing else.
- **Don't** apply amber to a statistic or blue to a rule, and don't give a source's matchup reading either rail.
- **Don't** use gradients, glass, glow, coloured shadows or a light theme.
- **Don't** nest a card inside a rail inside a card; a suggestion is a row.
- **Don't** set prose in mono or a number in Inter.
- **Don't** add a fourth surface tone, a fourth radius on the site, or a shadow beyond the three listed.
- **Don't** drop the app's quietest text below `faint-app` or the site's below `faint-doc`; both are the contrast floor for their base size.
- **Don't** render status as a filled capsule or a button as a teal fill when it cannot act; a disabled primary is raised neutral with slate text.
