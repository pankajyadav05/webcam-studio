---
name: Webcam Studio (landing page)
description: The product page at webcam.codifyit.dev, in Samsung.com product-page grammar. Scope is site/ only.
colors:
  ink: "#000000"
  paper: "#ffffff"
  mist: "#f4f4f4"
  line: "#dddddd"
  text-2: "#4d4d4d"
  text-2-on-dark: "#b3b3b3"
  hover-dark: "#333333"
  hover-light: "#e3e3e3"
  device-graphite: "#1f1f1f"
  accent: "#e7783f"
  live: "#2fbf71"
typography:
  display:
    fontFamily: "Urbanist, Helvetica Neue, sans-serif"
    fontSize: "clamp(3rem, 6vw, 5.25rem)"
    fontWeight: 800
    lineHeight: 0.98
    letterSpacing: "-0.03em"
  headline:
    fontFamily: "Urbanist, Helvetica Neue, sans-serif"
    fontSize: "clamp(2.25rem, 5vw, 4rem)"
    fontWeight: 800
    lineHeight: 1.04
    letterSpacing: "-0.025em"
  title:
    fontFamily: "Urbanist, Helvetica Neue, sans-serif"
    fontSize: "clamp(1.6rem, 2.4vw, 2.2rem)"
    fontWeight: 800
    lineHeight: 1.1
    letterSpacing: "-0.02em"
  lede:
    fontFamily: "Figtree, Helvetica Neue, sans-serif"
    fontSize: "clamp(17px, 1.4vw, 19px)"
    fontWeight: 400
    lineHeight: 1.55
  body:
    fontFamily: "Figtree, Helvetica Neue, sans-serif"
    fontSize: "17px"
    fontWeight: 400
    lineHeight: 1.55
  label:
    fontFamily: "Figtree, Helvetica Neue, sans-serif"
    fontSize: "16px"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "0.01em"
  caption:
    fontFamily: "Figtree, Helvetica Neue, sans-serif"
    fontSize: "13px"
    fontWeight: 600
    lineHeight: 1
rounded:
  pill: "999px"
  xl: "28px"
  lg: "20px"
  md: "14px"
spacing:
  gutter: "clamp(20px, 4vw, 48px)"
  section: "clamp(80px, 11vw, 144px)"
  section-head-gap: "clamp(40px, 5vw, 64px)"
  grid-gap: "20px"
  panel-pad: "clamp(28px, 3.4vw, 48px)"
  wide: "1280px"
components:
  button-dark:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    typography: "{typography.label}"
    rounded: "{rounded.pill}"
    padding: "0 28px"
    height: "48px"
  button-dark-hover:
    backgroundColor: "{colors.hover-dark}"
  button-light:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    typography: "{typography.label}"
    rounded: "{rounded.pill}"
    padding: "0 28px"
    height: "48px"
  button-light-hover:
    backgroundColor: "{colors.hover-light}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.paper}"
    rounded: "{rounded.pill}"
    padding: "0 18px"
    height: "36px"
  button-small:
    rounded: "{rounded.pill}"
    padding: "0 18px"
    height: "36px"
  chip:
    rounded: "{rounded.pill}"
    size: "36px"
  panel:
    backgroundColor: "{colors.mist}"
    rounded: "{rounded.xl}"
    padding: "{spacing.panel-pad}"
  media-frame:
    backgroundColor: "{colors.ink}"
    rounded: "{rounded.xl}"
  overlay-tag:
    backgroundColor: "rgba(0,0,0,0.55)"
    textColor: "{colors.paper}"
    typography: "{typography.caption}"
    rounded: "{rounded.pill}"
    padding: "8px 14px"
  product-bar:
    backgroundColor: "rgba(255,255,255,0.86)"
    textColor: "{colors.text-2}"
    height: "56px"
---

# Design System: Webcam Studio (landing page)

> **Scope.** This file describes the landing page at `webcam.codifyit.dev`, which is the `site/` folder (`site/index.html`, `site/site.css`, `site/site.js`, `site/img/`, `site/fonts/`). It does not describe the desktop app UI in `apps/desktop/ui`. That app has its own visual system. Do not apply these tokens there, and do not apply the app's tokens here.
>
> **Two owner commitments bind this page.** (1) The visual grammar is the Samsung.com product page, by the owner's explicit brief. (2) All site copy is written in ASD-STE100 Simplified Technical English at the `ste:85` level: short sentences, one instruction per sentence, approved verbs, active voice, no idioms. New copy on the page must follow the same standard.

## Overview

**Creative North Star: "The Product Page Stage"**

The page is a consumer-electronics product page, built in the Samsung.com grammar: a black hero with a very large, heavy, centred headline; a wide 16:9 media stage directly under it; then a long scroll of full-bleed bands that alternate black, white and light mist. Each band has one centred headline, one short lede, and one large rounded demonstration. The product is shown on real photos with the app's own backgrounds, not described in a grid of feature cards.

Density is low and the scale is big. Headlines carry the page; body text is quiet grey under them. Interaction is part of the proof: the hero chips change the background on the stage, a before-and-after line compares blur, zoom and mirror controls move the framing photo, and a menu-bar switch changes the camera status. Everything else is still. Colour lives almost only in the photographs and the six preset backgrounds; the chrome is black, white and grey.

**Key Characteristics:**
- Full-bleed bands in three tones (ink, paper, mist), each with a centred head and one demonstration.
- Urbanist 800 display at very large sizes with tight negative tracking; Figtree for everything else.
- Big rounded media frames (28px) and fully round pills and chips.
- Black and white pill buttons; the light pill on dark bands, the dark pill on light bands.
- Real interactive demos instead of icons and feature cards.
- Small numbered footnotes for every simulated image and for trademark notes.

## Colors

A monochrome product-page palette (black, white, a light mist grey) with colour carried by photography and the app's own preset backgrounds, plus two small functional hues.

### Primary
- **Ink Black** (ink): The hero, the dark bands (Framing, Privacy), the closer, the dark pill, the slider fill, the tick discs and the active nav underline. Black is the page's primary colour, not a neutral.

### Secondary
- **Mark Orange** (accent): The orange disc in the Webcam Studio mark. It does not appear as a fill on buttons, panels or headlines. The text-selection fill also uses it. Keyboard focus uses a darker Focus Orange (`--focus: #c25a1f`), which keeps at least 3:1 on white, mist and black.
- **Live Green** (live): Only the "camera on" signals: the live dot on the stage tag, the status dot in the menu-bar panel, and the switch track when on.

### Neutral
- **Paper White** (paper): Default page background, the light pill, the slider card and the popover modules.
- **Mist** (mist): Alternate band background (Menu bar, Specs, Notes) and the bento panel fill on white bands.
- **Rule Grey** (line): Hairline rules between specs rows, FAQ items, step columns and the footer top.
- **Secondary Text** (text-2): Ledes, body copy under headlines, notes and inactive nav links on light surfaces.
- **Secondary Text on Dark** (text-2-on-dark): The same role on ink bands.
- **Hover Greys** (hover-dark, hover-light): Pill hover fills only.
- **Device Graphite** (device-graphite): The simulated meeting window surface. It belongs to the depicted device, not to page chrome.

The six preset backgrounds (Studio, Paper, Sage, Dusk, Sand, Slate) are content, not UI colour. Their values are copied from the app (`apps/desktop/ui/lib/backgrounds.js`) and live in `site/site.js` and inline chip styles. They must stay in step with the app.

### Named Rules
**The Monochrome Chrome Rule.** Page chrome is ink, paper, mist and greys. Colour on the page comes from photos and preset backgrounds. If a new element needs a hue to be noticed, give it scale or position instead.

**The Two Signals Rule.** Orange means the brand mark. Green means "the camera is on". Do not use either for decoration, headings or buttons.

## Typography

**Display Font:** Urbanist 700 / 800 (with Helvetica Neue, sans-serif)
**Body Font:** Figtree 400 / 500 / 600 (with Helvetica Neue, sans-serif); pills use the 700 weight
**Label/Mono Font:** none; Figtree with `tabular-nums` for the zoom value

**Character:** A heavy, round geometric display face at Samsung Sharp scale over a friendly, neutral grotesque. The contrast is all weight and size, not style.

All fonts are self-hosted WOFF2 in `site/fonts/` with `font-display: swap`; Urbanist 800 and Figtree 400 are preloaded.

### Hierarchy
- **Display** (800, clamp 3rem to 5.25rem, line-height 0.98, tracking -0.03em): The hero headline and, at clamp 2.5rem to 5rem with line-height 1, the closer headline. One per band at most.
- **Headline** (800, clamp 2.25rem to 4rem, line-height 1.04, tracking -0.025em): Section heads. The split layout (Menu bar) and the statement use a smaller step (clamp 2rem to 3.6rem).
- **Title** (800, clamp 1.6rem to 2.2rem, line-height 1.1, tracking -0.02em): Bento panel heads. Step titles (1.5rem) and privacy facts (clamp 1.5rem to 2rem) sit on the same step.
- **Numeral** (800, clamp 3.5rem to 5rem, line-height 1, tracking -0.03em): The step counters in "Ready in three steps".
- **Lede** (400, clamp 17px to 19px; hero lede clamp 18px to 21px): The one paragraph under each headline, in secondary text, max 46ch to 56ch, centred.
- **Body** (400, 17px, line-height 1.55; 16px under 600px viewports): FAQ answers (max 68ch), specs values, panel copy (max 42ch).
- **FAQ question** (Urbanist 700, clamp 18px to 21px, line-height 1.35).
- **Label** (Figtree 700, 16px, tracking 0.01em; 14px on small pills): Pill buttons.
- **Caption** (Figtree 600, 13px): Overlay tags on media, tile names, notes and footnotes (13px, 400).

### Named Rules
**The Weight Carries It Rule.** Display headings are always weight 800 with negative tracking and `text-wrap: balance`. Never set a heading in the body face, and never set body copy in Urbanist.

**The One Lede Rule.** Each section head is one headline plus at most one short paragraph. There is no label above the headline.

## Layout

The page is a stack of full-bleed bands. Each band pads with the section rhythm (clamp 80px to 144px vertical) and the gutter (clamp 20px to 48px horizontal). Content inside a band caps at 1280px (`wide`); section heads cap at 820px and centre; reading blocks cap narrower (FAQ 880px, specs 1040px, statement 1100px, hero stage 900px).

Band order alternates tone so that no two adjacent bands share a background where it can be helped: ink hero, white statement and effects, ink framing, white meetings, mist menu bar, ink privacy, white setup, mist specs, white FAQ, ink closer, mist notes. A "tight" band (top padding 0) continues the band above it.

Grids used: bento 1.35fr / 1fr with a 20px gap; split 1fr / 1fr with clamp 40px to 96px gap; three-column facts and steps separated by hairline rules, not boxes; two-column specs as label / value rows. Media is always 16:9.

Responsive: at 900px and below, all multi-column grids collapse to one column, the nav links hide (the Download pill stays), step rules turn from vertical to horizontal, and the hero chips move from an overlay bar on the stage to a centred row under it. At 600px and below, the 28px radius steps down to 20px on large frames, body drops to 16px, and overlay tags and call tiles shrink.

The product bar is sticky, 56px high; anchor scrolling offsets by the bar height plus 16px.

## Elevation & Depth

The page is flat. Depth comes from tonal bands (ink, paper, mist) and from photographs, not from shadows on page surfaces. Shadows appear only on objects that imitate a real device control, where the shadow belongs to that object.

### Shadow Vocabulary
- **Floating control card** (`box-shadow: 0 18px 40px -16px rgba(0,0,0,0.35)`): The slider card that overlaps the "Tune the edge" image.
- **macOS panel** (`box-shadow: 0 0 0 0.5px rgba(0,0,0,0.25), inset 0 0 0 0.5px rgba(255,255,255,0.6), 0 22px 50px -10px rgba(0,0,0,0.5)`): The menu bar panel inside the Mac screen scene. The scene is a 28px-corner "screen" with the app's Dusk preset as wallpaper, a translucent 34px menu bar (Finder menus, the green camera-in-use light, the pressed Webcam Studio icon, status icons, clock), and the app's real panel (system font, round orange tile, orange switch, "Quit Webcam Studio" below a rule) placed by script directly under the icon. The switch hides the green camera light.
- **Knob** (`box-shadow: 0 1px 4px rgba(0,0,0,0.3), 0 0 0 0.5px rgba(0,0,0,0.1)`, and `0 6px 24px rgba(0,0,0,0.25)` for the large compare handle): Slider thumbs, switch thumbs and the before-and-after handle.

Translucent glass is used in two places only: the sticky product bar (white at 86% with `saturate(180%) blur(18px)`) and the dark overlay tags on media (black at 55% with `blur(10px)`).

### Named Rules
**The Flat Band Rule.** Bands, panels and media frames never carry a shadow. Only depicted controls (cards, popovers, knobs) do.

## Shapes

Two shape families. Containers are large rounded rectangles: 28px on hero stage, compare frame, framing view, meeting window and bento panels; 20px on inner stages, the slider card, and on all large frames under 600px. Depicted device pieces use smaller radii that match macOS (16px menu bar panel, 14px call tiles, 12px panel module, round panel tile). Controls are fully round: pills (999px), chips and round icon buttons are circles, the switch is a capsule, overlay tags are capsules.

Rules are 1px hairlines (`line` on light, white at 22% on dark). Icons are inline SVG line drawings, stroke 1.6 to 2, round caps and joins, drawn in `currentColor`. Link arrows are drawn chevrons from CSS borders, not glyphs.

## Components

### Buttons
- **Shape:** Fully round pill (999px), 48px high, 28px side padding; the small pill is 36px high, 18px padding, 14px text.
- **Dark pill:** Ink fill, paper text. Used on light bands and in the product bar ("Download").
- **Light pill:** Paper fill, ink text. Used on ink bands (hero, closer: "Download for Mac").
- **Ghost pill:** Transparent with a 1.5px white border at 45%, white text; for toggles on ink bands (Mirror). Pressed state fills white with ink text.
- **Hover / Active:** Fill shifts to the hover grey (0.25s); border goes full white on ghost; press scales to 0.97 on the page ease.
- **Round icon button:** 44px circle, ghost border, white stroke icon; disabled at 35% opacity.

### Text link
- Figtree 600, underlined at 1.5px with a 5px offset, followed by a drawn chevron. Hover thickens the underline to 2.5px. Used for the one secondary action beside a primary pill and for inline "Read the code".

### Chips
- **Style:** 36px circles (32px in the desktop hero bar and on phones) filled with the preset's two-stop diagonal gradient, or with a crop of the hero photo for Original and Blur. A faint 1px inner white ring.
- **State:** Works as a radio group (arrow keys move the selection). Selected chip shows a 2px white ring 6px outside the disc. Hover scales to 1.08.
- **Hero chip bar (over 900px):** A black capsule centred on the bottom of the stage, with "Background: <name>" on the left and the chips on the right.

### Cards / Containers
- **Panel:** Mist fill, 28px corners, padding clamp 28px to 48px, a 32px internal gap between copy and its demo image. No border, no shadow.
- **Media frame:** 16:9, 28px corners, clipped, dark placeholder fill. Holds a photo or a layered composite (room, blur, preset, cutout person).
- **Overlay tag:** Black 55% glass capsule, white 13px semibold text, 20px from the frame edge. Used for "Webcam Studio Camera", "Without" / "With Webcam Studio", and "Images simulated" with its footnote number.

### Navigation
- **Product bar:** Sticky, 56px, translucent white glass with a faint bottom hairline. Brand (mark plus Urbanist 800 19px) on the left; section links (Figtree 500 15px, secondary text) on the right; small dark Download pill last. The current section turns ink with a 2px ink underline at the bar's bottom edge. Under 900px only the brand and the pill remain.
- **Footer:** Hairline top, brand, plain text links (underline on hover), licence line right-aligned.

### Before-and-after compare (signature)
A 16:9 frame with the original photo below and the effect clipped from a CSS position variable. A 2px white line and a 52px white round handle with drawn chevrons mark the split. A transparent full-size range input drives it, so mouse, touch and keyboard all work; focus shows on the handle.

### Lists of facts
- **Ticks:** 22px ink discs with a white check, beside 500-weight text.
- **Facts, steps, specs:** Columns or rows divided by hairline rules, never boxed. Steps carry a large Urbanist numeral above each title.
- **FAQ:** Native disclosure rows between hairlines; a drawn plus that turns to minus on open.

### Focus
- Every interactive element shows a 2px outline with a 3px offset and a 6px radius (capsule on the switch, on the handle for the compare). The colour is Focus Orange `#c25a1f`: 4.4:1 on paper, 4.0:1 on mist, 4.8:1 on ink, so one colour passes the 3:1 non-text minimum on every band.

### Footnotes
Every simulated image carries a superscript number that links to a numbered note in the mist Notes band before the footer. The targeted note turns ink.

### Motion
One entrance moment only, and only without reduced motion: the hero stage opens from 90% scale with an inset rounded clip and dimmed brightness (1.4s), and the hero copy rises 18px in a short stagger (0.9s, 80ms steps). The ease is `cubic-bezier(0.16, 1, 0.3, 1)` throughout. State changes (background swap, zoom, chip ring, switch) use 0.25s to 0.7s on the same ease. Content is visible by default; the entrance runs only when JavaScript adds the `js` class. Under reduced motion, transitions drop to near zero and the preset cycle in the "Put a new room behind you" panel does not run.

## Do's and Don'ts

### Do:
- **Do** keep each band to one centred headline, at most one short lede, and one large demonstration.
- **Do** prove features on the real photos with the app's own preset backgrounds, and footnote every simulated image.
- **Do** use the light pill on ink bands and the dark pill on light bands; one primary pill per band, with at most one text link beside it.
- **Do** use 28px corners on large containers (20px under 600px) and fully round shapes on all controls.
- **Do** keep the preset colours identical to `apps/desktop/ui/lib/backgrounds.js`.
- **Do** write all copy in ASD-STE100 (`ste:85`): short sentences, approved words, active voice.

### Don't:
- **Don't** put a grid of icon feature cards under the hero; the demos replace it.
- **Don't** add shadows to bands, panels or media frames (The Flat Band Rule).
- **Don't** use orange or green as decoration, on buttons, or in headlines (The Two Signals Rule).
- **Don't** put a small label or eyebrow line above a headline.
- **Don't** claim features that PRODUCT.md lists under "Do not claim", and don't add testimonials, counts or prices.
- **Don't** import the desktop app's tokens, components or styles into `site/`.
