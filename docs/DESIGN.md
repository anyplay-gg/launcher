# AnyPlay Launcher: UI design specification

Source of truth for the UI of the Windows port. It specifies the macOS launcher of
[utkuhalis/AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM) (GPL-2.0, commit `cb20fd80c9f7a044c62d51da3cdd0475100b5554`,
`tools/macos/launcher/Sources/AnyPS5`) so that it can be rebuilt pixel-close without reading the Swift source.

**Target of this port: native Rust, eframe/egui on wgpu** (workspace pins `eframe`/`egui`/`egui-wgpu` 0.37, `gilrs`, `rodio`, `notify`).
Everything below is expressed for that target: sRGB colour values, spring solvers, cubic-bezier easings, WGSL shader math,
GPU blur recipes and bundleable free fonts. The original SwiftUI values are always quoted next to the port values so that
nothing is lost in translation.

Conventions used in this document:

- **dp** = "design pixel". The original lays everything out for a **1600 x 900** window and multiplies every length by one
  global `scale` (section 1.1). All lengths below are dp at `scale = 1` unless stated otherwise.
- Colours are written `white 0.12` = sRGB white with alpha 0.12, or hex with alpha. Alpha is straight (not premultiplied).
- "Original" = the Swift source. File names without a path refer to `tools/macos/launcher/Sources/AnyPS5/`.
- Things that could not be verified without running the macOS app (it was not run) are marked **(estimate)**.

Screenshot references point at the ARM repository at the pinned commit (permalinks, nothing is copied into this repository;
the screenshots show third-party game artwork that is not ours to redistribute). Base URL:
`https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/`

| Short name | File |
|---|---|
| S-home | [launcher-home.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-home.jpg) |
| S-home-trophies | [launcher-home-trophies.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-home-trophies.jpg) |
| S-unlocking | [launcher-unlocking.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-unlocking.jpg) |
| S-starting | [launcher-starting.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-starting.jpg) |
| S-game-settings | [launcher-game-settings.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-game-settings.jpg) |
| S-controls | [launcher-controls.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-controls.jpg) |
| S-advanced | [launcher-advanced.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-advanced.jpg) |
| S-library | [launcher-library.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-library.jpg) |
| S-settings | [launcher-settings.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-settings.jpg) |
| S-themes | [launcher-themes.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-themes.jpg) (onboarding step 2) |
| S-trophies | [launcher-trophies.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-trophies.jpg) |
| S-banner | [launcher-trophy-banner.jpg](https://github.com/utkuhalis/AnyPS5-ARM/blob/cb20fd80c9f7a044c62d51da3cdd0475100b5554/docs/screenshots/launcher-trophy-banner.jpg) |

The other files in `docs/screenshots/` (`breakout`, `converter-*`, `dreaming-sarah-*`, `stray-*`) show games and the separate
converter app, not the launcher. There is also a video, `docs/videos/launcher.mp4` (1:39), useful for motion timing.
No screenshot exists for onboarding steps 0 and 2, the empty library, the log view, a toast, the confirm-uninstall panel,
settings categories other than Appearance, or the Windows-irrelevant move-to-Applications banner. Those are specified from code only.

Contents: 1 Tokens, 2 Motion, 3 Backgrounds (shaders), 4 Materials and blur on the GPU, 5 Screens and components, 6 Input,
7 Sounds and media, 8 Localization, 9 Deviations, 10 Assets and licences, 11 Implementation notes for egui,
18 Phone as controller, 19 Mac style.

---

## 1. Design tokens

The app is **dark only** (`preferredColorScheme(.dark)`, window background black). There are no light variants. The three
themes change the animated background, the default accent colour and nothing else: all UI chrome is white/black with alpha.

### 1.1 Scale and window

| Item | Value |
|---|---|
| Reference size | 1600 x 900 dp |
| `scale` | `clamp(min(w / 1600, h / 900), 0.68, 1.45)` where w, h = window content size |
| Minimum window | 1100 x 650 |
| Initial window | maximised (fills the screen's visible frame; not exclusive fullscreen); fullscreen allowed |
| Window background | `#000000` |
| Title bar (original) | hidden, transparent, window draggable by its background (Windows: see section 9) |

In egui: set `pixels_per_point = os_pixels_per_point * scale` every frame (or when the window resizes) so that 1 egui point = 1 dp.
Compute `scale` from the window size in OS-scaled points *before* applying it. Every number in this document is then used as is.
The trophy banner is **not** scaled (section 5.14).

### 1.2 Colours

#### Themes (background palettes)

Float triples are the Metal uniforms (`Theme.palette`: deep, middle, bright). They are written to a UNorm target without gamma
conversion, so they behave as display-referred (sRGB-encoded) values; hex = value x 255, rounded.

| Theme | deep | middle | bright | default accent |
|---|---|---|---|---|
| Cosmic (index 0) | (0.012, 0.024, 0.090) `#030617` | (0.040, 0.150, 0.420) `#0A266B` | (0.420, 0.640, 1.000) `#6BA3FF` | Blue |
| Aurora (index 1) | (0.006, 0.030, 0.045) `#02080B` | (0.020, 0.300, 0.300) `#054D4D` | (0.560, 1.000, 0.780) `#8FFFC7` | Mint |
| Sunset (index 2) | (0.060, 0.010, 0.040) `#0F030A` | (0.520, 0.110, 0.220) `#851C38` | (1.000, 0.640, 0.330) `#FFA354` | Coral |

Theme titles: Cosmic / Aurora / Sunset (TR: Kozmik / Aurora / Gün Batımı). `Theme.gradient` (middle to deep, top to bottom) exists
in the source but is not used anywhere; it is the documented fallback if the GPU shader is unavailable (use it for that).

#### Accent colours (user setting, default per theme)

| Name (EN / TR) | float | hex |
|---|---|---|
| Blue / Mavi | (0.40, 0.64, 1.00) | `#66A3FF` (was `#4F94FF`, 3.0:1 short on Sunset's lit band; 3.10) |
| Mint / Nane | (0.36, 0.92, 0.74) | `#5CEBBD` |
| Coral / Mercan | (1.00, 0.50, 0.40) | `#FF8066` |
| Violet / Mor | (0.70, 0.57, 1.00) | `#B391FF` (was `#A885FF`; 3.10) |
| Gold / Altın | (1.00, 0.80, 0.36) | `#FFCC5C` |
| White / Beyaz | white 0.94 | `#F0F0F0` |

The accent is used for: progress bars (fill and its glow), the running-state dot, "installing/starting/running" state labels,
the avatar gradient, the switch-on track, the splash glow, trophy-progress bars, the scan-result icon, the theme-card dot (that
theme's default accent).

#### UI colours (all sRGB, straight alpha)

*The text, surface and hairline values in this table are the first round; 3.10 and 3.12 hold the values in use.*

| Role | Value |
|---|---|
| Primary text / icons | white 1.0 (title text), white 0.9 (secondary labels), white 0.85 (values), white 0.72 (hints) |
| Secondary text | white 0.7 (subtitles), white 0.65 (empty text), white 0.6 (detail lines), white 0.55 (overlines, row detail on dark), white 0.5 (counts), white 0.45 (path), white 0.4 (inactive date) |
| Text on a focused (white) surface | `#000000`; its detail text black 0.55 |
| Surface, resting | white 0.10 (buttons, icon buttons, language cards), white 0.07 (settings rows, trophy-icon tile), white 0.05 (trophy row), white 0.08 (game tile placeholder) |
| Surface, prominent button | white 0.20 |
| Surface, hover (button) | white 0.16 |
| Surface, focused | `#FFFFFF` (solid); trophy row instead white 0.13 + ring |
| Hairline | white 0.12 (buttons), white 0.14 (language cards), white 0.09 (glass), white 0.08 (side panel edge, card gradient end) |
| Track (progress/indeterminate) | white 0.16 |
| Keycap fill (hint) | white 0.16 |
| Full-screen panel | black 0.74 over blurred backdrop (settings, library, trophies, and the panels of a game); black 0.65 for the log |
| Side panel (first round, gone in 0.2.8: no drawers) | `Color(white: 0.06)` at 0.92 over blurred backdrop, 1 px left edge white 0.08. SwiftUI's `Color(white:)` is a **gray level** (rgb 15,15,15), not white with alpha; the same holds for the next row |
| Glass panel (cards, toast, log boxes) | blurred backdrop + black `opacity` (default 0.38; 0.5 diagnosis box; 0.55 log box; 0.6 toast and banner) + 1 px inner stroke white 0.09 |
| Splash | blurred backdrop + black 0.55 |
| Destructive | text/icon `(1.00, 0.60, 0.55)` = `#FF998C` for titles, `(1.00, 0.55, 0.50)` = `#FF8C80` for icons and the button label |
| Error text | `(1.00, 0.70, 0.65)` = `#FFB3A6` (failure message), `(1.00, 0.60, 0.55)` = `#FF998C` (state label failed/unreadable) |
| Warning text | `(1.00, 0.78, 0.50)` = `#FFC780` (diagnosis), `(1.00, 0.82, 0.40)` = `#FFD166` (update available) |
| Toast/banner card fill | `Color(white: 0.07)` (gray, rgb 18,18,18) at 0.94 |
| Game artwork scrims | see 5.2 |
| Switch off track | white 0.18 (black 0.18 when its row is focused), knob `#FFFFFF`, knob shadow black 0.25 |

#### Trophy grades

| Grade | float | hex | points |
|---|---|---|---|
| Platinum | (0.74, 0.84, 0.97) | `#BDD6F7` | 300 |
| Gold | (0.97, 0.78, 0.34) | `#F7C757` | 90 |
| Silver | (0.80, 0.82, 0.86) | `#CCD1DB` | 30 |
| Bronze | (0.83, 0.55, 0.36) | `#D48C5C` | 15 |

Percent = `unlocked points * 100 / total points` (integer division, 0 when total is 0).

### 1.3 Radii

*3.12 reduces these to five values; the table is the first round.*

SwiftUI uses `.continuous` (superellipse "squircle") corners everywhere. egui only has circular corners; use them (difference
is invisible at these radii). An optional refinement is Figma "corner smoothing 60 %", which matches Apple's continuous curve.

| Element | Radius |
|---|---|
| Game tile | `size * 0.12` (176 -> 21.1, 118 -> 14.2, 196 -> 23.5, 200 splash -> 24, 84 -> 10.1) |
| Trophy icon | `size * 0.18` (112 -> 20.2, 68 -> 12.2, 60 -> 10.8) |
| Console button | 14 |
| Icon button (top bar) | circle (r = 22) |
| Info card, glass default | 16 |
| Settings row, category row, trophy secret tile | 12 |
| Trophy row | 16 |
| Language card | 20 |
| Theme card preview | 18 |
| Hint keycap | 5 |
| Key cap (controls) | 7 |
| Toast | 28 (pill-ish) |
| Banner card | 20 |
| Move banner (not ported) | 20 |
| Log boxes, diagnosis box | 14 |
| Capsules (progress, switch, onboarding dots) | fully round |

Focus ring (`focusRing(focused, radius, width = 3)`): stroke white 0.95, width 3, drawn **outside** the element: ring path radius
`radius + width`, outset by `width * 1.6` (4.8 for width 3, 3.2 for width 2), plus a glow of white 0.45 with SwiftUI shadow radius 10.
Trophy rows use width 2. When unfocused the ring is invisible (alpha animates, see section 2).

### 1.4 Shadows

SwiftUI `shadow(radius: r)` and CSS/egui blur are related by `egui blur = 2 * r` (SwiftUI radius is the Gaussian sigma-ish value,
CSS and `epaint::Shadow` blur is about twice that) **(estimate; verify against the screenshots)**. Values:

| Element | Colour | SwiftUI radius | y offset |
|---|---|---|---|
| Game tile (unfocused) | black 0.45 | 10 | 8 |
| Game tile (focused) | black 0.45 | 22 | 8 |
| Focus glow | white 0.45 | 10 | 0 |
| Console button focused | white 0.28 | 18 | 0 |
| Top-bar icon button focused | white 0.40 | 10 | 0 |
| Settings row focused | white 0.22 | 12 | 0 |
| Language card focused | white 0.30 | 20 | 0 |
| Hero title text | black 0.35 | 12 | 0 |
| Progress fill glow | accent 0.6 | 6 | 0 |
| Running dot glow | accent 1.0 | 6 | 0 |
| Splash icon glow | accent 0.2 .. 0.55 (breathing) | 18 .. 40 | 0 |
| Toast | black 0.40 | 20 | 8 |
| Banner card | black 0.50 | 18 | 8 (not scaled) |
| Switch knob | black 0.25 | 2 | 1 |

All radii and offsets are multiplied by `scale` unless stated (banner: not scaled).

### 1.5 Spacing and sizes (dp)

Screen paddings:

| Screen | Padding |
|---|---|
| Home top bar | horizontal 64, top 34 |
| Home tile row | top 34 below the top bar; leading 64 (92 from the second game on, see Tile row in 3.7) |
| Home hero info | horizontal 72 |
| Home hint bar | horizontal 64, vertical 28 |
| Onboarding | horizontal 110, vertical 80 |
| Settings | horizontal 90, top 70, bottom 50 |
| Library | horizontal 80, vertical 60 |
| Trophies | horizontal 80, vertical 56 |
| Log | 70 all around |
| Game panel frame | the full-screen area's margins (`pad::TROPHIES`), content column 720 |
| Toast stack | top 26 |

Components:

| Element | Size |
|---|---|
| Tile row small / large / gap / label column | 118 / 176 / 16 / 300 |
| Unselected tile y offset | `(176 - 118) * 0.18` = 10.44 |
| Library tile | 196, column gap 26, row gap 30, as many columns as fit the width (at least 3), grid padding 14 |
| Splash tile | 200 |
| Onboarding folder tiles | 84, gap 12, first 8 games |
| Console button | min height 54, horizontal padding 30 (18 when icon only), icon 17, gap 10, `Play` min width 210, onboarding buttons 220 / 180 |
| Top-bar icon button | 44 circle, glyph 18 |
| Avatar | 36 circle |
| Info card | 236 x 96, padding 18, row gap 14 |
| Hero progress block | width 340, leading pad 10 |
| Settings category column | width 330 + 36 trailing; row height 52, row gap 6 |
| Settings row | min height 58, horizontal padding 18, row gap 8, list padding 6 |
| Row icon / chevron box / chevron glyph | 24 box, glyph 17 / 26 box, glyph 13 / ">" glyph 14 |
| Choice value | min width 120, max 190 |
| Stepper value | min width 70 |
| Text field value | width 200 |
| Switch | 50 x 28, knob inset 3 |
| Key cap | min 30 x 28, horizontal padding 9, gap 6 |
| Trophy row | icon 68, gap 20, horizontal pad 18, vertical pad 14, row gap 10 |
| Trophy header icon | 112 |
| Trophy progress (header / row) | 260 x 4 / 180 x 4 |
| Language card | 280 x 150, padding 26, gap 20 |
| Theme card | 330 wide, preview 330 x 186, gap 26 |
| Onboarding dots | 5 high; 46 wide current, 18 others; gap 10 |
| Progress line | height 4 (tile: `max(3, size * 0.03)`) |
| Banner | window 560 x 150 (unscaled), card 480 wide, padding 14, icon 60 (glow circle 64), top offset 14 |

### 1.6 Typography

The original uses `Font.system` (SF Pro, with SF Pro Rounded for hint keycaps and SF Mono for the log) at fixed sizes times `scale`.

**Port font: Inter** (SIL OFL 1.1, free to bundle). Justification: it is the closest free match to SF Pro: same neo-grotesque
skeleton, nearly identical x-height and numeral shapes, full weight range 100-900 (the original uses 300 to 700), large Latin
Extended coverage including all Turkish letters (ı İ ş Ş ğ Ğ ç ö ü), and Inter 4.x ships a separate **Inter Display** cut for
large sizes, which mirrors SF Pro Text / SF Pro Display. The screenshots' typeface reads as SF Pro; Inter is the standard
free stand-in. Alternatives considered: Geist (too geometric), Roboto (different proportions), Noto Sans (wider).

Bundling in egui:

- egui renders **static** font instances. Bundle `Inter-Light`, `-Regular`, `-Medium`, `-SemiBold`, `-Bold` (TTF, from the official
  Inter release) and register each as its own `FontFamily::Name` ("inter-light" ... "inter-bold"). Use the **InterDisplay-\*** files
  for text at or above 20 dp (mirrors SF's text/display split at 20 pt) and the regular Inter files below.
- Recent egui exposes `TextFormat::coords: VariationCoords` (variable-font axes). If the pinned version supports it, one
  `InterVariable.ttf` with `wght` and `opsz` coordinates can replace the ten static files. **Decided: variable font.** egui 0.37 supports it through `FontTweak::coords`, so `InterVariable.ttf` (one 880 KB file) is
  registered ten times as named families (`inter-{light,regular,medium,semibold,bold}` at `opsz` 14 and `inter-display-*` at
  `opsz` 32). Sizes from 20 dp use the display families.
- Letter-spacing: SF Pro applies size-dependent tracking automatically. Inter Display and the Display files already carry the
  tighter spacing, so **no extra tracking** is applied except where the original sets it explicitly: uppercase overlines use
  `TextFormat::extra_letter_spacing` = 0.6 (info-card titles) or 0.8 (onboarding "LANGUAGE", trophies "TROPHIES"), in dp.
- Line height: SwiftUI uses the font's natural line height (about 1.19 em for SF Pro). Use `line_height = 1.2 * size` (Inter's
  natural height is 1.21 em). Multi-line text (splash hint, row titles, trophy detail) uses the same.
- **Tabular figures** (`monospacedDigit`: clock, percentages, counts, timer, stepper values): egui has no OpenType features. Lay
  digits out one by one in cells as wide as the widest digit of the font (Inter digit advance at 0.62 em is within 1 % across
  digits; measure `0`..`9` once and use the max), and the colon or percent sign with its natural advance.
- Monospaced (log, `12 dp`): **JetBrains Mono Regular** (OFL 1.1) as the SF Mono stand-in.
- Rounded (hint keycaps, 12 bold): Inter Bold is acceptable; no extra font is worth bundling for 4 small labels.
- Glyph coverage: Turkish and the arrows `← → ↑ ↓ ↩` are in Inter. The shape glyphs `✕ ○ △ □` (controller faces) should be **painted
  as vector shapes** (circle, triangle, square, cross strokes of 1.5 dp) instead of relying on font glyphs. The 18 game languages
  include Japanese, Korean, Chinese and Arabic names: load Windows' own fonts as egui fallbacks at runtime if present (Yu Gothic UI /
  Malgun Gothic / Microsoft YaHei UI / Segoe UI) instead of bundling large CJK fonts; trophy texts in game languages need the same.
- Icons: SF Symbols cannot be redistributed. Use **Phosphor Icons** (MIT; `egui-phosphor` crate or the font file), mapping in 5.16.

*The scale in use is in 3.12 (twelve sizes); the table below is the first round.*

Type scale (size dp / weight; weight numbers: light 300, regular 400, medium 500, semibold 600, bold 700):

| Use | Size / weight | Notes |
|---|---|---|
| Hero game title | 58 / light | max 2 lines, shrinks to 60 % to fit (`minimumScaleFactor 0.6`), shadow black 0.35 r12 |
| Onboarding title | 54 / light | |
| Settings and Library titles | 40 / light | log title: 30 / light |
| Empty-library title | 34 / light | |
| Splash title | 34 / light | centred, 2 lines |
| Trophy list title | 36 / light | 1 line |
| Side-panel title | 30 / light | 2 lines |
| Top bar "Games" | 24 / semibold | |
| Library count | 22 / medium | white 0.5 |
| Tile-row game name | 22 / semibold | 2 lines, beside the large tile |
| Theme card name | 22 / regular, semibold when focused | |
| Percent (trophy header) | 22 / semibold tabular | |
| Language card name | 28 / medium | |
| Clock | 20 / medium tabular | white 0.92 |
| Button label | 19 / semibold | glyph 17 semibold |
| Category row | 19 / regular, semibold when selected | |
| Onboarding subtitle | 19 / regular | white 0.7, max width 720 |
| Row title, trophy name, onboarding status | 17 / medium, 18 / semibold, 18 / medium | |
| Info-card value | 17 / semibold | |
| Profile name, toast | 16 / medium | |
| Choice / stepper value | 16 / semibold | |
| Diagnosis in log | 16 / medium | |
| Row info value | 15 / medium | |
| Progress label | 15 / medium, percent 15 / semibold | |
| Hint label | 13 / medium | keys 12 / bold rounded |
| Detail lines | 13..14 / regular | row detail 13, trophy detail 14, splash text 14, failure/diagnosis 14 |
| State label | 14 / medium | 12 in the library grid |
| Info-card overline | 12 / semibold, uppercase, tracking 0.6 | white 0.55 |
| Onboarding overline | 14 / semibold, uppercase, tracking 0.8 | |
| Trophies overline | 13 / semibold, uppercase, tracking 0.8 | |
| Key cap | 13.5 / semibold | |
| Log | 12 / mono | white 0.85 |
| Banner | label 13 semibold, name 19 semibold, detail 13 regular | unscaled |

---

## 2. Animation

### 2.1 Spring solver

SwiftUI `.spring(response: r, dampingFraction: z)` is a critically-parametrised damped spring with unit mass:

```
omega = 2 * pi / r          (natural frequency)
stiffness k = omega^2
damping   c = 2 * z * omega
x'' = -k * (x - target) - c * x'          (x is the animated scalar, starts at the old value with the old velocity)
```

Implement one small `Spring { value, velocity }` with `step(dt, target)` (sub-step at <= 1/240 s with semi-implicit Euler, or
use the closed form for under-damped motion
`x(t) = 1 - e^(-z*omega*t) * (cos(wd*t) + (z*omega/wd) * sin(wd*t))`, `wd = omega * sqrt(1 - z^2)`), keep the velocity when the
target changes (SwiftUI springs are interruptible and carry velocity), request repaint until `|x - target| < 0.001` and
`|v| < 0.01`. Every animated visual property (scale, ring alpha, fill mix, shadow alpha, x/y position, panel offset) is
`lerp(from, to, spring_value)`; colours lerp in sRGB and clamp to [0, 1].

| Name | Use | Response | Damping | omega | stiffness k | damping c | overshoot | 98 % settle |
|---|---|---|---|---|---|---|---|---|
| `Motion.focus` | focus/hover feedback, switch, key scroll | 0.32 | 0.82 | 19.63 | 385.5 | 32.2 | 1.1 % | 0.25 s |
| `Motion.layout` | tile row move, hero swap, onboarding steps, list reflow | 0.45 | 0.86 | 13.96 | 195.0 | 24.0 | 0.5 % | 0.33 s |
| `Motion.overlay` | opening/closing panels, splash, toast | 0.42 | 0.90 | 14.96 | 223.8 | 26.9 | 0.15 % | 0.30 s |
| `Motion.exit` (2026-10-25) | what leaves while something else arrives: the old hero | easeOut 0.18 s, the same when reduced | - | - | - | none | gone at 0.18 s |
| Banner slide | trophy card in | 0.55 | 0.78 | 11.42 | 130.5 | 17.8 | 2.0 % | 0.45 s |
| Banner pop | trophy icon scale | 0.45 | 0.55 | 13.96 | 195.0 | 15.4 | 12.6 % | 0.52 s, starts after 0.18 s delay |

Measured at 240 Hz (test `settle_times_match_the_design_table`), time to within 2 % of the target: focus 0.200 s, layout 0.308 s,
overlay 0.317 s; press 0.10 s and exit 0.12 s are eases and take their length. Overshoot only visibly matters for the banner pop; clamp alpha and colour lerps to [0, 1] but let scale and offset overshoot.

### 2.2 Easing curves

SwiftUI's named curves are the standard cubic Beziers; implement one `cubic_bezier(x1, y1, x2, y2, t)` solver (Newton on x).

| SwiftUI | Bezier |
|---|---|
| `easeIn` | (0.42, 0, 1, 1) |
| `easeOut` | (0, 0, 0.58, 1) |
| `easeInOut` | (0.42, 0, 0.58, 1) |

### 2.3 Reduced motion (setting "Animations: Reduced")

`Motion.reduced` swaps: focus -> easeOut 0.12 s; layout -> easeOut 0.15 s; overlay -> easeOut 0.15 s; fade 0.55 s easeInOut ->
0.15 s easeInOut; banner slide -> easeOut 0.25 s. Also (2026-10-25): a panel does not slide, a full-screen panel does not grow from 1.03 and the toast does not travel; each fades in
place (opacity only), because movement is what reduced motion removes and opacity is what it keeps. The edge bump (6.7) leans nothing.
Also: the theme background is paused (one still frame is drawn), artwork drift
and splash breathing are off, the user picture does not drift. **Not** affected: the indeterminate progress sweep, the "Press a
key..." pulse, the banner pop.

### 2.4 Every animation, with its port form

| # | What | Original spec | Port |
|---|---|---|---|
| 1 | Tile focus: scale/ring/shadow of the selected tile | `Motion.focus` on `focused` | spring focus; ring alpha 0 -> 0.95, glow 0 -> 0.45, shadow r10 -> 22 |
| 2 | Tile row move on selection change | `Motion.layout` on `selected` | per tile spring layout on x, y offset, size (see 5.3); the game before the selected one stays at 0.4 opacity, the ones before it fade to 0 as they leave left |
| 3 | Tile size 118 <-> 176 | `Motion.layout` on `size` | spring layout on a `size` scalar (animate size, not scale; text is outside the tile) |
| 4 | Hero info swap on game change | `.id(game.id)`, insertion opacity + y offset 14, removal opacity, `Motion.layout` | new hero: spring layout 0 -> 1 on opacity (no travel: the hero changes on every arrow press); old hero (2026-10-25): opacity 1 -> 0 on `Motion.exit` (easeOut 0.18 s), quicker than the arriving spring: it is under 0.3 when the new one is half in and the two together never dip below 0.6 of one title (test `the_hero_pair_never_dips_and_the_old_one_leaves_first`) |
| 5 | Tile-row name label | opacity transition, `Motion.layout` | opacity spring layout |
| 6 | Backdrop crossfade (theme change, selected-game artwork change) | `easeInOut 0.55` on `theme` and on `selectedGame.id` | keep the previous artwork texture, cross-fade alpha with easeInOut 0.55 |
| 7 | Artwork decode fade-in | `easeOut 0.35` | alpha 0 -> 1 easeOut 0.35 when the texture becomes ready |
| 8 | Game artwork drift (home) | scale 1.01 -> 1.07, anchor top-trailing, `easeInOut 24 s`, repeat forever, autoreverse | ping-pong of `t = easeInOut(phase)`, period 48 s (24 s each way); UV zoom about the top-right corner |
| 9 | User picture drift | scale 1.0 -> 1.08 and x offset +1.5 % of width -> -1.5 %, `easeInOut 28 s` autoreverse | same ping-pong, 28 s each way, translate in UV |
| 10 | Button focus | scale 1.05, fill, ring-less glow white 0.28 r18, `Motion.focus` | spring focus |
| 11 | Top-bar icon focus | scale 1.1, glow white 0.4 r10 | spring focus |
| 12 | Settings row focus | scale 1.015, glow white 0.22 r12 | spring focus |
| 13 | Language card focus | scale 1.04, glow white 0.3 r20 | spring focus |
| 14 | Theme card focus | scale 0.97 -> 1.04, opacity 0.72 -> 1 | spring focus |
| 15 | Switch | knob moves leading <-> trailing, track colour | spring focus on the knob x and the colour mix |
| 16 | Progress bar | fill width `easeOut 0.35` | width eased with easeOut 0.35 per change |
| 17 | Indeterminate sweep | a 35 %-wide gradient (clear, accent, clear, left to right) slides from x = -0.4 w to +1.05 w, `easeInOut 1.3 s`, repeat forever **no** autoreverse, clipped to the capsule | `phase = easeInOut(fract(t / 1.3))`, `x = lerp(-0.4, 1.05, phase) * w` |
| 17b | "Press a key..." | opacity 1 <-> 0.45, `easeInOut 0.7 s`, repeat autoreverse | triangle wave, easeInOut 0.7 each way |
| 18 | Onboarding step change | `Motion.layout`: old step opacity + x -40, new step opacity + x +40 | spring layout cross-slide; dots widths animate 18 <-> 46 with the same spring |
| 19 | Onboarding finished | `easeInOut 0.7` flips `onboarded` | cross-fade onboarding -> home over 0.7 s |
| 20 | Settings category change | `Motion.focus`; rows fade in with x offset 18 | spring focus; new list opacity + `offset_x = 18 * (1 - v)` |
| 21 | Settings panel in/out | opacity, `Motion.overlay` | spring overlay on opacity |
| 22 | Library and Trophy list in/out | opacity + scale 1.03 -> 1, `Motion.overlay` | spring overlay: opacity and scale about the centre |
| 23 | Panel in/out (the game panels, 5.9) | the full-screen area's enter and exit: opacity v (no slide from the side since 0.2.8) | spring overlay, as every full-screen area |
| 24 | Log in/out | opacity | spring overlay |
| 25 | Launch splash in/out | opacity + scale 1.04 -> 1, `Motion.overlay` | spring overlay |
| 26 | Splash breathing | icon scale 0.985 <-> 1.015, glow alpha 0.2 <-> 0.55 and radius 18 <-> 40, `easeInOut 1.6 s` repeat autoreverse (off in reduced motion) | triangle wave, easeInOut 1.6 each way |
| 27 | Toast in/out | move from top + opacity, `Motion.overlay`; stays 3.2 s | spring overlay, `offset_y = -(1 - v) * 80`; a new toast replaces the old one and restarts the 3.2 s timer |
| 28 | Banner slide in | card offset y from -150 to 14 and opacity 0 -> 1, spring 0.55 / 0.78 (easeOut 0.25 in reduced motion) | see 5.14 |
| 29 | Banner icon pop | icon scale 0.6 -> 1 and glow opacity 0 -> 1, spring 0.45 / 0.55, delay 0.18 s | see 5.14 |
| 30 | Banner out | after 5 s: easeIn 0.3 s back up and out; next banner starts 0.35 s later | see 5.14 |
| 31 | Scroll-to-focus in lists | `scrollTo(anchor: .center)` inside `Motion.focus` | spring focus on the scroll offset toward "focused row centred" |
| 32 | Tile hover | none (only buttons hover: fill white 0.16) | |
| 34 | Select pulse | a click shows the control pressed while the button is down | a select from a key or the pad holds the focused control pressed for 0.09 s (`ui::press`, `Motion.press`, scale 0.97 on controls, +6 % lift on rows): the same look as a click, in the frame of the cue |
| 33 | Edge bump (6.7) | none | focus ring leans 3 dp toward the wall and returns: velocity kick 140 dp/s on `Motion::Focus`; off under reduced motion |

Animations 1, 10-15 can share one helper: `focus_t: Spring` per focusable widget id stored in egui memory.

---

## 3. Animated theme backgrounds

Original: `BackgroundRenderer.swift`, one Metal fragment shader with three theme functions, compiled at start. It renders at
**half the physical pixel size** (`drawableSize = bounds * backingScale * 0.5`) at 60 fps on the Mac, is soft by design, and leaves the GPU to
the games. The launcher draws the idle backdrop and the artwork drift at a target of about 30 Hz (`IDLE_REPAINT`, 30 ms, meant to land on every second vsync of a 60 Hz display; not yet measured on a real window); the clock advances by real time, so the drift speed is the same.

### 3.1 Driver

```
clock   = random(40.0 ..= 400.0)           // start somewhere in the middle of the animation, not at its quiet first frame
each frame:  clock += min(0.1, now - last) * speed      // speed = 1; dt clamp avoids jumps after pauses
uniform.time = clock mod 3600
paused = a game is running  OR  the window is not active  OR  reduced motion   // paused: no clock advance, keep last frame
```

Theme change swaps only the palette and the function index; the cross-fade is done on the compositing side (animation 6).
When paused the original stops redrawing; draw one frame when pausing so the picture stays.

### 3.2 egui/wgpu integration

- Use an `egui_wgpu::Callback` (`CallbackTrait::prepare` + `paint`) registered through `Painter::add(Shape::Callback(...))`
  covering the whole window rect, drawn **before** any UI.
- Render the shader into an offscreen texture of `ceil(window_px * 0.5)` (`Rgba8Unorm`, linear sampling) in `prepare` using the
  provided `egui_encoder`, then in `paint` draw that texture as a full-rect quad (a second tiny pipeline), upscaled with bilinear
  filtering. This also gives the texture the blur passes in section 4 need.
- Output is **display-referred**: the Metal target is `bgra8Unorm` and the shader writes sRGB-encoded values directly. Keep the
  offscreen target `Rgba8Unorm` (not `...Srgb`) and let egui's own sRGB handling treat the blit as an ordinary texture, or if the
  swap-chain format is sRGB, apply `srgb_to_linear` on the way out. Verify by comparing with the screenshots (S-themes).
- The three onboarding theme cards show the theme frame: either render three small textures (original: 660 x 372 stills at
  `time = 200`, the focused card animates, the others are paused) or reuse the pipeline with a different uniform.

### 3.3 Uniforms (std140-friendly, 64 bytes)

```
struct Uniforms { deep: vec4<f32>, middle: vec4<f32>, bright: vec4<f32>, size: vec2<f32>, time: f32, theme: u32 }
```

`deep/middle/bright` = palette rgb with a = 1; `size` = offscreen pixel size; `theme` 0 = Cosmic, 1 = Aurora, 2 = Sunset.

### 3.4 WGSL port (complete)

Direct port of the Metal source. Differences from the Metal text that must be kept: all `smoothstep` calls go through `sstep`
because the original calls it with `edge0 > edge1` (well-defined in Metal, undefined in WGSL); a scalar added to a vector is written
as an explicit `vec2f(...)`. **This WGSL was written by hand and not compiled** (no GPU/Rust toolchain was used on the machine that
produced this document): compile it with naga first, and compare against the screenshots.

```wgsl
struct Uniforms { deep: vec4<f32>, middle: vec4<f32>, bright: vec4<f32>, size: vec2<f32>, time: f32, theme: u32 };
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) id: u32) -> VsOut {
    // one oversized triangle: corners (0,0) (2,0) (0,2)
    let corner = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(corner.x, 1.0 - corner.y);   // uv origin top-left, y down (uv.y = 1 at the bottom of the screen)
    return o;
}

fn sstep(e0: f32, e1: f32, x: f32) -> f32 {        // Metal-style smoothstep, valid for e0 > e1
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn hash(p_in: vec2<f32>) -> f32 {
    var p = fract(p_in * vec2<f32>(123.34, 456.21));
    p = p + vec2<f32>(dot(p, p + vec2<f32>(45.32)));
    return fract(p.x * p.y);
}
fn hash2(p: vec2<f32>) -> vec2<f32> { return vec2<f32>(hash(p), hash(p + vec2<f32>(17.17))); }

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), w.x),
               mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), w.x), w.y);
}

fn fbm(p_in: vec2<f32>) -> f32 {
    var value = 0.0;
    var amplitude = 0.5;
    var p = p_in;
    for (var octave = 0; octave < 5; octave++) {
        value += amplitude * noise(p);
        p = p * 2.03 + vec2<f32>(11.7);
        amplitude *= 0.5;
    }
    return value;
}

// Drifting soft discs of light; `rate` moves them upwards (negative: downwards).
fn bokeh(p: vec2<f32>, time: f32, cells: f32, rate: f32, seed: f32) -> f32 {
    var grid = p * cells;
    grid.y -= time * rate;
    let cell = floor(grid);
    var light = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let here = cell + vec2<f32>(f32(x), f32(y));
            let random = hash2(here + vec2<f32>(seed));
            let center = here + vec2<f32>(0.5) + 0.38 * sin(random * 6.2831 + time * (vec2<f32>(0.15) + 0.2 * random.yx));
            let radius = mix(0.06, 0.24, random.x * random.x);
            let dist = length(grid - center);
            let twinkle = 0.55 + 0.45 * sin(time * (0.4 + random.y) + random.x * 40.0);
            let present = step(0.55, hash(here + vec2<f32>(seed * 3.1)));
            light += present * twinkle * sstep(radius, radius * 0.25, dist) * (0.35 + 0.65 * random.y);
        }
    }
    return light;
}

fn cosmic(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    var color = mix(u.middle.rgb * 1.15, u.deep.rgb, sstep(0.0, 1.0, uv.y * 0.85 + 0.08));
    color += u.bright.rgb * 0.28 * exp(-2.8 * length(vec2<f32>((uv.x - 0.8) * aspect, uv.y - 0.08)));
    // ribbons of light flowing across the lower half
    for (var band = 0; band < 6; band++) {
        let b = f32(band);
        let center = 0.56 + 0.028 * b
            + 0.075 * sin(uv.x * (1.4 + 0.22 * b) + time * (0.10 + 0.021 * b) + b * 1.9)
            + 0.03 * sin(uv.x * 4.1 - time * 0.12 + b * 3.1);
        let d = abs(uv.y - center);
        let core = 0.0016 / (d + 0.0028);
        let haze = exp(-d * 14.0);
        let fade = sstep(-0.15, 0.4, uv.x) * sstep(1.2, 0.6, uv.x);
        let pulse = 0.75 + 0.25 * sin(time * 0.3 + b * 1.7 + uv.x * 2.0);
        let tint = mix(u.bright.rgb, vec3<f32>(0.85, 0.92, 1.0), 0.25 + 0.1 * b);
        color += tint * (core * 0.16 + haze * 0.1) * fade * pulse;
    }
    let p = vec2<f32>(uv.x * aspect, uv.y);
    color += u.bright.rgb * 0.22 * bokeh(p, time, 5.0, 0.012, 1.0);
    color += vec3<f32>(0.9, 0.95, 1.0) * 0.14 * bokeh(p, time * 1.3, 11.0, 0.02, 7.0);
    return color;
}

fn aurora(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    var color = mix(u.deep.rgb * 1.8, u.deep.rgb * 0.7, uv.y);
    let p = vec2<f32>(uv.x * aspect, uv.y);
    for (var layer = 0; layer < 3; layer++) {
        let l = f32(layer);
        let sway = fbm(vec2<f32>(p.x * 0.7 + l * 3.7 + time * 0.02, time * 0.03 + l));
        let center = 0.30 + 0.09 * l + 0.36 * (sway - 0.5) + 0.05 * sin(p.x * 1.7 + time * 0.08 + l * 2.0);
        let thickness = 6.0 + 2.0 * l + 2.5 * sin(p.x * 1.3 + time * 0.05 + l);
        let below = uv.y - center;
        let curtain = exp(-below * below * thickness * thickness) * sstep(-0.35, 0.0, below + 0.12);
        let rays = sstep(0.15, 0.95, noise(vec2<f32>(p.x * 16.0 + l * 7.0, time * 0.18 + p.x * 0.5)));
        let glow = curtain * (0.45 + 0.55 * rays);
        let tint = mix(u.bright.rgb,
                       mix(vec3<f32>(0.35, 0.55, 1.0), vec3<f32>(0.75, 0.4, 1.0), l * 0.5),
                       sstep(-0.05, 0.12, -below));
        color += tint * glow * (0.62 - 0.14 * l);
    }
    let starCell = floor(p * 150.0);
    let star = step(0.9965, hash(starCell)) * (0.55 + 0.45 * sin(time * 1.6 + hash(starCell + vec2<f32>(3.0)) * 30.0));
    let local = fract(p * 150.0) - vec2<f32>(0.5);
    color += star * sstep(0.35, 0.0, length(local)) * sstep(0.8, 0.1, uv.y) * 0.9;
    color += u.middle.rgb * 0.45 * sstep(0.6, 1.0, uv.y) * (0.6 + 0.4 * fbm(vec2<f32>(p.x * 1.5, time * 0.03)));
    return color;
}

fn sunset(uv: vec2<f32>, aspect: f32, time: f32) -> vec3<f32> {
    let top = u.deep.rgb * 1.4;
    let middle = u.middle.rgb;
    let low = mix(u.bright.rgb, vec3<f32>(1.0, 0.42, 0.38), 0.45) * 0.85;
    var color = mix(top, middle, sstep(0.0, 0.6, uv.y));
    color = mix(color, low, sstep(0.5, 1.1, uv.y));
    let p = vec2<f32>(uv.x * aspect, uv.y);
    for (var blob = 0; blob < 6; blob++) {
        let b = f32(blob);
        let center = vec2<f32>((0.15 + 0.16 * b) * aspect + 0.14 * sin(time * (0.05 + 0.013 * b) + b * 2.0),
                               0.5 + 0.24 * sin(time * (0.04 + 0.01 * b) + b * 1.3));
        let radius = 0.2 + 0.07 * sin(b * 4.1);
        let d = p - center;
        let light = exp(-dot(d, d) / (radius * radius));
        color += mix(u.bright.rgb, vec3<f32>(1.0, 0.35, 0.6), fract(b * 0.37)) * light * 0.2;
    }
    color += mix(u.bright.rgb, vec3<f32>(1.0, 0.8, 0.6), 0.4) * 0.22 * exp(-abs(uv.y - 0.76) * 5.0)
             * (0.85 + 0.15 * sin(uv.x * 2.4 + time * 0.15));
    color += vec3<f32>(1.0, 0.85, 0.7) * 0.1 * bokeh(p, time, 7.0, -0.01, 3.0);
    return color;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let aspect = u.size.x / max(u.size.y, 1.0);
    let uv = in.uv;
    let time = u.time;
    var color: vec3<f32>;
    if (u.theme == 1u) { color = aurora(uv, aspect, time); }
    else if (u.theme == 2u) { color = sunset(uv, aspect, time); }
    else { color = cosmic(uv, aspect, time); }
    let centered = (uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
    color = color * (1.0 - 0.45 * sstep(0.35, 1.25, length(centered)));   // vignette
    color = color / (vec3<f32>(1.0) + color * 0.35);                      // soft tone map
    color += vec3<f32>((hash(in.pos.xy + vec2<f32>(fract(time) * 91.0)) - 0.5) / 255.0 * 2.0);  // dither
    return vec4<f32>(color, 1.0);
}
```

Behaviour summary, for sanity checks against screenshots:

- **Cosmic**: vertical gradient middle*1.15 (top) to deep (bottom), a bright glow in the upper right (centre x 0.8, y 0.08), six
  sinusoidal ribbons of light across the lower half (y about 0.56 to 0.7) that fade in toward the left, and two layers of slow bokeh
  discs (5 and 11 cells across) drifting upward. (S-themes, left card.)
- **Aurora**: dark teal sky, three vertical curtains (fbm sway, ray structure from stretched noise) coloured mint -> blue -> violet,
  tiny twinkling stars in the upper part, a soft middle-colour haze at the bottom. (S-themes, middle card.)
- **Sunset**: three-stop vertical gradient (deep*1.4, middle, warm low band), six large soft glows drifting around, a warm horizon
  band at y 0.76, sparse bokeh drifting **down**. (S-themes, right card.)

### 3.5 User background picture or video

Setting "Background: My picture / My video": the picture (or a looping, muted video) replaces the shader entirely.
The picture is decoded at `max(w, h) * 2` px and slowly drifts (animation 9). Video: looped, muted, `aspectFill`, paused when the
window is inactive/a game runs/reduced motion. Video decoding is out of scope for the first port (offer the picture only, hide the
"My video" choice until a decoder exists); keep the preference value.

### 3.6 Cleaned-up backgrounds (deviation from 3.4)

The shipped `shaders/background.wgsl` keeps each theme's composition and palette but no longer matches 3.4 line by line:

- **Hash and noise:** an integer PCG hash per cell replaces `fract(p * 123.34 ...)`. The float hash gave neighbouring pixels
  different values for the same cell once the GPU compiler fused the multiply, which showed as hard vertical seams in Aurora.
  `noise` is gradient noise with a quintic fade (no creases at the lattice).
- **Colour:** the base gradients (all three themes) blend in Oklab instead of sRGB, so Sunset no longer turns brown between rose
  and amber.
- **Cosmic:** four strands on one slow wave instead of six crossing ribbons; fewer, softer bokeh with a gentler twinkle; the light
  pool and the ribbon haze are Gaussian (no bright point or crease line in the middle).
- **Aurora:** curtains have a crisp lower rim and a long glow upwards; wide soft folds replace the 16-per-unit ray columns; fewer
  stars, slower twinkle.
- **Sunset:** five slower glows, a Gaussian horizon band, very sparse bokeh.
- **Banding:** the offscreen backdrop is `Rgba16Float` and the blit adds a fixed triangular dither of about one 8-bit step per
  framebuffer pixel; the per-frame dither of the half-resolution pass is gone.

For tuning, run `anyps5-launcher --gallery=bg0 --live` with `ANYPS5_SHADER_DIR` set to `crates/anyps5-launcher/src/ui/shaders`:
the background and blit shaders reload on save (invalid WGSL is logged and skipped), Tab switches the theme.

---

### 3.7 Cleaner home, panels and launch (deviation from 1.4, 5.x and 7.1)

The launcher keeps the port's structure but trims what read as noise next to the PS5 home.

- **Launch intro** (`ui::intro`). The window opens black for 0.15 s, the backdrop comes up out of it
  over 1.1 s (ease-in-out) while the `boot` cue (7.3) swells and the home music starts its 2 s fade.
  Then the parts rise 18 dp into place on a strong ease-out (`cubic-bezier(0.23, 1, 0.32, 1)`):
  top bar at 0.45 s for 0.7 s, tile row at 0.55 s for 0.75 s, hero at 0.7 s for 0.8 s, hints at
  0.9 s for 0.6 s. Once per launch; with reduced motion only a 0.45 s fade from black remains.
  Previews and tests skip it (`--intro` forces it in the preview).
- **Tile row.** The selected tile no longer carries its name beside it (the hero title shows it,
  the state line moves above the title), so the row closes up (`TILE_LABEL_COLUMN = 0`). Small
  tiles share the large tile's centre line. Only the selected tile shows the locked badge and dim.
  From the second game on the row moves 28 dp right (`TILE_PEEK_SHIFT`) and the game before the
  selected one stays on screen at 0.4 opacity (`TILE_PEEK_OPACITY`), partly past the window edge;
  it can be clicked to select it. On the first game the row sits at `leading` as before.
- **Hero.** One left edge with the top bar (`HERO_X = 64`; the tile row is the exception from the second game on, see Tile row). The four boxed info cards become a
  plain facts line (Size, Version with the title ID beside it in 14 dp quiet ink, then Runs, Toolkit and Played when
  there is something to say; at most four, the playing time gives way first; 13 dp tracked overline over a 17 dp value, 48 dp
  apart, no card). The status card is gone; the state stands above the title. Switching games
  cross-fades without the 14 dp slide (it happens on every arrow press). The "more" button is a
  54 dp square with three painted dots.
- **Hint bar.** Follows the device in use (`input::device`): keycaps `Enter`, `Esc`, `O` on a
  keyboard, ✕ ○ △ in circles on a gamepad. No combined labels, no Move hint. 26 dp keys, 15 dp
  labels, 28 dp between hints.
- **Top bar.** Outline icons at 20 dp (`squares-four` for the library, `images` for the captures, `users` for friends, `gear-six`
  for settings), 12 dp between the buttons of the strip, 32 dp between the groups (the strip, the
  profile, the clock). The profile and the friends button are pills on the icon buttons' own
  surface, hover and focus, so the whole corner reads as one family of controls. The avatar's
  gradient circle is feathered by hand (a raw mesh gets no anti-aliasing).
- **Shadows and focus.** Elevation is a quiet dark shadow (tile black 0.22 to 0.32), focus is
  the white fill or ring with only a faint halo (0.07 to 0.14 instead of 0.22 to 0.45). Focus
  rings grow their radius with their outset, so they stay concentric. Button hairlines drop to
  white 0.07; the primary button rests at white 0.26 so it does not read as disabled.
- **Panels.** Rows grow a fixed 3 dp when focused (no proportional scale). Choice values are
  regular weight. The game sheet groups display, play, maintenance and
  removal with a double gap. The removal confirmation opens on Cancel and the destructive row
  stays dark red when focused. Full-screen overlays share a 90 dp margin.
- **Crisp edges.** Scaled buttons snap to physical pixels and use one whole-dp radius for shadow,
  fill and stroke. Hairlines, glass strokes and the side panel edge are one physical pixel wide
  (`1 / ppp`), and the glass shader shares the stroke's pixel-aligned rect and radius. Text sizes
  quantise to whole physical pixels, not 0.25 dp. Chevrons are drawn lines (1.75 dp), not the thin
  font caret; the cross glyph is 1.8 dp against 1.5 dp for the others, the triangle sits 0.12 r
  lower. The indeterminate band follows the capsule's rounded ends.
- **Panel rows, quieter.** Chevrons and steppers rest at 0.45 and come up on the focused row;
  choice and stepper values share a 100 dp minimum so their chevrons line up down a panel. Only
  rows that open a picker or a panel keep the trailing chevron; commands (rescan, remove, reset,
  log) have none. Section captions (14 dp semibold, white 0.55) and notes (14 dp regular, white
  0.5) are bare text on the panel, as high as their text, not 58 dp rows. The settings rows start
  level with the first category, and the trophy rows and library grid stand on the title edge. The
  focused trophy is the white card (3 dp grow, dark ink, darker grade tones), not a ring.
- **Sharing, quiet.** The layout export and import are two plain command rows above Reset
  to Default Keys; community layouts are options of the Layout choice, not a page. The report to
  AnyPS5 reuses the log view for its review, so no new surface exists: a plain sentence (no
  warning box, it warns of nothing) above the text, the primary button on the right.
- **Home polish.** Soft shades sit under the home UI so white text reads on bright artwork (soft-edged patches since
  round 3, `ui::shade`, 3.10; first a black 0.30 band over the top 150 dp and black 0.46 in the hero's bottom-left corner). The
  selected tile keeps 10 dp more air before the next one (`TILE_LABEL_COLUMN = 10`). While a game
  unlocks, the step is named once, in the state line above the title; the hero shows only a 260 dp
  bar and its percentage on the buttons' centre line. The scanning icon breathes in opacity only.
  The launch splash's explanation wraps at 460 dp, so it never ends on a lone word. The onboarding
  theme names share one baseline under the resting cards; focus grows the card, not the label.
- **Toolkit channels, notes and recommendations.** The Toolkit page has one `Channel` choice
  (Stable, AnyPlay Engine, Pinned, Local Build: the choice steps one entry at a time and Local Build opens a folder picker, so it stays last and every other channel is reachable without it) and shows only the rows of that channel: stable has the update
  toggle and "Check Now" (its detail line carries the status or the last check), pinned has a
  `Version` choice over the last ten releases and no update, local has the folder row, and AnyPlay Engine has the update toggle and "Check Now" like stable, with the day the build was made hanging under the version row ("Built on 2026-10-25"; gallery scenes `toolkit-engine`, `toolkit-engine-update`) and no "What's New" (a nightly build has commit messages, not notes). Choosing it downloads the nightly build when none is installed; "Install the Update" fetches a newer one, checked against the release's checksum list, and a refusal is said in a toast. "What's New"
  opens the notes of the release waiting to be installed, else of the installed one, in one
  full-screen panel: header with release, date and count, Highlights (`feat` and `perf` by area,
  the seven busiest areas folded, the rest as "Other areas"), then Everything. Rows are 44 dp, drawn
  from the visible range only; select unfolds an area or opens the pull request. The game sheet
  gains one row, "Recommended for this game", first in the play group, only when the catalogue
  has something: it opens a side panel with `current -> recommended` lines, the reason under each,
  and one `Apply`. Warnings alone show a `Close` row instead. "Unlock Again" carries a detail line
  ("Unlocked with v0.1.2. v0.1.3 is installed.") when the copy was made with another toolkit.
  What each toolkit release supports is the table in `core::compat`; an older toolkit loses the
  feature (screenshots row, patch resolutions), never the launch.
- **One way to report a problem.** The game sheet has one row, "Report a Problem" (detail: "Review
  the report, then save it or open it on GitHub"), in place of "Make a Report" and "Report to AnyPS5". It
  opens the review (the log view): Cancel, Save Report and Open on GitHub, 12 dp apart, Open on GitHub
  the one primary. The log view's own button reads "Report a Problem" and leads to the same review.
  The sentence above the text says both ways out in one line.
- **Recommended, grouped.** Each reason hangs directly under its `current -> recommended` line
  (`Row::attached`, no row gap, no air above the note); the pairs are set apart by the double gap, so
  a reason no longer sits as far from its line as from the next one. The toolkit page's "Check
  Now" no longer repeats "Update available" (the row above says it) and shows the last check;
  "What's New" uses the document glyph, not the menu one.
- **Review pass.** Recommended titles are in title case like every row; its hint says what to do
  after applying; the Apply or Close row has the double gap above it.
- **Buttons keep their gap when focused.** A focused console or icon button grows a fixed 3 dp on
  every side (`FOCUS_GROW`), and the layout reserves that room while it is focused, so the
  neighbour moves aside by the same amount instead of the gap closing to 7 dp. Every row of
  buttons is 12 dp apart (the hero's was 16, the notes header's 14, the onboarding's 18); the
  top bar is one right-to-left strip so its two buttons do the same. Panel rows are a list, not
  buttons: they keep the 3 dp grow over the 8 dp row gap. The room is the one layout motion in the
  app: the neighbour moves on the same focus spring as the edge it makes room for, so the gap is
  12 dp in every frame and nothing overlaps.
- **Commands read as commands.** "Check Now" is its title and icon alone, like "Install the
  Update"; what the last check found hangs under it as a note (`Row::attached`). A detail line
  under a command looked like the description of a status row.
- **Caption corner.** Full-screen panels start their header at `pad::PANEL_TOP` (the 32 dp caption
  buttons plus 32 dp), so a button row on the right never touches the window buttons (the notes
  header sat 3 dp off them at 1280 x 720). Settings and the log were already lower.
- **One GitHub button.** "Open on GitHub" in the notes and in the report review has the same
  words and the same glyph (`github-logo`).
- **Played.** The facts line gains a fourth fact, "Played" over the playtime ("45 min", "3 h 20 min",
  "12 h"; "Under 1 min" for a first minute), only for a game that was played. Hours are rounded down
  and the minutes drop from ten hours on. The clock (`app::playtime`) starts when the game's
  window shows, and adds what it counted to the preferences every minute and at the exit, so a
  crash keeps it. The game sheet shows no second copy: the home screen stays visible behind it.
- **Save data.** The game sheet gains one row, "Save Data" (detail: the number of backups), after
  Trophies and only for an unlocked game. It opens a side panel: the command "Back Up Now" (with a
  note under it while there is nothing to back up), then the backups newest first, each a row with a
  chevron: the date, then its size and why it was made. Selecting one opens the same two-row question
  as the removal, on Cancel, saying what is replaced and that the current state is backed up first;
  either answer returns to the list, where the toast says what happened. A running game gets a note
  and a refusal instead. Backups are also made, quietly, before Unlock Again and before the removal
  of a game, and once an hour after a game ends well (not after a crash, which may have damaged the
  saves). Gallery scenes `saves`, `saves-empty` and `restore`.
- **A steady toolkit header.** The toolkit version row is one line always; the note about a toolkit
  newer or older than the launcher was tested with is its own attached row below it, so the
  title no longer moves up when it appears (gallery scene `toolkit-newer`).
- **Gallery scenes** `about-update` (About with a staged update, focus on "Restart Now") and
  `behavior-links` (links registered here) cover the two states that had none.
- **Start-up and idle** (docs below measured on the demo preview, release build, AMD 880M plus RTX
  5070 laptop, warm; the real launcher adds only preferences and a queue). The window now uses
  DirectX 12 alone on Windows (Vulkan stays the fallback: a start without a DirectX 12 GPU runs
  again with `ANYPS5_GPU=vulkan`; OpenGL stays out) and asks for the low-power GPU, so the
  discrete one is not woken for a launcher. Enumerating Vulkan loaded every driver layer and probed
  both GPUs (cooperative-matrix queries) before the first frame. Time from `main` to "window and
  GPU ready": 571 and 600 ms before (2200 ms on the first run), 369 to 460 ms after (530 ms first
  run). The pipelines take 130 to 170 ms, the first frame about 60 ms. A focused, idle home screen
  used 16.5 % of a core and 30 % GPU (58 threads) before, 11 % and 15 % (26 threads) after; the
  backdrop still drew at 60 Hz while focused then (it draws at about 30 Hz since the idle-rate change; the figures above come from the demo). A window without focus, and a game
  running, stop all drawing (0.2 to 2 % of a core). The gamepad thread polls every 100 ms while no
  pad is connected (4 ms with one). `ANYPS5_GPU=dx12|vulkan|both` and `ANYPS5_POWER=high` undo the
  choice for diagnosis; the log carries `startup t=...ms <step>` lines and the launcher an `idle ...
  frames/s` line every 10 s (the window restarts after a gap, so the first line after a refocus is a true rate). The demo no longer forces a repaint every frame (it idled at 40 % of a
  core, unlike the launcher).

### 3.8 A toolkit for one game

A newer toolkit can break one game that every other game likes. The game stays on the release it
works on; the rest follow the global choice (Stable, Pinned, Local Build, 3.7).

- **The choice.** The game sheet has one row, "Toolkit for This Game", after "Game Language" and
  before the maintenance commands. Its entries: "Follow Global Setting" (the default, and what a
  game without the setting does); "Recommended for This Game: v0.1.2" when the catalogue names a
  release for this game and the toolkit it uses now (see `data/README.md`, `set.launcher.toolkit`);
  then the installed releases and the last releases of the configured repository, newest first.
  A release that is not on this PC reads "v0.1.1 (not installed)": choosing it is allowed, the
  first start or unlock then asks (below). The row is left out while there is nothing to choose
  from. A file of exported settings neither carries nor changes the choice: it is about this PC.
- **Side by side.** A release a game stays on, or that the pinned channel stays on, is never deleted
  when a newer one is installed; the clean-up that keeps the newest two skips it. Settings >
  Toolkit lists every installed release that neither the toolkit in use, nor the pinned channel,
  nor a game needs, as one command each ("Remove v0.1.2", detail "412 MB · No game uses it"),
  after the CPU conversion row, at most six at a time. Only the configured repository is asked for
  a release, and a release is checked on install like any other.
- **Unlock and start.** An unlock uses the game's release, a start tells the game what that
  release supports (input file, screenshots) and the "outdated" mark and the note under "Unlock
  Again" compare the copy with the game's release, not the global one. Resolutions come from the
  patches of that release.
- **A release that is gone.** Pressing Play or Unlock for such a game opens a side panel over the
  home screen (title: the game, one sentence: "This game stays on toolkit v0.1.2, which is not
  installed. Download it again, or let the game follow the global setting."), rows "Download
  v0.1.2" (focused first; it shows the download and the panel closes when the release is there),
  "Follow Global Setting" (removes the game's choice) and "Cancel". Nothing starts by itself
  afterwards, and the global toolkit never stands in. A start or unlock that did not come from the
  buttons (an invitation) fails with "the toolkit 0.1.2 chosen for this game is not installed".
- **The fact.** The facts line gains "Toolkit": "v0.1.3" for a game that follows the global
  setting, "v0.1.2 (pinned)", "v0.1.2 (not installed)" or "Local Build".
- **Preferences.** `games.<id>.toolkit` (`"v0.1.2"`). It is written only when set, so a file
  without it, and every game that follows the global setting, read and write as before.
- Gallery scenes `game-toolkit` (the row, on a pinned game), `toolkit-unused` (two removable
  releases) and the home scenes `home-toolkit` and `home-toolkit-missing` (the fact; the question).

### 3.9 One control family and one focus (2026-10-25, replaces the white focus of 1.2, 1.4 and 5.x)

The audit is `docs/ui-audit-2026-10-25.md`. The owner's findings were: the focused profile pill turns white and
covers its neighbours, the buttons are three families, hover is missing, the clock looks odd. The rules:

**Controls are capsules, containers are rounded rectangles.** Every button, pill and icon button is a capsule (a glyph alone
makes a circle); rows, cards, cells and tiles keep their radii (12, 16, 20, 12 %). Three heights, nothing between:

| Size | Height | Text | Glyph | Used for |
|---|---|---|---|---|
| Large | 54 | 19 semibold, 30 padding | 17 (22 alone) | the action row of the game page, dialogs |
| Medium | 44 | 16 semibold, 20 padding | 18 (20 alone) | the top bar: library, captures, friends, settings, profile; the chips and actions of the Captures area |
| Small | 32 | 14 medium, 14 padding | 14 | an answer inside a line of text ("How did it run?") |

Neighbouring controls are always 12 apart (`size::CONTROL_GAP`), in the top bar and the action row alike. The profile pill is a
control like the three buttons; only the clock, which is a status and not a control, sits 28 dp further out.

**Two variants.** *Primary* is ink, not accent: white 0.92 fill, a dark label (`color::on_primary`, 16:1 to 19:1 on the theme tones), one per view
(Play, Unlock, Accept). *Secondary* is everything else: white 0.10 fill, a 1 px white 0.12 hairline, white ink. Destructive is
a secondary with the destructive ink.

| State | Secondary | Primary |
|---|---|---|
| Rest | fill 0.10, hairline 0.12 | fill 0.92 |
| Hover (pointer only, eases in 120 ms) | fill 0.16, hairline 0.32, pointing-hand cursor | fill 1.0 |
| Pressed | fill 0.26, scale 0.97 | fill 0.78, scale 0.97 |
| Focused | fill 0.20, hairline 0.32, **ring** | fill 1.0, **ring** |
| Disabled | 40 %, no hover, no cursor | 40 % |

**Focus is a ring** (3.11: it shows only for keys and the pad, and is quieter than written here). One treatment everywhere: a ring outside the shape with a 1.2 dp gap, 1.5 dp wide on controls
(`size::FOCUS_RING`) and 2 dp on tiles, cards and cells, plus a fill one step lighter. Nothing grows, nothing swaps its ink and no
glow sits behind a translucent fill (a shadow under a translucent fill shows through as a seam). The layout never moves when focus
moves, so the 12 dp gap holds. The ring is the focus indicator for keyboard and gamepad: it is white on dark artwork and on every
fill of the family (a primary focused is a white ring around a white capsule, separated by the gap). Containers (rows, cards,
cells) use `components::paint_surface`: rest fill, +5 % white on hover, +11 % and the ring on focus, +6 % pressed.

**Hover** exists on every control the pointer can reach and answers like the tiles do: the tiles draw the focus ring at 45 % under
the pointer, controls lighten and show a brighter hairline. Only colour and opacity change, in 120 ms (`Motion::Hover`, the same
under reduced motion because it moves nothing); the press scale (0.97) is the only transform.

**The clock** is a status line, not a control: 17 medium, white 0.78, set in a box four digits wide with the colon fixed in the
middle (two dots on the digits' cap height), so the minute ticking over moves nothing and the digits need no tabular cells
(whose spaced 1s read as a typo). Centred on the bar's centre line with the buttons.

**Failed unlock** says what happened in the headline ("Unlocking stopped.") and, under it, one plain sentence that names the way out (free some space, connect the drive, try again, Options > Log). Every failure reaches the page as such a sentence (`prepare_strings::failure`); the technical error, including a toolkit that could not be opened, goes to the log and never to the page. The sentence runs to three lines and, when it is cut, is read in full under the pointer. Scenes `home-failed`, `home-failed-long`.

**Round 2 (2026-10-25).**
- *Press is immediate.* The press scale (0.97) rides `Motion::Press`: easeOut 0.10 s both ways, retargeted from the
  current value, so a quick click reverses mid-press. The focus spring (0.32 s) stays for focus only.
- *The reaction pills of the feed* are Small controls (32), like every answer inside a line of content.
- *Optical centring.* A right-pointing triangle has its mass on the flat side; `Icon::optical_dx` moves the play glyph 8 % of
  its size to the right.
- *The pointer lights nothing while a pad is the device in use* (`input::hovered`, `input::pointer_in_use`): no hover fill,
  no hover ring and no pointing hand next to the control the pad has focused. Moving the mouse takes the device back.
- *Screen-reader focus follows the ring* (`ui::a11y`). The model decides the focus and the widgets draw the ring; egui's
  own focus is what AccessKit announces, so each focusable widget calls `a11y::follow` with whether the model has it
  focused. egui's focus is only taken when nothing, or our own earlier choice, holds it (a text field being edited, or a
  control reached with Tab, keeps it); Tab and the arrows are claimed so egui does not move the focus away from the ring;
  Enter and Space, which egui would also read as a click on the focused widget, are consumed once the model has taken
  them as "select".
- *A note marked destructive in a panel* was a warning in this round; it is `Row::warning` since round 3 (3.10). The top scrim
  of the home screen is 0.40 so the clock and the caption glyphs hold on a pale picture (0.45 since round 3).
- *Gallery.* The panel scenes draw the real home under the panel (`app::preview::HomeStand`: the same `home::show` and
  artwork layer as the app, with the focus hidden as under a panel). New flags: `--theme=aurora|sunset|cosmic` (components,
  panels, home scenes), `--bright` (a pale picture as the selected game's artwork), `--frames=N` (gallery pages, so the real
  home's pictures have loaded), `--hover=X,Y` with `--pad` rests the pointer without taking the device back from the pad.
  New page `--gallery=states`: destructive buttons, the switch and rows in every state.

### 3.10 Round 3: contrast measured, one case rule, Tab (2026-10-25)

**Contrast is measured, not estimated.** `ui::contrast` (a test: `cargo test -p anyps5-launcher contrast -- --nocapture` prints the
table) lays every text token and the status, state and accent inks over the backdrops they meet and measures them with the WCAG
2.x formula. A pair has to reach 4.5:1 as text, 3:1 as large text (24 dp, or 18.66 dp bold), as a glyph or as a part of a control.
Inactive and disabled ink (`text_inactive`, a locked trophy's picture) is exempt by the standard. The backdrops come from the theme
tokens: per theme the deep tone, the lightest tone the shader paints behind text (Sunset's lit lower band; a soft glow under the
top bar), the panel fill over the theme's lit tone with a row in each state (rest, hover, focus, pressed), the toast fill, and a
pure white picture under the artwork scrims, the worst a game can bring. The home screen's text is measured where it sits, under
the shades below. A rendered screenshot is measured with `tools/measure-contrast.mjs` (the backdrop under a box is its median colour, the ink
is composited on it); the model and the render agreed to within 0.1 on the facts, hints and clock of a white picture.

With the tokens of round 2 the test found 71 of 504 pairs short. What changed (the weakest ratio of the old ink, then the new one;
the new weakest of every ink is in the test's output):

| Ink | Before | After | Where it was short |
|---|---|---|---|
| `text_overline` (facts, captions over rows) | white 0.55, 3.2 to 3.8 | 0.70 | facts on Sunset's band, a focused or pressed row |
| `text_clock` | 0.78, 4.2 | 0.88 | the clock over a white picture |
| `text_detail`, `text_row_detail`, `text_inert` | 0.60, 0.55, 0.62; 3.5 to 4.2 | 0.70 | a row in focus or pressed |
| `text_empty`, `text_subtitle`, `text_path` | 0.65, 0.70, 0.45; 4.3 to 4.5 | 0.72, 0.72, 0.50 | a pressed row |
| `text_hint`, `text_hint_key` | 0.72, 0.92; 4.4 | 0.78, 1.0 | the hint bar on Sunset's band, the key on its cap |
| `destructive_text`, `error_state` | (1.0, 0.60, 0.55); 3.6 | (1.0, 0.72, 0.67) | a focused or pressed row |
| `destructive_icon`, `error_text` | (1.0, 0.55, 0.50), (1.0, 0.70, 0.65) | (1.0, 0.62, 0.57), (1.0, 0.73, 0.68) | a pressed row |
| state label of Starting, Installing, Playing | the accent; 3.2 for blue on Aurora's teal, worse on Sunset | `text_active`, white 0.92 (5.1 or more) | an accent on its own colour |
| knob of a switch that is on | white; 1.1 on the White accent, 1.5 on mint or gold | `on_primary`, dark (6.5 or more) | every accent |
| progress line in Blue and Violet | 2.0 and 2.2 on Sunset's band | Blue `#66A3FF`, Violet `#B391FF` (3.2, 3.3) | the unlock line on a bare Sunset |

The initial in the profile button's avatar (and the monogram of a friend without a picture) takes `theme::readable_ink` of its
colour: white, or the dark `on_primary` when that reads better (white on Mint, Gold or White was 1.2 to 2.2:1; every accent now
reaches 4.5). The literal inks the audit found in the screens (white 0.55 to 0.66 as text, 17 places) now use these tokens, and the row text of
the panels uses `text_row_detail`, `text_inert`, `text_note` (0.50, bare on the panel) and `text_caption`. New text ink is a token,
so the test sees it.

**The active states are ink, a glyph and a word.** "Starting", "Installing" and "Playing" were set in the player's accent, which can
be the colour of the theme behind it. They are `color::text_active` now: the glyph (hourglass, arrow, play circle, each its own
shape) and the word carry the state and no colour is needed. The accent stays where it fills something (the progress line, the
switch, the online dot).

**Shades, not a ramp** (`ui::shade`). The home screen's scrim was a ramp over the whole lower half, 0.58 in the corner, laid on the
artwork's own two scrims: on a pale picture that is the grey fog. The artwork scrims already leave a pure white picture at 5.0:1
under the facts and 6.7:1 under the hints, so the home screen lays only what a bare theme needs: a soft-edged patch under the hero
(black 0.46 on a plateau from the left edge to 880 dp and from 400 dp above the bottom down, easing out over 360 dp across and 260
dp up), one under the hint bar (0.28; the right 520 dp, the bottom 110 dp, easing out over 300 and 220 dp) and the band under the
top bar (0.45, 150 dp). The ease follows the distance from the plateau, so the contours are round and no straight edge shows. A
patch is as dark as the ink on it needs (Sunset's lit band sets it) and nothing more; the tile row and the middle of the picture
get none. The facts and the glibc line can run to 880 dp, so the model samples them there too.

**One case rule.** Titles of rows, buttons, captions, headings and the names of a choice are in Title Case ("Save Changes", "Game
Default", "Top Left", "Report as Installed"): every word capital except the short ones inside (a, an, the, and, or, of, to, in, on,
for, with, at, by, from, as). What reads as a sentence is in sentence case: details, subtitles, notes, statuses ("Ready to play",
"Not installed", "Up to date"), toasts, errors, questions and the headlines of empty states ("No games yet"). Turkish follows the
same rule: every word capital except `ve`, `ile`, `de`, `da`, `mi`, `için`, `veya`. The English and Turkish tables were swept once
for it (about 70 strings changed, mostly Turkish buttons and captions); new text follows it and the review checks it.

**Warning is not danger.** `Row::destructive` is for a row that removes, ends or blocks something (Remove Installation, Close Game,
Block): the danger ink. `Row::warning` is for an info row that says something may not work (the glibc note): the amber of the line
under Play. `PanelRow` has both flags, and the danger ink means danger only.

**The question of a confirmation** (Close Game, Remove, Restore, the toolkit pin, Delete a capture, every question of the online area) is the sentence the player decides on: 17 dp
regular in `text_label` ink with 1.45 leading under the card's title, not the quiet 15 dp line of a subtitle. It stands on a card (5.9.1).

**Tab walks the ring.** Tab and Shift+Tab are the model's keys now (`InputEvent::Tab`; egui no longer sees them). On the home screen
they step through the stops in reading order: the top bar from left to right, the tile row as one stop (the arrows choose the
tile), the action row, the question line while it asks; they stop at the ends without wrapping. In a panel they are Down and Up (a
panel is one column of rows), in the onboarding Right and Left (the choices sit side by side). The library, a grid, reads along the row (Tab is Right, Shift+Tab Left). A text field
keeps Tab for itself, and while a call rings Tab stays with egui, which focuses the card's buttons (they are for the pointer and for
Tab; the pad reaches a call through the Invitations row). The strip of a call in progress does not hold Tab: Mute, Deafen and Leave
Call are rows of the Online panel.

**Screen readers follow the ring on more widgets.** `a11y::follow` is called by the switch (when it is a node of its own), by the
cards of the theme step of the onboarding, by the rows of the profile page and by the lines of What's New. The language cards,
rows, tiles, cells and buttons did already.

**The glibc sentence has a next step.** It names the problem and what to do: "Games need glibc 2.38 or newer; this system has 2.35.
Update to Fedora 39, Ubuntu 24.04 or newer." The releases are the two oldest of a short list that ship at least that glibc (Ubuntu
22.04 2.35, Debian 12 2.36, Fedora 39 2.38, Ubuntu 24.04 2.39, Debian 13 2.41); past the newest it says "Update to a newer release
of your distribution." Settings adds a row under the note, "Read the Install Guide", that opens `docs/INSTALL.md` on GitHub
(`about_strings::INSTALL_GUIDE`, the one new address); INSTALL.md lists the releases and their glibc.

### 3.11 Round 5: the ring is for keys and the pad (2026-10-25, changes the ring of 3.9)

The white ring was loud, and it stayed after a click. Now it behaves like `:focus-visible`:

- **It shows only while the keyboard or the pad steer** (`input::focus_visible`). A click or tap anywhere hides it: the control
  that was pressed answers with its press and its hover, nothing lingers, and a click on empty space leaves nothing lit. The next
  arrow, stick, d-pad, Tab or Select brings it back. The press that brings it back only shows where the focus is; it does not
  also move it (a second press moves). It shows at start, where nothing has been clicked yet.
- **One place.** `AnimExt::focus_t` is the amount every ring, lift and fill reads, and it follows the focus only while the ring
  shows; `choice_t` is the same spring for a choice that stays marked whatever moved the focus (the selected category, a theme
  card). `components::paint_focus_stroke` is the one place a ring is drawn. No screen decides on its own.
- **Quiet.** The ring is `color::focus_ring()` (white 0.50, measured against every panel by `ui::contrast`: 5.3:1 on Aurora, 3:1 is needed), 1.5 dp
  on controls, rows and cells and 2 dp on tiles and cards (`size::FOCUS_RING`, `FOCUS_RING_LARGE`), 1.2 dp outside the shape. The
  hairlines of shade beside it are 0.22 and 0.26. The fill carries more of the cue: containers lift 11 % (was 9 %). It fades in
  and out with the 120 ms ease of the hover (`Motion::Hover`): opacity and colour only, the same under reduced motion.
- The pointer resting on a tile or a primary button still draws the hover ring at 45 % of this ring (3.9): that is the hover, not a lingering focus.
- Gallery: `--noring` renders any scene as it looks after a click.

### 3.12 Round 6 (0.2.8): the system, fewer values (2026-10-26, replaces the value tables of 1.2, 1.3 and 1.6)

The audit is `polish028/audit/system.md`. The tokens were many and close together: eleven white alphas for text, nineteen font
sizes, eight radii, and 14 different text alphas as literals in the screens. The rule of this round is that a value is chosen
from a short list, and a page holds no literal that has a token. Where 1.2, 1.3 and 1.6 differ, this section wins.

**Text ink: four levels** (`ui::theme::color`). The contrast test keeps every one of them above its ratio.

| Level | White | Tokens |
|---|---|---|
| Text | 1.00 | `text` |
| Label | 0.90 | `text_label`, `text_value`, `text_clock`, `text_active` |
| Detail | 0.78 | `text_detail`, `text_subtitle`, `text_empty`, `text_overline`, `text_row_detail`, `text_inert`, `text_hint`, `state_neutral` |
| Note | 0.55 | `text_note`, `text_caption`, `text_count`, `text_path` |
| Inactive (exempt) | 0.40 | `text_inactive` |

**Surfaces and hairlines.** Fills: 0.07 row (`surface_row`, `surface_trophy_row`, `surface_tile_placeholder`), 0.10 control, 0.16
hover (also track, keycap, switch track off), 0.20 focus, 0.26 pressed. Hairlines: 0.08 panel and glass edge, 0.12 control and
card, 0.32 under the pointer. Removed (unused): `hairline`, `selected_category`, `surface_prominent`, `panel_fullscreen`,
`panel_log`, `splash_overlay`.

**Type scale: twelve sizes.** 12, 13, 14, 15, 16, 17, 19, 22, 28, 34, 40, 56. Light only from 28 up; no Bold. A page takes a
`ty::` token by role, it does not build a style.

| Token | Size / weight | Was |
|---|---|---|
| `DISPLAY` (`HERO_TITLE`, `ONBOARDING_TITLE`) | 56 light | 58 and 54 |
| `TITLE` (`SCREEN_TITLE`) | 40 light | |
| `HEADING` (`EMPTY_TITLE`, `SPLASH_TITLE`, `TROPHY_LIST_TITLE`) | 34 light | trophy list 36 |
| `PANEL_TITLE` (`SIDE_PANEL_TITLE`, `LOG_TITLE`) | 28 light | 30 |
| `LANGUAGE_NAME` | 28 medium | |
| `ITEM_TITLE` (`TOP_BAR_TITLE`, `TILE_NAME`, `PERCENT`) | 22 semibold | top bar 24 |
| `STATE_TITLE` (`LIBRARY_COUNT`) | 22 medium | |
| `BUTTON`, `CATEGORY`, `CATEGORY_SELECTED`, `BANNER_NAME` | 19 | |
| `BUTTON_MEDIUM`, `TOAST` (`PROFILE`, `DIAGNOSIS_LOG`), `CHOICE_VALUE` | 16 | |
| `BODY` (`QUESTION`), `ROW_TITLE` (`CLOCK`, `ONBOARDING_STATUS`), `INFO_VALUE` (`TROPHY_NAME`) | 17 regular, medium, semibold | trophy name and onboarding status 18 |
| `LABEL` (`ROW_INFO`, `HINT_LABEL`, `PROGRESS_LABEL`), `FACT` (`PROGRESS_PERCENT`), `SENTENCE` | 15 medium, semibold, regular | |
| `NOTE` (`DETAIL_LARGE`), `CHIP` (`STATE_LABEL`), `SECTION_CAPTION` | 14 regular, medium, semibold | |
| `DETAIL`, `BADGE` (`HINT_KEY`, `KEY_CAP`, `BANNER_LABEL`), `OVERLINE` (tracking 0.8 for all three overlines) | 13 regular, semibold | key cap 13.5; info overline tracking 0.6; onboarding overline 14 |
| `MICRO` (`STATE_LABEL_SMALL`) | 12 medium | |

**Radius: five values.** Keycap 7, row 12, card 16, large 20, capsule (and the toast, 28). `TROPHY_ROW` and `BOX` are 16 (the box was 14),
`LANGUAGE_CARD`, `THEME_CARD` (was 18) and `BANNER` are 20. Removed: `CONSOLE_BUTTON`, `ICON_BUTTON`, `KEYCAP_HINT` (now 7, was 6).

**Spacing: `ui::theme::space`.** 4, 8, 12 (the control gap), 16, 24, 32, 48. Gaps of 6, 10, 14, 18, 20, 26 and 28 are off the grid and
move to the nearest step when a page is touched.

**Rules for the rest** (one source for the area work: `polish028/SYSTEM.md`): one primary button per view and destructive is a
secondary with the danger ink; icons are 14, 17, 18, 20, 24 or 36 dp; the only motion is `Motion::*`; one action has one cue; a
page draws no focus ring of its own; the case rule of 3.10 stands, and the verbs are Cancel, Back, Close, Done, Continue, Try
Again, Remove, Delete, Sign In, Later.

**The hint bar has one place (PR 2 of the round).** Every full-screen area shows it at the bottom right, where Home has it: the right edge
`pad::HOME_X` in, the baseline `pad::HINT_Y` up (`screens::footer_hints`), never in a header. A screen with a Back button on screen
leaves the "Back" hint out (`HintSet::without_back`): the bar never repeats a control the player can see.

**Messages** (tokens for the social area): `color::bubble_other` white 0.10, `bubble_own` 0.16, `composer` 0.10 with the control hairline;
gaps `space::BUBBLE_IN_GROUP` 4, `BUBBLE_BETWEEN` 16, `BUBBLE_DAY` 24. A system row is centred 14 regular in `text_note` with no bubble.

**Motion and avatars (PR 3 of the round).** Hover, press, progress and every reduced-motion ease (focus, layout, overlay, banner slide) use `Bezier::STRONG_OUT` (0.23, 1, 0.32, 1)
at the same durations (120, 100, 350, 120 ms): the answer to a touch is seen at once. The exit of the old hero keeps the softer ease-out it was
tuned with. The initial in the call card and the Play Together card (24 bold before) is `ty::ITEM_TITLE` (22 semibold) in `readable_ink` of the gradient's middle
(`home::avatar_middle`), not white: white on Mint, Gold or White was 1.2 to 2.2:1.

## 4. Materials and blur on the GPU

SwiftUI `.ultraThinMaterial` is a backdrop blur with a light tint and some saturation; it is used under every panel. egui has no
backdrop filter. Recipe:

1. After the theme background and the game artwork have been drawn (everything below the UI), keep the **offscreen backdrop
   texture** (section 3.2, half resolution) as the blur source. Compose the artwork into the same texture (artwork pass first, then
   gradients, see 5.2) so panels blur what the user sees.
2. Produce a **blurred copy** once per frame (or only on frames where the backdrop changed): downsample to quarter of the half-res
   texture (so 1/8 of physical resolution) and run a **two-pass separable Gaussian** (horizontal then vertical, 9 or 13 taps,
   linear-sampling trick for half the taps). Target sigma about **20 dp at the final scale** **(estimate; ultraThin dark is roughly a
   20 to 30 pt blur)**; at 1/8 resolution that is sigma 2.5 px times `scale`.
3. For each glass rect (info cards, toast, panels, splash), draw the blurred texture sampled with **screen-space UVs** of that rect
   (a textured `Mesh` with a rounded-rect clip, or a custom `CallbackTrait` that draws a rounded rect with an SDF mask), then the
   tint layers listed in section 1.2 on top: for "glass" `.ultraThinMaterial.opacity(0.55)` then black `opacity` then the 1 px stroke.
   The material tint itself is close to white 0.04 .. 0.08 over the blur **(estimate)**; tune by eye against S-home (info cards).
4. Full-screen panels (settings, library, trophies, log) and the splash: blurred copy + black 0.74 / 0.65 / 0.55. The home UI
   under them is *not* part of the blur source in the port (the original blurs it too, but under 74 % black it is not visible
   as more than a smudge); simply fade the home UI to 0 opacity while a full-screen overlay is open, or leave it and accept the
   difference.
5. The side panel: gray 0.06 (`Color(white: 0.06)`) at 0.92 alpha, so almost opaque; use the blurred copy plus that fill. Its left edge is a
   1 px line white 0.08; the scrim on the rest of the screen is black 0.35.
6. Performance: the blur copy is tiny (1/8 resolution); the whole chain is a few hundred microseconds. Skip steps 1-2 on frames
   where nothing under the panels changed and no panel is open.

---

## 5. Screens and components

Draw order of the root (`RootView`), back to front: **Backdrop** (shader or user picture/video, game artwork) -> **Home** or
**Onboarding** -> **overlay** (one of settings, library, game-settings side panel family, log, trophies) -> **launch splash** ->
**toast** (top, centred). All overlay state changes run with `Motion.overlay`. App state machine: `onboarded == false` shows only
Onboarding; otherwise Home. Overlays: `settings`, `gameSettings`, `library`, `log`, `confirmUninstall`, `controls`, `trophies`,
`advanced` (the last four plus `gameSettings` render as the side panel, except `trophies`).

### 5.1 Game states and what each looks like

`GameState` for each game, in priority order (first match wins): `starting` (press of Play until its first window), `running`,
`installing(step, fraction)`, `queued`, `failed(message)`, `installed(outdated)`, `unreadable` (executable is neither ELF nor SELF
container), `locked`.

| State | Tile overlay | State label (symbol, tint, text key) | Primary button (symbol, text) |
|---|---|---|---|
| locked | black 0.42 + lock badge bottom-right | lock, white 0.75, `locked` | lock-open, `unlock` |
| unreadable | black 0.42 + octagon-x badge | octagon-x, `#FF998C`, `unreadableDump` | lock-open, `unlock` (press shows toast `unreadableDump`) |
| queued | black 0.42 + clock glyph (size 0.18) white 0.85 centred | clock, white 0.75, `queued` | x, `cancel` |
| installing | black 0.30 + progress line at the bottom | down-arrow-circle, accent, current step title | x, `cancel` |
| failed | black 0.42 + warning badge | warning, `#FF998C`, `failed` | refresh, `retry` |
| installed | none | check-circle, white 0.75, `ready` (outdated: refresh-arrows, `#FFD166`, `updateAvailable`) | play, `play` |
| starting | black 0.42 + small spinner | hourglass, accent, `starting` | hourglass, `starting` (inactive, so not prominent: it only reports) |
| running | accent dot (0.09 size) top-right with glow | play-circle, accent, `running` | play-circle, "Back to Game" (the verb of what a press does: it re-activates the game window; the state line above the title says "Playing") |

While a game runs or starts the hero has no prominent button: nothing there is a call to action. On the home hero the state line is left out for the plain `installed` state ("Ready"): the Play button says it, and every game would repeat it.

Badge (locked/unreadable/failed): glyph size `tile * 0.13` semibold white, padding `tile * 0.08`, behind it a circle black 0.55
inset by `tile * 0.035`, anchored bottom-trailing. Installing line: height `max(3, tile * 0.03)`, padding `tile * 0.07`.

### 5.2 Backdrop and game artwork

- Backdrop = theme shader (section 3) or the user's picture/video.
- If onboarded **and** "Show the selected game's artwork" is on **and** the settings overlay is not open **and** the selected game
  has `sce_sys/pic0.png`: draw the artwork over the backdrop, cross-faded per selection (animation 6):
  - image: aspect-fill the window, decoded to max dimension `max(w, h) * 1.6` px; scale 1.01 -> 1.07 about the **top-right**
    corner, 24 s easeInOut ping-pong (animation 8; static when reduced motion);
  - horizontal scrim (leading to trailing): black 0.78 at 0, black 0.35 at 0.5, clear at 0.85;
  - vertical scrim (top to bottom): black 0.55 at 0, clear at 0.3, clear at 0.55, black 0.85 at 1.
  (S-home, S-unlocking: the artwork darkens toward the left and bottom where the text sits.)

### 5.3 Home screen

Vertical stack, spacing 0: **TopBar**, **TileRow** (or **EmptyLibrary**), flexible space, **HeroInfo**, **HintBar**.

**Top bar** (padding h 64, top 34): left "Games" (24 semibold), a small spinner while scanning; right-aligned, left to right:
library button (glyph: 3x3 grid), captures button (glyph: two pictures), friends button (glyph: two people; see below), settings button (glyph: gear), 12 dp apart; then 32 dp
on, the profile (12 dp, the same gap), then 28 dp on, the clock (HH:mm, local time, 17 medium white 0.78, colon fixed; 3.9). Icon buttons: 44
circle, a secondary control of the family in 3.9. The profile and the friends button are pills of the same
height on the same surface, hover and focus (3.9: a ring and a lighter fill, never white). Everything sits on
one centre line, the clock's digits included. (S-home.)

- **Profile.** Signed in: the avatar (a 36 dp circle with an accent-to-accent-0.55 diagonal gradient and the first letter of the
  account's handle, uppercase, 16 bold white), then the handle (16 medium, white 0.92, cut in the middle past 160 dp). Signed
  out: the `user-circle` outline in the avatar's footprint, in ink, and "Sign In" (16 medium). The PC's user name never shows here;
  it stays a setting (`profile_name`) for what the launcher calls the player locally. Signed in, Select opens the player's own profile screen (section 17, "Profile screen"); signed out it opens the online area on sign-in.
- **Friends.** The friends icon in a 44 dp circle. While friends are online it grows into a pill that ends in a dot in the accent (8 dp,
  8 dp after the glyph) and their number (16 medium, tabular, 6 dp after the dot, 16 dp of padding at the end; `99+` above 99). At
  none, or while signed out, it is the plain circle: a count of zero says nothing. Its name says what it shows: "Friends, 3 online"
  (just "Friends" at none). Select opens the online area, which lands on the friends list once signed in and on sign-in otherwise.
  Keyboard and controller order, left to right: library, friends, settings, profile (`HomeFocus::Top(0..=3)`); up from the tiles
  lands on settings.

**Tile row** (height `176 + 10`, leading 64). All tiles are in one layer; for tile `i` with `offset = i - selected`:

```
leading = 64 + (selected > 0 ? 28 : 0)               // TILE_PEEK_SHIFT: the row moves right past the first game
x = offset < 0 : leading + offset * (118 + 16)       // the game before the selection stays visible, partly past the window edge
    offset == 0: leading
    offset  > 0: leading + 176 + 16 + 300 + (offset - 1) * (118 + 16)     // after the 300-wide label column
y = selected ? 0 : (176 - 118) * 0.18               // small tiles hang 10.44 lower
size = selected ? 176 : 118
opacity = offset == -1 ? 0.4 : offset < -1 ? 0 : 1   // TILE_PEEK_OPACITY for the previous game
```

Selected tile has the focus ring only while focus is on the tiles and no overlay is open. Next to the selected tile: label column
at `x = 64 + 176 + 16 * 1.4 = 262.4`, `y = 176 * 0.32 = 56.3`, width `300 - 16 = 284`: game name (22 semibold, white, 2 lines) and the state
label (14 medium). The label fades in with the selection (animation 5). Tile appearance: placeholder fill white 0.08, the 512 px icon
(`icon0.png`, decoded to <= 400 px) aspect-filled, state overlay (5.1), clipped to the rounded square, shadow per 1.4, focus ring.
A game without icon shows a gamepad glyph (size 0.32 of the tile, white 0.5). Row motion = animations 2 and 3.
Mouse: one click selects and focuses the tiles (sound `move` if the selection changed); double click selects and runs the primary action.

Row sketch (S-home):

```
[ selected 176 ][ name / state  (300) ][ 118 ][ 118 ] [ 118 ] ...        at x = 64, 256, 556, 690, 824
```

**Hero info** (horizontal padding 72, stack spacing 22, transitions animation 4):

1. Title: game name, 58 light white, max 2 lines, shrinks to 60 %.
2. Action row (gap 16): primary button (`Play`/`Unlock`/`Retry`/`Cancel`/...: prominent = white 0.20 fill, min width 210, symbol
   and text as in 5.1); the **trophy chip** if the game has a trophy list (`"unlocked/total"` with the trophy glyph, a 32 high
   chip, 14 medium tabular, vertically centred on the row; it opens the trophies, and it is the only place the count is shown, the
   game settings carry no trophies row); the "..." button (glyph: three dots, icon only), which opens game settings. To the right, depending on state:
   - installing: a 340-wide block (leading pad 10): row with step title (15 medium, white 0.85) and `"NN%"` (15 semibold, tabular)
     where NN = `Int(step.overall(fraction) * 100)`; under it the progress line (accent), or the **indeterminate** sweep while step ==
     relink and fraction == 0. (S-unlocking.)
   - failed: the failure message (14 regular, `#FFB3A6`, max 2 lines, max width 520);
   - otherwise if the last run ended on an error: the diagnosis (warning triangle + one sentence, 14 regular, `#FFC780`, 2 lines,
     max width 520), see 5.13.
   Action row reflows with `Motion.layout` when the state changes.
3. Four **info cards** (gap 14), each 236 x 96, padding 18, glass 16: overline with a glyph (12 semibold uppercase, tracking 0.6,
   white 0.55) over the value (17 semibold white). Cards: **Status** (circle-dashed glyph, the state label), **Size** (disk glyph,
   `"..."` while being measured, else a decimal-unit file size such as `8,39 GB`; measured lazily for the selected game), **Title ID**
   (hash glyph, e.g. `PPSA12544`), **Version** (tag glyph, e.g. `01.000.010`).

**Game page additions** (Windows launcher; the online features show only when signed in):

- **Friends line.** Under the action row, 14 regular white 0.7, one line, 12 below the buttons: `Mira is playing this`,
  `Mira and Sam are playing this`, `3 friends are playing this`. Friends are those whose presence activity has the game's title ID;
  none, or signed out, shows nothing. The gap above the facts is 46 so the line has room.
- **Runs fact.** A fifth fact (after Version) with the overline `RUNS` and the value `Playable · 12 reports`: the most reported
  verdict (a tie goes to the lower one) and the number of players, from the cloud's run-report summary. It shows once at least 3
  players reported, preferring the player's own game version when it has 3 or more, else all versions of the title. Otherwise
  there is no fact.
- **"How did it run?"** One row below the facts (18 below them, 32 high; the row is kept free whether it shows or not, so nothing
  moves): the question in 14 regular white 0.7, four chips `Doesn't start`, `Menus`, `In game`, `Playable` and a round dismiss
  chip with the X glyph (12 between the controls). It appears after a session of the game ended while the player is signed in, on
  the page of that game, as a plain fade-in (`Motion.fade`), without a sound, toast or other movement. Focus: Down from the action row
  enters it (`HomeFocus::Ask`), Left and Right move, Select answers, Up or Back leaves; a press answers and the row goes. It is
  asked once per game version and toolkit version (`Preferences.run_asked`), and an answer or a dismiss both count as asked. The
  answer goes to the net service as `run_report`: title ID, game version, toolkit version and verdict (the service adds the system).
  Signed out, nothing is asked and nothing is sent.

**Hint bar** (right aligned, padding h 64 v 28, item gap 22): `← → ↑ ↓` "Move", `↩ / ✕` "Select", `esc / ○` "Back", `O / △` "Options"
(keys in a keycap: 12 bold, padding 7 x 3, white 0.16 fill, radius 5; label 13 medium; all white 0.72; gap 8). Library, trophies and
onboarding show it without "Options". A hint names only a key that does something: on the home screen "Back" is shown only while the launcher is in front because of the Home button (Back returns to the game) or a Play Together card is up (Back closes it), or the focus is off the tile row (Back returns to it); otherwise the bar is "Select" and "Options". The glyphs are the fixed text above (PlayStation face names), not adapted to the pad in use.

**Empty library** (no games): centred column, gap 18: icon 54 thin (magnifier while scanning, otherwise a stack-slash icon, white
0.8), title (`scanning` or `emptyLibraryTitle`, 34 light), text `emptyLibraryText` (17, white 0.65), the games folder path if set
(13, white 0.45, middle-truncated, 1 line), a focused button `changeFolder` (folder glyph) with 10 extra top gap. Select with no
game opens the folder dialog.

Library behaviour: games are folders (down to depth 3, skipping hidden entries, packages and the names `.AnyPS5 sce_sys sce_module
sce_modules prx out rel`) holding `eboot.bin` and `sce_sys/param.json`; sorted by name case-insensitively. The folder is watched
(root only); a change triggers a rescan if the last scan was more than 2 s ago and no unlock is running; the window regaining focus
rescans if more than 3 s passed and reloads trophies of the selected game.

### 5.4 Unlocking progress

Unlock queues the game (sound `select`); one unlock runs at a time; failures and successes call `startNext`. Steps and their share
of the single overall bar:

| Step | Text key | start | length |
|---|---|---|---|
| unwrap | `stepUnwrap` | 0 | 0.12 |
| relink | `stepRelink` | 0.12 | 0.50 |
| copy | `stepCopy` | 0.62 | 0.26 |
| bundle | `stepBundle` | 0.88 | 0.08 |
| finish | `stepFinish` | 0.96 | 0.04 |

`overall = start + length * fraction`. Progress events are coalesced: the bar only updates when the step changes, the fraction
moved by >= 0.005, or fraction >= 1. The tile shows the small bar, the hero shows step title + percentage + bar. Success: sound
`unlocked`, toast `"<game> · Ready to play"` (lock-open glyph). Failure: state `failed(message)`, toast `"Could not unlock: <game>"`.
Cancel (button or Esc/B while selected): sound `back`. The installer keeps an in-memory log (max 600 lines, trimmed by 100) shown in
the log view. (Screenshot S-unlocking shows step "Converting for Mac", 39 %.)

#### Launcher additions to the unlock (Windows and Linux)

*Weights.* The table above is the default for a PC that has not unlocked before. After every unlock the pipeline reports how many seconds each step took (kept in `Cache\unlock-timings.json`); the next bar gives each step its measured share (a step never gets less than 2 %). On the owner's PC with a 322 MB executable the relink takes about 75 % of the time, so the default is 4 / 78 / 12 / 4 / 2 instead of the table's 12 / 50 / 26 / 8 / 4.

*Time left.* The hero shows the remaining time beside the percentage once 4 % of the bar and 8 s of work lie behind it: half the speed of the last 40 s, half the speed of the whole run, smoothed with a time constant of 8 s, rounded to "Under a minute left", "About 3 min left", "About 2 h left". Time spent standing still does not count; nothing is shown before there is enough to tell.

*Faster.* The dump's files are laid out by up to eight threads (hard links, copy only where the volume cannot link). The relinker starts below normal priority; the executable it made is linked, not copied, into the copy. An unlock again whose relink key (release, flags, patches, size and time of every input) matches the copy it replaces keeps that copy's converted files and skips the relinker. The relinker's own output is not changed.

*Where the copies live.* The store is in the games folder unless the player chose another location (Settings, "Unlocked Games Location"); the space check and the link-or-copy rule apply to the drive of the store.

*Preparing.* A game that appears in the games folder after the launcher first looked at it, and a game left outdated by a toolkit update, is unlocked in the background (setting `prepareGames` "Prepare New Games Automatically", on), one at a time, at idle priority, only when the idle gate is open and no game is starting or running. The first scan only notes what is there; it prepares nothing. A preparation that fails is not tried again with the same toolkit and raises the toast "Could not prepare: <game>"; one that finishes raises "<game> · Ready to play".

- Tile: a band at its foot with "Preparing… 40%" (12 medium) and the progress line; paused "Paused · 40%". The selected game's state line says Preparing or Paused; the hero shows the bar, the percentage and the time left, or one sentence while the work has not started.
- Play or Unlock on a game being prepared moves it to the front (its relinker leaves the idle class, the others stand still) and from then on it shows as an ordinary unlock. Cancel on it stops the preparation for good.
- Starting a game freezes a preparation at once (the relinker is suspended, not killed); it goes on after the game ends.
- Failure sentences say what to do: toolkit missing, drive full, file held by another program, unusable dump, converter stopped; the technical message goes to the log.

### 5.5 Launch splash ("start screen with timer")

Shown from the press of Play until the game's first window appears (poll every 400 ms for a visible top-level window of the game
process, larger than 200 x 150, up to 240 s; if the process dies first the splash closes and the diagnosis runs; if no window ever
appeared the toast `startFailed` is shown). Esc / controller back cancels: the game process is killed, sound `back`. While the
splash is up **all other input is swallowed** except back.

Layout: full-window overlay (blurred backdrop + black 0.55); centred column, gap 24: tile 200 (radius 24, same placeholder rules),
breathing glow (animation 26); game name (34 light, centred, 2 lines); a 380-wide block (gap 12): row `starting` (16 medium, white
0.88) and the elapsed `m:ss` (16 semibold tabular, counting from 0:00, tick 1 s), the indeterminate sweep (accent), and
`startingText` (14, white 0.6, centred, wraps). At the bottom, 40 above the edge, a secondary 44 dp button "Cancel" (X glyph, 240 wide): the pointer's way to the same stop as Esc or the pad's back, which stay. After the window shows,
the splash leaves (overlay spring), the launcher optionally hides ("Hide Launcher While Playing"). (S-starting.)

### 5.6 Toast

Pill at the top centre (top 26): glyph 17 semibold + text 16 medium (max 2 lines); padding 22 x 14; glass radius 28 with black 0.6;
shadow (1.4). 3.2 s on screen, spring in/out (animation 27). Events that raise toasts: unlock ready/failed, unreadable dump, no
toolkit (`noToolkit`), start failed, uninstall done, shader cache cleared, report saved/failed, settings exported/imported/file
invalid, "game: diagnosis".

### 5.7 Settings (global)

Two columns on a black 0.74 blurred panel (S-settings). Left (`size::SETTINGS_CATEGORY_WIDTH` 330, then `space::XXL` before the rows): title `settings` (40 light) with `space::XXL` below, then
eight category rows (height 52, gap `space::S`, radius 12, glyph `size::ROW_ICON` in a `size::ROW_ICON_BOX` box, `space::M` gap, title 19). Selected category: `color::surface` fill, text
semibold; **focused** (keyboard focus in the category list): the focus ring. Unselected text `color::text_detail`, selected `color::text`. Right: the
category's rows (5.8).

**One designed place (0.2.8).** A page is a few *groups* of rows. The first group has no heading: it is the page. Every later group
starts with a caption (`Row::caption`, 14 semibold, quiet ink; not focusable, the pad skips it) after a double row gap, so the gap
between groups is at least twice the gap inside one. A group is a subject, not a release: sound, the pad, the programs the launcher
talks to. Rules for rows:

- Anatomy is always icon, title, optional detail, control. A result (what the last check found) is the **detail** of its command, never
  a gray note under it. Notes (`attached`) stay for a sentence that belongs to the row above (a warning about a folder, Cisco's licence).
- A toggle is named for what happens when it is **on** ("Home Button Opens the Launcher", "Video for Share Screen"). The detail says
  what happens, in one sentence.
- Red (danger ink) is for something that loses data. Quit and removing a toolkit release are ink; the game's
  "Remove Installation" confirmation is the red one.
- The Windows-only rows (clips, links from websites) are built from `SettingsEnv::windows_rows`; the gallery shows them on every system.

| Category (key) | Glyph | Groups and rows (id, kind, key, notes) |
|---|---|---|
| Appearance (`appearance`) | brush | `theme` choice (Cosmic/Aurora/Sunset), `accent` choice (six), `backdrop` choice (`backgroundTheme` / `backgroundImage` / `backgroundVideo`; picking image/video opens a file dialog when no file is set or the kind changed), `file` action `chooseFile` (only when backdrop is not theme; detail = file name), `artwork` toggle `gameArtwork`, on the Mac `glass` toggle (19), `motion` choice (`motionFull` / `motionReduced`) |
| Library (`storage`) | 2x2 grid | `folder` action (detail = path or an em dash; opens the folder dialog), `rescan` action (detail `gamesFound`; sound `select`), then the rows of the unlocked games location ("Unlocked Games Location" below): `unlockedLocation` with its note, `unlockedDefault`, and while copies move `moveCancel`, `moveContinue`, `moveLeave` |
| Language (`language`) | globe | `language` choice (Türkçe, English; shown in their own language). Caption **Fonts**: `fonts` action `fontsFolder` (detail = the picked path, else the built-in Noto set and whether Japanese, Chinese and Korean are included, not downloaded or downloading), `fontsReset` (only when a fonts folder is set), `fontsCjk` command (download the Japanese, Chinese and Korean fonts; only without a picked folder and while they are missing), info `fontsHint` |
| Behavior (`behaviour`) | sliders | *(no heading)* `name` text field `profileName` (live edit), `prepare` toggle (Prepare New Games Automatically), `hide` toggle `hideWhilePlaying`, `hiddenTrophies` toggle (the trophy list's only option, 5.11). **Sound**: `sounds`, `music` (Home Screen Music, 7.3), `gameMusic` (Game Music, 7.3). **Controller**: `vibration` choice (6.8), `homeButton` toggle (6.6), info `controllerHelp` (what to do when the pad does not react). Windows, after the pad: caption **Captures** with the Clips rows (9.8). **Connected Apps**: `discord` toggle (7.4) and its `discordJoin` toggle while the status is on, Windows `links` toggle (9.6), `addToSteam` and `addGamesToSteam` (9.9) |
| Phone as Controller (`phone`) | phone | Windows and Linux addition, section 18. Sits between Behavior and Toolkit. `phone` toggle `phoneRemote` with the phone server's rows, then, while it is on, `ludusctrl` toggle `phoneLudusctrl` (game controller glyph) with the LudusCtrl rows (18, "LudusCtrl"), which take in the browser client's (18, "LudusCtrl browser client": its QR code, link, Copy Link, status, notes, Other Addresses, New Link; it has no switch of its own and runs with LudusCtrl). |
| Toolkit (`toolkit`) | drive | info *Installed Toolkit* (version), notes for a toolkit newer or older than this launcher was tested with and for a glibc that is too old (Linux), `channel` choice (Stable, AnyPlay Engine, Pinned, Local Build) with its detail, `pinned` choice or `toolkitFolder` action for the last two, `whatsNew` action, `install` command while the toolkit is missing or an update waits. Caption **Updates** (Stable and AnyPlay Engine only): `updateCheck` toggle, `checkNow` command whose detail is what the last look found ("Up to date · Last checked 2026-10-22 09:30"). `cpu` choice (CPU Conversion). Caption **Not Used by Any Game** and one `Remove v0.1.2` command (detail: size) for each installed release nothing uses |
| Advanced (`advanced`) | wrench+screwdriver | *(no heading)* `screenshots` action `screenshotsFolder` (detail = path, `~` abbreviated), `showScreenshots` command, info `screenshotHint` (F12). **Diagnostics**: `recordNetwork` toggle. **Share Screen** (Linux, share screen on): `openh264` toggle *Video for Share Screen* (detail: off and what turning it on does, downloading, downloaded, not downloaded yet, the download failed, or what the share worker made of the file) with the info row `openh264Notice` attached under it, Cisco's text unchanged in every language |
| About (`about`) | info circle | info `aboutText`, info `version` (app version), the launcher update rows (9.5), then `aboutWebsite`, `aboutDiscord`; `aboutPrivacy`, `aboutTerms`; info `credits`, info `creditsLudus` attached under it ("LudusCtrl protocol and app by Josip Parlov (@parl0v)"); `quit` command (ink) |

Settings are persisted per user (`Preferences`): `onboarded`, `language` (system default: Turkish if the OS UI language starts with
`tr`, else English), `theme` (Cosmic), `accent` (nil = theme default), `backdrop` (theme), `backdropFile`, `gameArtwork` (true),
`reducedMotion` (false), `showHiddenTrophies` (false), `sounds` (true), `music` (true), `discordPresence` (false, 2026-10-26: a new profile shows no game on Discord until the player turns it on), `hideWhilePlaying` (false), `prepareGames` (true), `profileName` (first word of the user's full name, fallback
"Player"), `gamesFolder`, `unlockedFolder` (nil = next to the games), `unlockedMovingFrom` (the store copies are moved out of, while any are left there), `screenshotsFolder` (nil = `Pictures/AnyPS5`), `fontsFolder`, per-game settings, `lastPlayed`.
Every change saves immediately and re-applies language, motion and sound flags.

#### Unlocked Games Location (Settings > Library)

By default the store (`.AnyPS5`) sits in the games folder, on the dumps' drive, and a copy is made of hard links, so it costs almost no extra space (on exFAT and FAT32 there are no links and it is a full copy). The player can keep the copies on another drive: dumps on a big slow drive, unlocked games on a fast SSD. Stored as `unlockedFolder` (nil = default). The store is then `<folder>\.AnyPS5`, the same layout as before (`Games`, `Work`); the chosen folder itself is never cleaned, only its `.AnyPS5`. Files go in as hard links where the folder happens to be on the dumps' drive and as full copies everywhere else. The dumps are never written to; the launcher's checks write only below the chosen folder.

Rows (after `folder` and `rescan`; the old info row `installedSize` is now the note under the location row):

| State | Rows |
|---|---|
| Default | action `unlockedLocation` "Unlocked Games Location" (drive glyph; detail "Next to your games · 312 GB free"), attached note `installedSize` |
| Another folder | the same row with the path in the detail ("E:\Unlocked · 412 GB free"), attached note "Unlocked games are copied in full into a .AnyPS5 folder here. Your dumps are never changed.", command `unlockedDefault` "Use Default Location" |
| Network drive or share | as above, the note is a warning: "This is a network location. Copying is slower, and games run over the network." Allowed: a share works, it only costs speed. |
| Not there (drive unplugged, share down) | detail "<path> · Not found", warning note "This folder is not available, so its games show as locked. Connect the drive, or choose another folder." Nothing is lost: the setting stays, the games show as locked, Unlock fails with "The folder for unlocked games is not there" (no files are written to the wrong disk), and everything is back when the drive is. |
| Moving | the row is a status (info, detail "Moving 3 of 7 · 42%"), note "Unlocking and playing wait until the move is done. Saves move with their game.", command `moveCancel` "Cancel Move" |
| Move stopped | action row, warning note "3 unlocked games are still in D:\Games\PS5. Unlocking and playing wait until they are moved.", commands `moveContinue` "Continue Moving" and `moveLeave` "Leave Games Where They Are" (detail: they stay in the old location, unused) |

*Free space.* The free space of the location's drive is looked at on a worker thread (never on the UI thread: a share that is down can take seconds to answer), when the page opens and every 5 s while it is open. The unlock's space check (5.4) measures that drive: "needs 41.2 GB, 12.0 GB free on E:\Unlocked", with the note that the files are copied when the drive cannot link to the dump, and the failure sentence "The drive for your unlocked games is full."

*Choosing a folder.* The native folder dialog opens at the current location. The choice is refused, with a toast and nothing stored, when it is not a folder that exists, when it is inside the games folder ("Choose a folder outside your games folder. Your dumps are never written to."), when it is inside any `.AnyPS5` folder, so inside an unlocked copy ("Choose a folder outside the .AnyPS5 folders."), or when a test file cannot be written there ("Cannot write to that folder. Choose another one."). Picking the games folder itself means the default. The location cannot change while a game runs, an unlock runs or a move runs (toast). If a games folder is picked later that is, or contains, the location, the location returns to the default and the copies stay where they were.

*Changing the location moves the copies.* This is the simpler robust of the two options: preparing them again would lose the saves and cost minutes of relinking per game. A background worker moves every unlocked copy from the old store to the new one, one at a time; the row shows "Moving 3 of 7 · 42%". A copy is either whole at the old place or whole at the new one:

- Same drive: one rename of the copy's folder (instant, hard links stay hard links).
- Another drive: the copy is written into `<name>.partial` at the new place, its file count and size are checked against the old tree, the folder is renamed to `<name>`, and only then is the old copy removed. Saves and trophies live inside the copy and travel with it. A copy needs its size plus 256 MB free at the new place, else the move stops with "The new drive is full: ...".
- Cancel stops after the file it is on, removes the `.partial` of that copy and leaves it whole in the old place; copies already moved stay in the new one. The row then offers Continue Moving and Leave Games Where They Are. Leaving forgets the old place: the copies stay on that drive, unused, and can be deleted by hand.
- A killed launcher leaves at worst a `.partial` (swept by the next scan) or an old copy next to an identical new one (removed when the move continues). `unlockedMovingFrom` keeps the old place in the preferences until nothing is left there, and a move cut off by a restart is taken up again once the library has loaded.
- A running game's copy is never moved (it stays, and the move reports it). While copies wait in the old place, Unlock, Prepare and Play are refused with "Your unlocked games are being moved. Try again when that is done." and background preparation waits, so a game can never be unlocked at the new place while its old copy still holds the saves.
- A copy that already exists at the new place is never overwritten unless it is identical to the old one.

*Linux.* The same, with ordinary paths. A mount point that is not mounted exists as an empty folder, so a custom location also needs its `.AnyPS5` to count as there. A network file system (nfs, cifs, smb3, sshfs, 9p) is read from `/proc/mounts` for the network note. On Windows a `\\server\share` path or a mapped drive letter is a network location.


### 5.8 Panel rows (shared by settings, game settings, controls, advanced)

A row has a symbol, a title (17 medium, up to 2 lines, shrinks to 0.85), an optional detail (13, 55 % of the row foreground, up to
2 lines, middle truncation) and a value control on the right; kinds:

| Kind | Control | Mouse | Keyboard |
|---|---|---|---|
| choice | `<  value  >` (chevrons 13 bold in 26 boxes, opacity 0.85, 0.25 when at the end; value 16 semibold, 120..190 wide, shrinks to 0.7) | click chevron = step; click row = next, wraps | left/right step (clamped), select = next (wraps) |
| toggle | switch 50 x 28 (on = accent track, off = white 0.18, knob white) | click row or switch toggles | right = on, left = off, select = toggle |
| stepper | `-  value  +` (tabular 16 semibold, 70 wide) | click -/+ | left/right by step (clamped), select = +step, wraps to min |
| action | trailing chevron-right (14 semibold, 0.6) | click runs it (sound `select`) | select runs it |
| info | static text (15 medium, 0.7) or just the title in white 0.62 when it has no value (section headers, hints) | none | not focusable |
| text | right-aligned text field 200 wide, edits live | click to edit | keys go to the field (6.1) |
| keys | key caps or the capture prompt | click starts capture | select starts capture |
| qr | a QR code on a white card with the row's detail beside it (below) | none | not focusable |
| link | an address in the mono face, in the room the title leaves (below) | none | not focusable |

Row chrome: min height 58, padding h 18, radius 12; fill white 0.07 (focusable) or none (info); **focused**: white fill, black
foreground, glow, scale 1.015. Destructive actions: title `#FF998C`, glyph `#FF8C80` (white-on-focus rule unchanged: black when
focused). Focused row scrolls to the centre of the list (animation 31). List scroll bars are hidden.

**QR code** (2026-10-26, `Row::qr`, `chrome::paint_qr`; LudusCtrl's browser link). A picture on the panel, like a note is
text on it: no row surface, no focus, no hover, so the ring and the pad skip it. The card is 200 x 200 dp (`size::QR_CARD`),
white (`color::qr_card`), radius 12 (`radius::QR_CARD`), standing on the text edge of the panel (the row padding, 18 dp) with 6 dp
above and below. The code is made with `qrcodegen` (MIT), error correction M, and painted as one mesh of black quads
(`color::qr_module`, one per run of dark modules) without feathering: every module is a whole number of physical pixels and the
code is centred on a pixel, so no module blurs into the next and the light margin is at least the standard's four modules
(`size::QR_QUIET`). The longest link (an IPv6 address and port 65535) keeps three pixels a module at the smallest scale (test). The
modules are kept between frames by their text. The row's detail stands 24 dp to the right of the card, vertically centred on it, as
a note (14 regular, `text_note`) at most 340 dp wide. Screen readers hear the title as the name of an image ("QR code of the browser
link"); what it encodes is the link row under it. Black on white is 21:1 on every theme; as a picture it is exempt from the ink
table, and `a_qr_code_is_black_on_white_whatever_the_theme` holds it there.

**Link** (2026-10-26, `Row::link`). An info row (58 dp, no surface, inert title) whose value is an address the player reads off
or types: JetBrains Mono 14 (`size::LINK_MONO`) in `text_path`, right-aligned, taking all the room the title leaves (the title kept
whole on one line, 24 dp between them) and cut in the middle (`text::middle_truncate_font`) when even that is too little, so both the
host and the end of the token stay readable. Screen readers hear "Link: https://...".

**Key cap** (controls): min 30 x 28, radius 7, padding h 9, text 13.5 semibold; fill white 0.12 for keyboard keys and white 0.06 for
mouse/wheel sources (black 0.12 / 0.06 when focused), 1 px stroke at 0.18. No binding: the text `noKey` (15 medium, 0.5). Waiting for
a key: `pressKey` (15 semibold, pulsing, animation 17b).

### 5.9 Game settings (the "..." menu) and the panels of its frame

**No drawers (0.2.8).** The launcher is a console home: every page is a full-screen area, nothing slides in from a side. The panels of a game share one frame (`screens::side`): game settings, controls, advanced, saves, the recommended settings, the toolkit pin, the Home-button panel and the confirmations. The frame is the full-screen area of Friends and Captures (`chrome::fullscreen_panel`: glass and black 0.84, `GlassStyle::game_panel`, a shade darker than the other full-screen panels so the danger ink of a pressed row keeps its ratio; the same enter and exit): the Back button at the top leading corner (`screens::back_button`; clicking it is the Back command) and the hint bar in the footer (`screens::footer_hints`, without "Back"), a content column 720 wide centred under it (title 28 light, subtitle 15 in the text-detail ink, then the rows with their 6 dp inset), nothing else. Every panel that is a list of rows (game settings, advanced, saves, recommended settings, the Home-button panel) runs through one routine, `SideState::show_rows` (drive the commands, draw the frame, apply the pointer, turn Back, key or button, into the panel's own `back` action and play the Close cue); a panel only says its title, subtitle, rows and what a change does. Switching from one panel to another keeps the area on screen and swaps the content. The hint bar is the footer's (3.12) with "Back" left out, since the Back button is on screen. A confirmation (Remove Installation, Close Game, Restore, the missing toolkit release) is not a page of the frame any more: it is the centred card of 5.9.1 over the dimmed page.

#### 5.9.1 The confirmation card (`screens::dialog`, 0.2.8)

Every question that stands alone is one card in the middle of the dimmed page, like a console's dialog: the uninstall question, the restore question, Close Game, the missing toolkit release, Delete in Captures and every question of the online area (Block, Remove Friend, Unsend, Leave Party, Remove from Party, Leave Conversation). One component draws them (`dialog::draw`; `side::draw` hands a `question` view to it), so they look and answer the same.

- **The page behind** is dimmed with black 0.55 (a local constant); the card is glass (`radius::LARGE` 20, black 0.6, the toast's shadow), 560 wide (less on a narrow window: the screen minus 2 x 24), padding 24. The card opens with the overlay spring: a fade and a rise of 12 dp (a fade only under "Reduced"). A click on the dimmed page does nothing, so nothing is answered by accident.
- **Content**, top to bottom with 8 between the lines: the title (28 light, `text`), the sentence (17 regular, `text_label`, leading 1.45; it wraps, in Turkish too), a quiet line in `text_detail` 14 only when a button carries a state (the download under way), then 24 and the buttons. Nothing else.
- **Buttons** are `ConsoleButton` Large (54), 12 apart, in one row filling the card's width when their labels fit and one under another (each the full width) when they do not. Order is SYSTEM.md 3: the primary or destructive answer first, **Cancel last**. The destructive answer is a secondary button in the danger ink, never a fill; only a question with no destructive answer has a primary (the first answer that is not a refusal: Restore, Download v0.1.2). Cancel, Keep Playing and "Follow Global Setting" are secondary.
- **Focus** opens on the safe answer (Cancel, Keep Playing) and on Download where the way on changes nothing. Left and Up go to the previous button, Right and Down to the next (a bump at the ends), Select (Enter, A) runs the focused one, Back (Esc, B) cancels. The ring shows only for keys and the pad. The card brings no hint bar: Cancel is on screen, so "Back" is not repeated (SYSTEM.md 13), and Select is the button in focus.
- Gallery: `confirm` (uninstall), `restore`, `toolkit-missing`, `captures-confirm` and `friends-confirm*`.

Opened by "..." or `O` / Y / Menu / Options. Title = game name, subtitle `gameSettings`, rows below. (S-game-settings.) Rows
in order, with the condition for each:

| # | Id | Kind | Key / content |
|---|---|---|---|
| 1 | `resolution` | choice | `resolution`; options `[resolutionDefault] + the title's patch choices`; only shown when the title has a resolution patch (e.g. Stray) |
| 2 | `display` | choice | `display`: `windowed` / `fullscreen` |
| 3 | `size` | stepper | `windowSize` `"NN%"`, 40..100 step 5, default 75; only when windowed |
| 4 | `vsync` | toggle | `vsync`, default on |
| 5 | `controls` | action | `controls` -> controls panel |
| 6 | `trophies` | action | `trophies`, detail `"8/34 · 8%"`; only if the game has trophies |
| 7 | `advanced` | action | `advanced`, detail `advancedDetail` |
| 8 | `overlay` | choice | `overlay` (performance overlay): Off / Top Left / Top Right / Bottom Left / Bottom Right (`overlayOff` ...) |
| 9 | `language` | choice | `gameLanguage`: `followLauncher` + 18 console languages (below) |
| 10 | `reinstall` | action | `reinstall` (closes the panel, unlocks again); only when installed |
| 11 | `finder` | action | `showInFinder` (Windows: show in Explorer) |
| 12 | `log` | action | `openLog` -> log view (5.12) |
| 13 | `report` | action | `makeReport`, detail `makeReportDetail` (5.13) |
| 14 | `cache` | action | `clearShaderCache` (toast `cacheCleared`) |
| 15 | `uninstall` | action, destructive | `uninstall` -> confirm panel; only when installed |

**Groups (0.2.8).** The rows stand in three groups, each under a section caption like Settings (`ty::SECTION_CAPTION`, a double gap above it): **Screen** (`resolution`, `display`, `size`, `vsync`), **Play** (`recommended` when the catalogue has something, `controls`, `saves`, `advanced`, `overlay`, `language`, `toolkit`) and **Maintenance** (`reinstall`, `finder`, `log`, `upstream`, `cache`). `uninstall` stands alone at the foot with no caption. Captions are not focus stops. `cache` uses the broom glyph (`Icon::BROOM`); the sparkle stays with the recommended settings and What's New. TR: Ekran, Oyun, Bakım.

Console languages (number, name): 19 Türkçe, 1 English (US), 18 English (UK), 4 Deutsch, 2 Français, 3 Español, 20 Español
(Latinoamérica), 5 Italiano, 17 Português (Brasil), 7 Português, 8 Русский, 16 Polski, 6 Nederlands, 0 日本語, 9 한국어, 11 简体中文,
10 繁體中文, 21 العربية. Launcher language maps to 19 (Turkish) or 1 (English) when the game follows the launcher.

Per-game settings (defaults): resolution nil, fullscreen false, windowSize 75, vsync true, overlay off, language nil, controls nil
(layout changes), tuning nil, shaderCache nil (= on). Passed to the game as environment variables at start:
`ANYPS5_PRESENT_MODE` (`fifo` / `immediate`), `ANYPS5_LANGUAGE`, `ANYPS5_OVERLAY=1` + `ANYPS5_OVERLAY_POS` (`tl`/`tr`/`bl`/`br`),
`ANYPS5_INPUT_CONFIG`, `ANYPS5_SCREENSHOTS`, `ANYPS5_SYSTEM_FONTS`, `ANYPS5_NO_SHADER_CACHE=1`. Upstream reads none of
`ANYPS5_FULLSCREEN`, `ANYPS5_WINDOW_SIZE` and `APS5_RESOLUTION` (they are in no list of upstream's `docs/dev/ENVIRONMENT.md`): fullscreen
and window size are the launcher's own window logic, the resolution is a patch. `core::compat` knows from which toolkit release each
variable and input line is read; what an older toolkit lacks is left out, and `ANYPS5_SCREENSHOTS` needs a build newer than v0.1.3.

**Confirm uninstall card** (5.9.1): title `uninstall`, subtitle `uninstallConfirm`; two rows: `uninstall` (destructive, trash glyph; its label repeats the title, "Remove Installation") and
`cancel`, which holds the focus when the panel opens. The line under the title is `uninstallConfirm` followed by `uninstallBacksUp`.

**Controls panel** (S-controls): subtitle `controls`; first row is an info hint (`controlsHint`, not focusable), then one `keys` row
per pad action in this order with titles: `✕ Cross`, `○ Circle`, `□ Square`, `△ Triangle`, `L1`, `R1`, `L2`, `R2`, `L3`, `R3`,
`Options`, `Touchpad (Left)`, `Touchpad (Right)`, `D-pad · Up/Down/Left/Right`, `Left Stick · Up/Down/Left/Right`,
`Right Stick · Up/Down/Left/Right`, `Slow Movement`; last row `resetControls` (refresh glyph). Opens focused on row 1 (first key).
Default layout (keys shown as in key caps): Cross K, Space, Return; Circle L; Square J, Left Click; Triangle I; L1 Q; R1 E; L2 U,
Mouse 4; R2 O, Right Click; L3 Left Shift; R3 Left Ctrl; Options Esc; Touch Left Backspace; Touch Right Tab; D-pad arrow keys (Up
also Wheel Up, Down also Wheel Down); Left stick W A S D; Right stick T F G H (up/left/down/right = T/F/G/H); Slow Movement unbound.
Capture rules: select starts waiting (sound `select`); Esc cancels (sound `back`); any other key (modifiers count when pressed)
replaces the row's keyboard keys, keeps its mouse sources, and **removes that key from any other action**; unmapped keys are
ignored. Resulting changes equal to the default layout are dropped. Key names are SDL scancode names (section 6.5). "Unbound" is
written as `KEY:F24` for the input file. Back/Options return to the game settings (on the `controls` row).

**Advanced panel** (S-advanced): subtitle `advancedTitle`; opens focused on row 1. Section headers are info rows. Steppers (id, key,
range, step, display, default): leftDeadzone `leftDeadzone` 0..90 step 2 `NN%` 0; rightDeadzone 0..90/2 `%` 0; mouse section
`sectionMouse`: sensitivity 1..100/1 `N.N×` (value/10) 10, vertical 1..100/1 `×` 10, curve 2..50/1 `N.N` 10, minimum tilt
(`mouseMinimum`) 0..90/2 `%` 20, max speed 1..100/1 `×` 10, smoothing 0..500/5 `N ms` or `Off` at 0, 10, decay (`mouseDecay`)
5..1000/5 `N ms` 30; keyboard section: stickModifierScale 5..100/5 `%` 50, keyboardRamp 0..2000/50 `ms`/Off 0, triggerRamp 0..2000/50
`ms`/Off 0; graphics section: `shaderCache` toggle (default on); sharing section: `exportSettings`, `importSettings` (file dialogs,
JSON, name `"<game> - AnyPlay.json"`), `resetAdvanced`. Each stepper row has a detail line (`...Detail` keys). Only changed values
are written to the game's input file; the whole group resets to `nil` when equal to defaults. Glyphs: left/right joystick, mouse,
up-down arrows, curve, scope, gauge, waveform, timer, tortoise, keyboard, trigger, stack, upload, download, rotate.

### 5.10 Library grid

Full-screen overlay over the home (black 0.74 blurred): header `library` (40 light) + count (22 medium, 0.5, 8 leading) + hint bar
without options; below, a scrolling grid of **196** tiles (gap 26 x 30) with as many columns as fit the width, at least three (`columns_for`; up and down move by that row). Each cell: tile with focus ring on the
keyboard-selected one, name (15; semibold white when focused, else regular 0.75; 1 line, truncated), state label (12). Click =
select that game and close. Keys: arrows move (up/down jump a row of 6), select picks, back/options close. The focused tile scrolls
to the centre (animation 31). The plain "Ready" is not drawn under a tile (it stays in the accessible name): every game would repeat it, and it would hide the one that differs ("Playing", "Update available"). With no games the grid says what the empty home says ("No games yet" and the line about the games folder, no count) and offers the same one button (Pick or Change Folder; Select does the same; scene `library-empty`). (S-library. The tile shown "Playing" uses the coral accent.)

### 5.11 Trophy list and trophy button

Home: the trophy button between Play and "..." appears only for games with a trophy list: label `"8/34"`. The list overlay (S-trophies):
black 0.74 blurred; header row (gap 26): set icon 112, then a column (gap 12): overline `trophies` (13 semibold uppercase tracking
0.8, 0.55), title (set title or game name, 36 light, 1 line), a row (gap 16): percent `"8%"` (22 semibold tabular), progress bar
260 wide (accent), and per grade with at least one trophy a count `"unlocked/total"` (trophy glyph in the grade colour + 15 semibold
tabular, white 0.85); hint bar (no options) right, the title giving way to its measured width. A set with no trophies says "This game has no trophies." in the middle (scene `trophies-empty`). Below, a scrolling list (gap 10, padding 8) of trophy rows in package order:

```
[ icon 68 ]  Name (18 semibold)                                     [trophy] Bronze (14 semibold, grade colour)
             Description (14, 0.6, 2 lines)                          Oct 17, 2026 at 9:14 PM  (13, 0.7)  |  Not earned (13, 0.4)
             [progress 180x4] 23/51  (not earned, has progress)
```

Earned: icon in colour, name white; not earned: icon in greyscale at 0.45 opacity, name white 0.8, grade colour at 0.55. Secret
(hidden and not earned, not revealed, and "Always Show Hidden Trophies" off): a lock tile (white 0.07, lock glyph 22 semibold, 0.5)
instead of the icon, name `trophyHidden`, description `trophyHiddenDetail`, no progress.
**Reveal (launcher addition, local only):** the focused secret shows a "Show" pill (`trophyShow`, 15 semibold, 34 high, 18 side
padding, radius 12, black 0.08 fill and black 0.9 ink on the white card, black 0.14 on hover, 0.98 on press) left of the grade
column, 26 from it; the click area reaches 5 past the pill. Click on the pill or select (Enter, Space, the confirm button)
reveals the trophy until the list closes: the lock tile and the placeholder words fade out and the greyscale icon, name and
description fade in (`Motion.focus`, opacity only; reduced motion follows the usual rule). A revealed trophy reads like any
unearned one (no "Show"). Every secret keeps the pill's room in its text column, so focus never reflows it. The setting
`showHiddenTrophies` (5.7, Behaviour; default off) shows all of them from the start. Nothing is sent anywhere; the cloud and friends
see the same as before. Select on any other row does nothing. The focused row: fill white 0.13 (else 0.05) and focus ring width 2. Dates use the
launcher language's locale, medium date + short time (`Oct 17, 2026 at 9:14 PM`, Turkish similar with the Turkish month name **(estimate)**). Keys: up/down move
(sound `move`), back/options close; select reveals a secret (sound `select`), otherwise it and left/right do nothing. Clicking a row focuses it.

Trophy list and texts come from `sce_sys/trophy2/trophyNN.ucp` of the dump (language = the game's language setting, else the
launcher's) and the unlock records the game writes; both read in `anyps5-core` (logic, not UI).

### 5.12 Log view

Full-screen overlay (black 0.65 blurred, padding 70, gap 18). Header: `"Log"` + `" · <game>"` (30 light), right: button
`makeReport` (stethoscope glyph, unfocused) and button `close` (x glyph, **focused**). If the game has a diagnosis: a glass box
(radius 14, black 0.5) with the warning triangle + sentence (16 medium, `#FFC780`). Then a scrolling, selectable text box (glass 14,
black 0.55, padding 18) with the log in 12 mono, white 0.85, **scrolled to the bottom** on open. Content = in-memory install log
lines, then the failure message if any, then the last 60 000 bytes of the game's log file; `"—"` if empty. Keys: back and select
close it; everything else is swallowed.

### 5.13 Error and diagnosis presentation, report

After a game exits (process watcher), wait 1.5 s for crash data, read the log of the last start (text after the last
`[anyps5-launcher] start` marker, which the launcher appends before each start) and the newest crash report since the start, and
classify. Presentation: a toast `"<game>: <sentence>"` (warning glyph), the amber diagnosis line in the hero (5.3) and in the log.
If the game never opened a window and no diagnosis was found: toast `startFailed`.

| Diagnosis | Sentence key (arg) | Detected from (original, macOS) |
|---|---|---|
| missing module | `diagnosisMissingModule` (file name) | "Library not loaded: @rpath/X" |
| unsupported function | `diagnosisUnsupported` (function) | engine exception "`<fn> not implemented`" |
| GPU lost | `diagnosisGpuLost` | `VK_ERROR_DEVICE_LOST` / "device lost" |
| out of memory | `diagnosisOutOfMemory` | `VK_ERROR_OUT_OF_DEVICE_MEMORY`, `std::bad_alloc`, "out of memory" |
| engine error | `diagnosisEngineError` (message, cut at 200 chars + ellipsis) | any other uncaught engine exception |
| crashed | `diagnosisCrashed` (exception) | the OS crash report, e.g. `EXC_BAD_ACCESS (SIGSEGV)` |

**Every sentence names a way out (0.2.8, `ui/i18n.rs` `ours`).** GPU lost: update the graphics driver, start again. Out of
memory: close other programs, start again. Engine error and crash: start again, and if it repeats send a report from Options > Log
(the part that varies in length, the function, the engine message or the exception, goes last so the step stays visible when the hero line clips at two lines). Unsupported function: send a
report so it can be added. A CPU that lacks an instruction set is a fact about the PC and has no step. `reportFailed` says
"Check that the disk has space, then try again." (a test in `screens/log.rs` checks that each sentence has a step).

**Report**: "Make a Report" writes a text file (`<titleID>-yyyyMMdd-HHmmss.txt` in the Reports folder) with: title line + ISO date,
Game (name, title ID, version), machine (model, CPU, RAM GB), OS version, launcher version, toolkit version, Reason (sentence or
"none found"), Settings (pretty JSON), the crash exception line if any, and the log of the last start (<= 400 KB); then reveals it in
the file manager and toasts `reportSaved` (doc glyph) or `reportFailed`.

### 5.14 Trophy banner

A separate always-on-top, click-through, non-focus-taking surface that must also appear over a game window. Size **560 x 150**,
centred horizontally at the top of the primary display (below the menu bar / safe area). Unscaled (dp = points). Contents (S-banner):

```
card: width 480, padding 14, radius 20, fill white 0.07 @ 0.94, 1 px stroke gradient top-to-bottom (grade colour @ 0.55 -> white 0.08), shadow black 0.5 r18 y8
 row, gap 16:
   [glow circle 64: grade colour 0.35, blur sigma 14, opacity animates] [trophy icon 60, radius 10.8, scale 0.6 -> 1]
   column gap 4:  "Trophy earned" (trophy glyph + 13 semibold, grade colour)
                  name (19 semibold white, 1 line; "Hidden Trophy" if empty)
                  description (13, white 0.6, 1 line)
   spacer, big trophy glyph 26 semibold with a vertical gradient grade colour -> grade colour 0.6, trailing pad 6
```

Timeline: spawn -> slide-in spring (offset y -150 -> 14, opacity 0 -> 1, spring 0.55/0.78; easeOut 0.25 in reduced motion) and
simultaneously start the icon pop 0.18 s later (spring 0.45/0.55); sound `unlocked` (when sounds are on); hold 5 s; easeIn 0.3 s out;
0.35 s later the next queued banner starts (banners are strictly queued, one at a time). The running game is watched every second
for new trophy records; each new one raises a banner and refreshes the trophy set.

### 5.15 Onboarding (first start)

Three steps, four when there is a cloud to sign in to (release builds name AnyPlay’s); left-aligned column on a left-to-right black 0.55 -> 0.1 gradient over the (live) theme backdrop; padding 110 x 80.
Top: one dot per step (current 46 x 5, others 18 x 5; white 0.95 for steps up to the current one, else 0.22; gap 10; 54 below).
Then the step (animation 18), flexible space, bottom row: `back` button (chevron-left glyph, unfocused, only from step 1) left,
hint bar (no options) right. Title block: title 54 light, subtitle 19 white 0.7 (max 720 wide), gap 14; step gap 46.

| Step | Title / subtitle | Content |
|---|---|---|
| 0 | `welcome` / `welcomeSubtitle` | overline `chooseLanguage`; two cards 280 x 150 (radius 20, padding 26, bottom-left aligned): `TR` or `EN` (15 bold, 0.6) over the language's own name (28 medium). Focused: white fill, black text, scale 1.04, glow; else white 0.10 + 1 px white 0.14 |
| 1 | `chooseTheme` / `chooseThemeSubtitle` | three theme cards (S-themes): live or still preview 330 x 186 (radius 18, focus ring) above a row: theme name (22) and a 12 dp dot in the theme's default accent; focused card scale 1.04 / opacity 1, others 0.97 / 0.72 |
| 2 | `chooseFolder` / `chooseFolderSubtitle` | button `pickFolder` (or `changeFolder` once chosen; folder glyph, min 220), and once a folder is set the prominent button `start` (arrow glyph, min 180); a glass card (radius 16, padding 20, max 640) with a status glyph (magnifier while scanning, question-folder if empty, check in accent) + `scanning` / `noGamesFound` / `gamesFound` (18 medium) over the path (13, 0.5, middle truncated); the first 8 games as 84 tiles |
| 3 | `Play together` / signing in (`OnboardingText`) | only with a cloud: one row of buttons, Sign In (prominent, sign-in glyph), Create Account (user-plus glyph) and Not Now (arrow glyph), min 180 each, gap 12; under them a detail line (13, 0.6): you can sign in any time from the top bar |

Our own wording replaces the generated table’s for the welcome and folder subtitles, the empty library and the About text
(`ui/i18n.rs` `ours`): they speak of the player’s games and game dumps, never of a platform’s name.

The **language and theme choices apply live** while moving the focus (left/right change `preferences.language` / `preferences.theme`
immediately, so the page re-renders in that language and the backdrop switches theme); select confirms and advances (sound
`select`); Back (Esc/B/button) returns (sound `back`). Step 2 focus: 0 = the folder button, 1 = Start (only once a folder is set; focus
jumps to Start after picking); select on Start goes on to the online step when there is a cloud, and otherwise finishes: sound `unlocked`, cross-fade
(animation 19), focus on the tiles, rescan if empty. Step 3 focus: 0 Sign In, 1 Create Account, 2 Not Now. Under the three buttons one line of 13 dp text (`screens::legal`): "By creating an account you agree to the Terms of Service and the Privacy Policy." The two names are links (2026-10-26; underlined, brighter on hover, pointing hand; `https://anyplaynet.com/terms` and `/privacy`, the About pages' addresses in `about_strings.rs`), wrapped at 800 dp, the "You can sign in any time" line under it. The sentence is a template with `{terms}` and `{privacy}` so a language can put the names where its grammar wants them. The links are for the pointer; the pad reaches the same two pages as rows on the Friends screen's Start page. Every choice
finishes the first start; Sign In and Create Account also open the online area, where the browser sign-in starts as soon as
the net service has said hello (a missing service drops it). Mouse: tapping a language card selects and confirms; tapping a theme card selects it (sound `move`).

### 5.16 Icon mapping (SF Symbols -> Phosphor, MIT)

| Original SF Symbol | Phosphor |
|---|---|
| `square.grid.3x3.fill` | `squares-four` (or `dots-nine`) |
| `gearshape.fill` | `gear-six` |
| `gamecontroller.fill` | `game-controller` |
| `lock.fill` / `lock.open.fill` | `lock` / `lock-open` (fill weight) |
| `xmark.octagon.fill` | `x-circle` fill (Phosphor has no octagon) |
| `exclamationmark.triangle.fill` | `warning` fill |
| `clock` | `clock` |
| `arrow.down.circle.fill` | `arrow-circle-down` fill |
| `checkmark.circle.fill` | `check-circle` fill |
| `arrow.triangle.2.circlepath` / `arrow.clockwise` | `arrows-clockwise` / `arrow-clockwise` |
| `hourglass` | `hourglass` |
| `play.fill` / `play.circle.fill` / `play.circle` | `play` fill / `play-circle` fill / `play-circle` |
| `xmark` | `x` |
| `trophy.fill` / `trophy` | `trophy` fill / regular |
| `ellipsis` | `dots-three` |
| `circle.dashed`, `internaldrive`, `number`, `tag` | `circle-dashed`, `hard-drive`, `hash`, `tag` |
| `folder`, `magnifyingglass`, `square.stack.3d.up.slash` | `folder`, `magnifying-glass`, `stack` (+ slash overlay) |
| `chevron.left/right`, `minus`, `plus` | `caret-left/right`, `minus`, `plus` |
| `stethoscope`, `doc.text`, `doc.badge.checkmark` | `stethoscope`, `file-text`, `file-text` (no `file-check` in Phosphor) |
| category glyphs: `paintbrush`, `square.grid.2x2`, `globe`, `slider.horizontal.3`, `wrench.and.screwdriver`, `info.circle` | `paint-brush`, `squares-four`, `globe`, `sliders-horizontal`, `wrench`, `info` |
| row glyphs: `paintpalette`, `circle.lefthalf.filled`, `photo.on.rectangle`, `photo`, `wind`, `person.crop.circle`, `speaker.wave.2`, `eye.slash`, `power` (+ Home Screen Music, 7.3) | `palette`, `circle-half`, `images`, `image`, `wind`, `user-circle`, `speaker-high`, `eye-slash`, `power` (+ `music-notes`) |
| game-settings glyphs: `rectangle.on.rectangle`, `macwindow`, `arrow.up.left.and.arrow.down.right`, `waveform.path`, `keyboard`, `slider.horizontal.below.rectangle`, `speedometer`, `character.bubble`, `sparkles`, `trash` | `rectangle`/`frame-corners`, `app-window`, `arrows-out-simple`, `wave-sine`, `keyboard`, `sliders`, `gauge`, `translate`, `sparkle`, `trash` |
| advanced glyphs: `l.joystick`, `r.joystick`, `computermouse`, `arrow.up.and.down`, `chart.line.uptrend.xyaxis`, `scope`, `timer`, `tortoise`, `l2.rectangle.roundedtop`, `square.stack.3d.up`, `square.and.arrow.up/down`, `arrow.counterclockwise` | `game-controller` (stick), `mouse`, `arrows-down-up`, `chart-line-up`, `crosshair`, `timer`, `person-simple-walk` (or any slow glyph), `trigger`/`rectangle`, `stack`, `export`/`download-simple`, `arrow-counter-clockwise` |
| controls: `circle.grid.cross`, `l.rectangle.roundedbottom`, `l.joystick.press.down`, `line.3.horizontal`, `hand.point.up.left`, `dpad` | `game-controller`, `rectangle`, `circle`, `list`, `hand-pointing`, `arrows-out-cardinal` |
| `folder`, `camera`, `photo.on.rectangle.angled`, `textformat`, `xmark.circle` | `folder`, `camera`, `images`, `text-aa`, `x-circle` |

Exact Phosphor names must be checked against the bundled icon set version; the table gives the intent.

### 5.17 Layouts and the game's own keys (Windows addition)

The Controls panel opens on a **Layout** choice above the key rows (a double gap below it):

- **Launcher**: the default layout (6.5), no changes stored.
- **Mouse and Keyboard**: the usual PC action layout. Space ✕, C ○, F □, E △, Q L1, R R1, right and
  left click L2 and R2, Shift L3, V or middle click R3, Tab and M the touchpad halves, 1 to 4 the
  D-pad, the arrows the right stick, Left Alt the stick modifier; WASD stays.
- **From the Game**: only offered when the game's files name PC keys (see below). Its detail line
  says for how many buttons.
- **Custom**: shown, as an extra last option, once the user has moved a key; choosing it changes
  nothing.

Choosing a layout replaces the stored changes at once; single keys are still moved by capture.

**Reading the game's files** (`anyps5_core::gamefiles`, off the frame, once per game and run):

1. Unreal paks (v8 to v11, unencrypted index; raw, Oodle or Zlib entries): the project's
   `Config/DefaultInput.ini`, `ActionMappings` and `AxisMappings`. Gamepad keys become PS5 buttons;
   PC keys (no modifier chords) become sources. Axes split into one action per stick direction.
2. When that ini says Enhanced Input: the `IMC_*` assets in the IoStore containers beside the paks,
   decoded without a schema (UE 5.3+ zen header, unversioned properties). Gamepad and keyboard
   contexts pair through the Input Action both name, without `IA_` and device prefixes. Contexts
   of other platforms and of tests are skipped. An unknown layout fails the asset, never yields
   wrong keys.

**From the Game** gives each key the game names to its action's button (first action wins, three
per button) and takes it from every other button; a button keeps its own other keys after the
game's. The walking keys (left stick) stay unless the game itself puts keys on the left stick, and
a 2D axis's keys stay (their direction is unknown).

Each key row shows what the game calls the button as its detail ("Jump", "Ranged Attack";
words split, the minus half of an axis marked "−", three names at most).

### 5.18 Layout files and the community gallery (Windows addition)

**The file format is the toolkit's own.** `anyps5-input.ini` is the file that upstream's
`INPUT_MAPPING.md` (release asset of v0.1.1 and later) describes; the launcher writes a subset of it.
A layout file is such a file, nothing else: `Action = KEY:Name | MOUSE:Left|Middle|Right|X1|X2 |
WHEEL:Up|Down`, one line per source, `#` or `;` at the start of a line for a comment, tuning lines
`Setting = Value`. `anyps5_core::controls` reads it as the engine does (`Controls::parse`): names
without regard to case, the first line of an action replaces its keys and later ones add, repeats
are dropped, UTF-8 with or without a byte-order mark. An action the file leaves out gets the engine's
built-in keys (`upstream_default_sources`), not the launcher's, because that is what the engine does with
the same file. `KEY:F24` alone stands for "no key". The first line the engine would refuse is the
error: "Line 7: unknown action or setting "Jump"." Keys must be names the launcher's scan code
table knows (F13 to F24 besides); `ToggleFullscreen`, `TogglePerformanceOverlay` and `Screenshot`
take keyboard keys only.

- Actions without a row here (`Shake`, `ToggleMouse`, `ToggleFullscreen`,
  `TogglePerformanceOverlay`, `Screenshot`) are kept in the game's changes by name and written back, so a
  shared layout survives a round trip. Importing one that has them says so in the toast.
- Tuning lines are optional. A file with at least one replaces the game's tuning (the other values
  are the defaults); a file with none leaves the tuning alone. Values finer than the launcher's
  steps (tenths, percent) are rounded.
- **Writing.** Only values that differ from the defaults are written (as before). `StickModifier`,
  `KeyboardRampMs` and `TriggerRampMs` exist from toolkit v0.1.3 on, and a line the toolkit does not know
  stops its input. `writes_v013_lines(toolkit)` is the one place that knows; an older toolkit gets
  none of the three. An unbound `StickModifier` is never written (it has no built-in key).

**Export Layout** and **Import Layout** are two command rows after the key rows, with Reset to
Default Keys after them (import and export one group, reset its own, a double gap above each). Both use the native dialogs. The export
lists every action of the launcher, so no omission can mean a different key, under two comment
lines naming the game and the format. Import replaces the game's layout at once and answers with a
toast.

**The community gallery** is a folder of this repository, read at runtime:

```
community/layouts/index.json
community/layouts/<TITLEID>/<name>.ini
```

```json
{ "version": 1,
  "layouts": [ { "titleId": "PPSA16064", "name": "Keys from the game", "author": "AlxcBTW",
                 "description": "One sentence.", "file": "PPSA16064/from-the-game.ini" } ] }
```

The launcher fetches `index.json` from
`https://raw.githubusercontent.com/anyplaynet/launcher/main/community/layouts` once per run and
title, when the controls open, and then the files of that title (8 at most, 16 KiB each, 8 s for
all). An entry is valid when the title ID is four capitals and five digits, the name has 1 to 40
characters, and the file is `<folder>/<name>.ini` of letters, digits, `.`, `_` and `-`. A file that fails
`Controls::parse` is left out. A newer `version` than 1, no network, no entry for the title: the
gallery is empty and nothing shows. Nothing but those GETs is sent.

The Layout choice lists them after the presets and the game's own keys as "Community: <name>";
choosing one is choosing a preset (its tuning lines come along). `community/layouts/PPSA16064/from-the-game.ini`
is what "From the Game" makes of the Minecraft Dungeons II keys that `gamefiles` reads from its IoStore
containers; a test keeps the file equal to that output.

### 5.19 Report to AnyPS5 (Windows addition)

A row of the game sheet after "Make a Report" (which still writes the file). It builds the report of
section 5.13 on a worker thread and shows the text that will be sent in the log view (the same
full-screen panel: a sentence above it, "Cancel" and "Open on GitHub" on the right, Select opens). Nothing
is sent from the launcher and there is no token: "Open on GitHub" opens the browser on
`https://github.com/boykopovar/AnyPS5/issues/new?template=bug_report.yml` with the form's fields in
the query (GitHub fills an issue form from the field ids `summary`, `commit`, `system`, `steps`, `log`,
`notes`; `title` is the issue's title), and the player presses GitHub's own button. Upstream asks for
game bring-up progress in Discussions and for bugs in the bug form; this is the bug form.

| Field | Content |
|---|---|
| title | `<game> (<title id>) <content version>: <diagnosis>` (the form's own "Title" field has the same id and gets the same text) |
| summary | the diagnosis sentence; empty when none was found |
| commit | `AnyPS5 toolkit <version>, launcher <version>` |
| system | Windows build, each GPU with its driver (`CheckInterfaceSupport`; NVIDIA also as the number of the download page) |
| steps | how it was started, and the game's settings that are not at their defaults |
| log | the last 30 lines, 2,000 characters at most |
| notes | only when the report is long: "paste the full report here" |

**Limits.** GitHub answers a link of about 7,000 characters with an error and one of 9,000 with
414 (measured), so the link stays within 6,000 after percent-encoding (`issue::MAX_URL_BYTES`). The
report is built in four tiers of detail (30, 8, 3 and 0 log lines); the first that fits is used.
From the second tier on the full report text (the file's text, the log's last 400 KiB) is copied to the
clipboard when the player continues, the notes field says to paste it, and the review says so. The
player's account name is replaced by `<user>` in everything that leaves. A browser that does not open
leaves the link on the clipboard.

---

### 5.20 The glibc the libraries need (Linux)

The relinker and the `.prx` libraries are ELF files that link against glibc. Their `.gnu.version_r` sections list the symbol versions they need (`GLIBC_2.38`); the newest over all of them is the oldest glibc the toolkit runs on (`anyps5-core::glibc`, reads section headers only, every offset bounded, 32- and 64-bit little-endian; `GLIBC_PRIVATE` and other families such as `GLIBCXX` do not count). The launcher compares it with `gnu_get_libc_version()` once when the toolkit is located. The idea of such a check came from the macOS and desktop launchers of the AnyPS5 community; the design and code here are our own.

When the system is older it says one sentence, the same in three places: "Games need glibc 2.38 or newer; this system has 2.35. Update to Fedora 39, Ubuntu 24.04 or newer." (the next step, and the install-guide row in Settings, are from round 3, 3.10) A note under the version row in Settings > Toolkit (like the "newer than tested" note, gallery scene `toolkit-glibc`), and a warning to the right of the buttons of the hero, in the status place a failure or a diagnosis uses (one status at a time: a failure, then this, then the last run's diagnosis), in the states where that place is free and a start is ahead: Locked, Queued and Installed (scenes `home-glibc`, `home-glibc-locked`); installing, preparing and failed show their own status there, and a failed unlock names its own cause. The friends line under the buttons keeps its place. It is a notice and never a block: Unlock and Play work as before. Nothing is shown when the system is new enough, when a version cannot be read, on Windows and on macOS.

## 6. Input

Original: `Input.swift`, `AppModel.handle`, `panelCommand`, `onboardingCommand`. One handler consumes seven abstract **commands**:
`left right up down select back options`.

### 6.1 Keyboard

| Key (original macOS) | Command | Windows / egui key |
|---|---|---|
| Left / Right / Up / Down arrows | left / right / up / down | `ArrowLeft` .. `ArrowDown` |
| Return, keypad Enter, Space | select | `Enter`, `NumpadEnter`, `Space` |
| Esc, Delete (backspace key) | back | `Escape`, `Backspace` |
| mouse button 4 (the back button) | back | `PointerButton::Extra1`, 2026-10-26; not while a binding waits for a key or a text field has focus |
| letter O (any layout, ignoring shift) | options | `O` |

Rules: any key with the **Command** modifier is ignored (Windows: ignore when Ctrl, Alt or the Windows key is held, except the
shortcut below). While a native dialog is open the launcher does not handle keys. While a text field has focus (profile name) keys go
to the text field (the original never handles them; the port additionally lets Enter and Esc blur the field, see section 9). While a
binding waits for a key (controls panel) every key is captured by the panel. Key repeat is ours, not the OS's (2026-10-25): the OS repeat events of an arrow
key are dropped and a held arrow runs the same repeat as a held direction on the pad (6.3), so a list moves the same under a finger
and under a thumb. It ends when the key is up, when a dialog opens or when a binding starts to wait for a key. **Shortcut**: `Cmd+,` opens Settings when onboarded -> `Ctrl+,` on Windows and Linux; on a Mac it stays `Cmd+,` (egui's `command` modifier is Ctrl there and Cmd on the Mac, so one check serves both, and the app menu's Settings item shows it, 19.4). F11 toggles fullscreen (Windows addition).

### 6.2 Mouse and wheel

Everything also works with the mouse (clicks listed per component); the mouse's back button (button 4) is the Back command, like Esc. Every full-screen area has a Back button for the pointer at its top left (Friends, Profile, Trophy compare; `screens::back_row`, the hint bar level with it). Mouse wheel / horizontal scroll on the **home screen with no
overlay**: moves the tile selection by one per step: wheel down / scroll right = next game, wheel up / scroll left = previous.
Accumulate deltas and step at +-40 dp of accumulated scroll (reset the accumulator after each step, and when the gesture pauses);
one notch of a classic wheel must produce exactly one step; trackpad pixel deltas are used as is. (The original's sign handling
uses AppKit's inverted deltas; the intent above is what the user experiences.)

### 6.3 Controller (gilrs)

| Control | Command |
|---|---|
| D-pad, left stick (axis > 0.55 on an axis, else that axis counts as 0, see hysteresis below) | direction commands; the dominant axis wins, stick up = up |
| A / Cross (South) | select |
| B / Circle (East) | back |
| Y / Triangle (North) | options |
| Menu / Start | options |

**Repeat timing (2026-10-25, replaces 0.38 s / 0.085 s):** when a direction is pressed it fires immediately; if held, after **0.36 s** it
repeats every **0.10 s**, and after six repeats every **0.06 s**, until released or the direction changes (a change cancels the timers
and fires the new direction at once). That is 17 rows in a 1.6 s hold, and a long list or the library grid no longer takes ten seconds
to cross. A slow frame never fires a burst. Identical consecutive directions are not re-fired. Controller input is not blocked by text fields or panels. The hint bar text is always `✕ / ○ / △` (PlayStation
naming) regardless of the controller.

**Stick hysteresis:** the stick engages at 0.55 and lets go only below **0.38**, so a thumb resting near the threshold does not
flutter between "right" and "nothing". A held direction gives way to the other axis only when that axis is at least 0.55 and **1.25
times** the held one, so a thumb near the diagonal does not jump between "right" and "up"; a flick through the middle (right to left)
changes direction at once. The D-pad beats the stick. The gamepad thread polls every **4 ms** (was 8) while a pad is connected.

**Latency.** `ui::latency` stamps an event when the gamepad thread reads it, when the frame takes it off the channel and when that frame
is built; with `RUST_LOG=anyplay_launcher::ui::latency=debug` it logs the average and the maximum of both intervals every 40 events.

### 6.4 Focus model

Home has three focus zones, `HomeFocus`: `top(0 | 1)` (library button, settings button), `tiles`, `actions(i)` (i = 0 primary,
then trophies if present, last = "...").

| Focus | left | right | up | down | select | back | options |
|---|---|---|---|---|---|---|---|
| tiles | previous game | next game | -> top(1) | -> actions(0) | primary action | - | game settings |
| top(i) | top(max 0, i-1) | top(min 1, i+1) | - | -> tiles | open library (0) / settings (1) | -> tiles | game settings |
| actions(i) | actions(i-1) | actions(min last, i+1) | -> tiles | - | 0 primary; 1 trophies when present; else game settings | -> tiles | game settings |

Every focus change plays `move`. `moveSelection` clamps (no wrap), plays `move`, and measures the new game's size / loads its trophies.
With no games: select opens the folder dialog. The primary action by state: installed = play, locked/failed = unlock (failed clears the
error first), installing/queued = cancel, running = activate the game window, unreadable = toast, starting = nothing.

Overlays (the handler order): onboarding first (5.15); if the splash is up only `back` (cancel start) is used and everything else is
swallowed; then by overlay:

- **settings**, **game settings**, **confirm**: `panelCommand` below. **controls / advanced**: `back` and `options` return to the game
  settings (sound `back`, focus the row that opened it), otherwise `panelCommand`.
- **log**: back or select closes; all else swallowed. **trophies**: up/down move, back/options close. **library**: arrows move (up/down
  by 6), select picks, back/options close.

`panelCommand`: multi-category panels (settings only) have two levels. *Category level* (`panelInRows == false`): up/down move the
category (clamped, row reset to 0, `move`), right/select enter the rows (first focusable row), back/options close the panel. *Row
level*: up/down move to the previous/next **focusable** row (skips info rows; `move`; no wrap); left: adjusts the row down
(choice/toggle/stepper) and, if the row is not adjustable (action, keys, text), leaves the row level; right: adjusts up; select
activates; back leaves the row level (single-category panels: closes); options closes. Single-category panels (game settings,
controls, advanced, confirm) always are at row level; there, back closes (or returns for controls/advanced). Panels open at: game
settings row 0, confirm on Cancel (5.9.1), controls row 1, advanced row 1; settings starts at the category level with category 0.
Sounds: category/row moves and value changes `move`; actions `select`; leaving `back`; opening `select`; closing `back`.

---

### 6.6 The Home button, game and launcher (2026-10-25)

The idea of switching between a running game and the launcher with the controller's Home button comes from the
macOS launcher; the design and the code here are our own. The button is the PS button of a PlayStation pad, the Guide
button of an Xbox pad, and the PS button of the phone controller (`Message::Home`). It acts only while a game runs and has
shown its window (`running`, not `starting`), and only when the preference `ps_button` is on (default on; Settings >
Behaviour, "Home Button Opens the Launcher").

| Where the foreground is | Home button | Any other button or direction |
|---|---|---|
| the game (or any window but the launcher) | brings the launcher in front, restoring it when minimised, and opens the game's quick panel | dropped, the game has the pad |
| the launcher | brings the game window back, closes the panel | as usual |
| no game runs | nothing (the phone's button keeps its old job: close the panel, restore the window) | as usual |

**Where it is read.** `ui::home_button::HomeButton` holds the state the input threads need (running, game window,
preference, focus) as atomics; the UI fills it every frame (`LauncherApp::sync_home_button`). Four sources press it, each on its
own thread, because a minimised launcher draws no frames: the gilrs thread (`Button::Mode`), a thread polling `XInputGetStateEx`
for the Guide bit (Windows; Windows.Gaming.Input, which gilrs uses, hands a pad to the focused window only and never shows
Guide), the PS button reader (below), and the phone server. Presses within 350 ms count once (gilrs and XInput can both see one press). The pure `route`
function is the state machine and carries the tests. The UI takes two flags once per frame, `take_summoned` and `take_returned`,
and opens or closes the panel.

**The quick panel** is a full-screen panel of the frame of 5.9 (`Overlay::InGame`, `screens::ingame`; it covers the game's window, which is behind the launcher anyway): the game's name, the line
"The game keeps running behind the launcher.", and the rows *Back to Game* (the focused row on open), *Trophies*, *Captures*, *Game
Settings* (the game's own sheet), *Friends and Party* (only when signed in) and *Close Game* (destructive). *Close Game* opens the
question `Overlay::ConfirmCloseGame` on *Keep Playing*, so Enter never ends a game. The panel selects the running game first, so
trophies and settings are that game's. Back on the panel, the Back button and *Back to Game* all return to the game. Back on the
home screen does the same while the launcher is there because of the Home button (`Model::summoned`); it is cleared when the
launcher loses the focus, and when the game ends the panel closes.
While the panel is up, the phone's navigation reaches the launcher and its virtual pad is detached from the game.

**The PS button reader** (`ui::ps_button`, 2026-10-26). Windows.Gaming.Input and evdev deliver a PlayStation pad to the focused window, so
with a game in front gilrs never hears its PS button. While a game runs and the preference is on, a thread opens every DualSense
(054C:0CE6, Edge 0DF2) and DualShock 4 (05C4, 09CC, adapter 0BA0) through `hidapi` 2 (Windows: its HID backend; Linux: hidraw),
shared and read only, and watches bit 0 of the third button byte of the input report. The reader never writes a report: rumble and
light bar stay the game's. The layouts are generic device facts from the Linux kernel (`hid-playstation.c`) and SDL
(`SDL_hidapi_ps5.c`, `SDL_hidapi_ps4.c`) and are written in the module's comment: DualSense USB (report 0x01, 64 bytes, byte 10) and
Bluetooth (0x31, 78 bytes, byte 11), DualShock 4 USB (0x01, byte 7) and Bluetooth (0x11, byte 9), and the 10-byte short Bluetooth
report both send before they are switched to the full one (byte 7). Bluetooth reports are accepted only with a valid CRC-32. A
button already held when the pad is opened is not a press; an edge (up, then down) is one press and goes into the same `HomeButton::press` as the Guide button, so gilrs seeing the same press,
or a second pad, counts once (350 ms). Pads are looked for again every 3 s while a game runs and none is open, every 10 s otherwise (a pad plugged in later is found), and
every device is closed again when the game ends or the preference is off.

*If a pad cannot be opened* the reader says so once per pad in `launcher.log` and leaves the button to the other sources (gilrs
where the launcher has the focus, XInput for what shows itself as an Xbox pad); it keeps looking every 3 s without logging again.
Linux needs read access to `/dev/hidraw*`: many desktops give it only to root, and `docs/INSTALL.md` has the udev rule (`TAG+="uaccess"`
for the Sony ids). *Steam Input* (with the pad held in its own configuration) and *HidHide* hide the physical pad from every HID reader,
this one too; what they show instead is a virtual Xbox pad, whose Guide button the XInput thread already reads. Nothing is
done for a pad they hide, on purpose. macOS has no reader here (IOKit HID delivers the button to a background process).

*Other buttons while a game is in front.* The gilrs thread asks `gamepad::press_effect` for every press: with a game in front only
Home and Create (which saves a clip, 9.8) do anything; every other button and the D-pad and stick are dropped before they reach
the UI, so the game's play never moves the focus of the launcher behind it (tested for all buttons).

*Windows.* A background process may not simply take the foreground, and a gamepad press counts as no input event.
`win::activate_window` restores the window if minimised, calls `SetForegroundWindow`, and when that does not take, attaches the input
queue of the foreground window's thread to its own for one `BringWindowToTop` plus `SetForegroundWindow` (`AttachThreadInput`), with
three tries. It sends no key to the game and changes no setting. The same function serves the second-launch hand-off.

*Linux.* The launcher asks its own window to show and take focus. X11 window managers may honour that or only flash the window;
a Wayland compositor ignores it. The game's window is not found there, so the way back minimises the launcher and leaves the
game what is behind it.

*The game's own use of the button.* The mapping of AnyPS5 v0.1.3 names no PS or Guide button, so nothing the game gets is taken
away. If a later AnyPS5 maps it to the PS5 home button, both would react; the preference turns the launcher's side off.

*Not verified with hardware:* that Windows delivers a physical pad's Guide button to the launcher behind a game, the PS button reader
on a real DualSense and DualShock 4 (USB and Bluetooth, Windows and Linux; its parser is tested on synthetic reports), and the
foreground switch against a real game window. The test hook is `--demo` with `ANYPS5_INSTANCE=<name>` and
`ANYPS5_PS_TEST_WINDOW=<hwnd>`: that window becomes the running game, each line `ps` on stdin is a press, and the log prints
the foreground window before and after (`ps-test: ...`).

### 6.7 The edge bump (2026-10-25)

A direction that has nowhere to go (left on the first tile, up in the top bar, down on the last row, the end of the library grid or the
trophy list, the top or bottom category) is answered like every other press, within the same frame: the `bump` cue (7.3) and a leaning
focus ring. The ring, and only the ring, moves toward the wall by about **3 dp** and springs back (`ui::nudge`): a velocity kick of
140 dp/s into a spring pair on `Motion::Focus`, which peaks after 55 ms, undershoots by less than a quarter of a dp and is still
inside a quarter of a second. Nothing but a transform changes, the layout stays put and the element does not move. Under "Animations:
Reduced" the ring stays and only the sound is left. A held direction bumps **once per hold**: the repeats at the edge
are silent and still (`Input::repeating`). The sites that bump are the home screen, the panel rows and categories, the library grid and the trophy list; each calls `Shell::play(Cue::Bump)`, and the shell leans the
ring toward `Input::last_direction`.

### 6.8 Controller vibration (2026-10-25)

A short, soft pulse answers the actions that have a sound, so the hand and the ear get the same answer in the same frame
(`Shell::play` is the one place that decides both). Setting **Controller Vibration** (Settings > Behaviour): Off, Light (x0.7),
Medium (x1, the default), Strong (x1.5). Changing it on the pad plays a select pulse at the new strength.

| Action (cue) | Pulse (ms after start: length, light motor / heavy motor) | Feel |
|---|---|---|
| select | 0: 24, 0.32 / 0 | very light tap |
| back | 0: 18, 0.22 / 0 | lighter |
| error, refused | 0: 30, 0.42 / 0.14; 90: 30, 0.42 / 0.14 | two short taps |
| trophy | 0: 24, 0.24 / 0; 70: 30, 0.30 / 0.12; 150: 70, 0.34 / 0.26 | three taps that swell |
| call (ring) | 0: 60, 0 / 0.24; 110: 40, 0.30 / 0 | a heavy beat, then a light one |
| edge bump | 0: 14, 0.16 / 0 | barely there |
| move focus, hover, boot | none | the focus moves dozens of times a minute |

How it runs. `ui::haptics` drives the pad through `gilrs-core` directly: gilrs' own force-feedback server works in 50 ms ticks, which
cannot make a 14 ms tap and delays every start by up to a tick. A thread of its own holds a pattern as a list of motor states with
their times, sets each at its moment (it asks Windows for a 1 ms timer) and ends on zero. It starts when the pad is first used,
so the first pulse is not late. A pulse is lifted to 0.14 before the level scales it (a weaker one is not felt; an estimate to
tune on hardware), so Light stays below Medium for every cue, and the result is capped at 1. Safe by construction: a pad without force feedback is skipped; the thread zeroes the motors (only when it has lit them: a game's
own rumble is never overwritten) when a pattern is replaced, when it ends, when the setting goes Off, when a game takes the pad
(`Shell::begin_frame` sees `HomeButton::accepts_pad` turn false and stops mid-pulse), when the handle is dropped (it joins the
thread), and a Linux pulse carries its own 200 ms timer. A crash is the one gap: on Windows the pad keeps a pulse of at most
70 ms until the system resets it. It acts only
while the pad is the device in use (a keyboard or mouse player with a pad plugged in feels nothing) and never while a game is in
front. Known limits. Every connected pad with force feedback vibrates, not only the one that was pressed. On Windows `gilrs-core` uses
Windows.Gaming.Input, which offers vibration to Xbox-style pads only: a DualSense or DualShock gets none there (`is_ff_supported`
is false), the setting then has no effect. Not verified on hardware: the feel of the strengths and the floor, WGI vibration from a
second thread and the Linux timer.

### 6.9 Finding the controller (2026-10-26)

**Backend.** gilrs 0.11 with its default `wgi` feature: on Windows the launcher reads pads through Windows.Gaming.Input (the Xbox
family as a `Gamepad`, everything else, a DualSense and a DualShock among them, as a `RawGameController` that gilrs maps from
the bundled SDL controller database; the DualSense and DualSense Edge, 054C:0CE6 and 054C:0DF2, have Windows entries there, and
Bluetooth pads are looked up as USB on purpose). On Linux it reads evdev (`/dev/input/event*` must be readable by the player; the
kernel's `hid-playstation` driver provides the DualSense, SDL's database has its kernel GUIDs). A pad with no database entry is not
dropped: it gets gilrs' default layout and `MappingSource::Driver`, and the launcher does not look at the mapping source. WGI delivers input only
while a window of the process has focus, which suits the launcher's own screens; with a game in front only the XInput thread
(Xbox pads) still hears the Home button.

**What the launcher does about it.**
- Each pad found at start or plugged in later is written to `launcher.log` with name, VID and PID, SDL GUID, mapping (or "no SDL
  mapping") and backend; a pad without a mapping is a warning, "no gamepad found at start" and "gamepad removed" are info lines.
- The pad thread restarts after five seconds when the backend cannot start (one odd device can fail gilrs on Windows) or panics,
  instead of ending for good.
- **Pad in use.** One pad counts at a time (`ActivePad`). Another pad's events are dropped while the pad in use spoke less than
  600 ms ago, so Steam Input or DS4Windows with both the real pad and their virtual Xbox pad visible do not press twice; after 600 ms of quiet the
  other pad takes over. The family (`ui::pad_info::Family`: PlayStation, Xbox, Nintendo, other) is read from the vendor id, else the name.
- **Top bar.** A small status after the title (and the phone status) shows a controller icon and, when the pad reports one, its battery
  in percent (read every 10 s; wired pads and pads without a reading show the icon alone). It is not a button and outside the focus order,
  and it wears no surface: icon and text in the detail ink, so nothing on the bar that only reports looks pressable.
  Screen readers get "Controller connected, battery 82 percent".
- **Hints.** With an Xbox or other pad the hint bar and key strings name the buttons A, B, X, Y; a Switch pad swaps A with B and X with Y
  (the positions). A PlayStation pad keeps the shapes. Before any pad was seen the shapes are used.
- **Settings > Behaviour** has a note under Controller Vibration: what to try when the controller is not found (Steam Input,
  DS4Windows).

A direct HID reader for the DualSense and DualShock reads the pad itself for nothing but the PS button, because that is the one gap WGI and evdev leave
while a game is in front (6.6, "The PS button reader"). The cases that make a pad invisible (Steam Input holding it, HidHide hiding it) hide it from
HID readers as well, so the pad is still not found for everything else by it.
Not verified on hardware: a DualSense or DualShock over USB and Bluetooth through WGI, the battery reading of a Bluetooth DualSense through WGI, the A/B hints on a real Xbox pad.

## 7. Sounds and media

### 7.1 Sounds (`Sounds.swift`): synthesized, nothing bundled

Four cues, each a short buffer rendered once at start, 44 100 Hz, stereo with identical channels, master volume **0.32**, four
voices used round-robin (a retrigger of the same voice cuts it: `stop`, schedule, `play`). A cue is a list of *partials*
`(frequency Hz, start s, duration s)` plus a decay constant `d`. Sample at time `t` (seconds from buffer start), for each partial
with `start <= t < start + duration`, `l = t - start`:

```
attack = min(1, l / 0.004)
sample += sin(2*pi*f*l)  * attack * exp(-l * d)       * 0.5      // fundamental
sample += sin(4*pi*f*l)  * attack * exp(-l * d * 1.6) * 0.12     // second harmonic, decays 1.6x faster
```

Buffer length = `max(start + duration) + 0.02` s; no explicit release (partials end abruptly after their exponential decay has
mostly vanished). Output = the sum (not normalised) times 0.32.

| Cue | Partials (Hz @ start, duration) | Decay `d` | Character |
|---|---|---|---|
| `move` | 1320 @ 0.000, 0.045 | 70 | short tick |
| `select` | 880 @ 0.00, 0.07; 1320 @ 0.05, 0.10 | 32 | rising pair |
| `back` | 990 @ 0.00, 0.06; 660 @ 0.045, 0.09 | 34 | falling pair |
| `unlocked` | 660 @ 0.00, 0.12; 990 @ 0.09, 0.14; 1320 @ 0.18, 0.30 | 12 | rising triad |

Port with `rodio`: build four `SamplesBuffer`s (f32, 2 ch, 44 100) at start, play through a small pool of sinks (or one mixer) with
`set_volume(0.32)`; do nothing when "Interface Sounds" is off; start the audio output lazily on the first cue.

Which events play which cue:

- **move**: any selection or focus change (tiles, top bar, actions, panel category, panel row, library, trophy list, onboarding
  left/right, theme card tap, tile click that changes the selection), any value change (choice, toggle, stepper, chevron click,
  switch tap), a binding captured.
- **select**: unlock queued, Play pressed, a panel/overlay opened, an action row run, rescan, onboarding advance, capture started.
- **back**: closing an overlay, leaving rows, returning from controls/advanced, cancel unlock, cancel start, onboarding back,
  capture cancelled.
- **unlocked**: unlock finished, onboarding finished, a trophy banner (when sounds are on).

### 7.2 Media (`Media.swift`, `GameLibrary.inspect`)

Files read from each game dump's `sce_sys/` folder:

| File | Size | Use | Decode |
|---|---|---|---|
| `icon0.png` | 512 x 512 | tile, splash, onboarding tiles, library | thumbnail, max side 400 px, cached by path + size |
| `pic0.png` | 3840 x 2160 | home backdrop artwork | thumbnail, max side `max(window w, h) * 1.6` px |
| `snd0.at9` | ATRAC9 | found by the scanner (`Game.music`) but **not used** by the UI | - |
| `param.json` | JSON | `titleId` (fallback: folder name), `localizedParameters.<lang>.titleName` using `defaultLanguage` (fallbacks: `en-US`, else the title ID), `contentVersion` (fallback `1.0`) | |
| `trophy2/trophyNN.ucp` | package | trophy list (5.11) | in core |

Decoding happens off the UI thread, results are cached per (path, max-pixel) and the UI re-renders when one arrives; an image fades in
over 0.35 s easeOut (animation 7). Missing `icon0.png`: a gamepad glyph on the white 0.08 tile. Missing `pic0.png`: no artwork layer.
Trophy icons are PNGs inside the packages (`icon0_<lang>.png`, falling back to `icon0.png` for the set icon), cached by id; unearned
trophies are drawn desaturated at 0.45 opacity (do it in a shader or by pre-converting to greyscale at load).
Upload thumbnails as egui textures with `TextureOptions::LINEAR`; pic0 is large: downscale on a worker thread (`image` crate) before
upload; keep at most the current and previous artwork textures.

### 7.3 Soft cues and home music (deviation from 7.1)

The launcher replaces the four sine cues of 7.1 with softer ones (plus `hover`, `error`, `boot` and `ring`) and adds home screen music. Both are still synthesised at
run time: nothing is bundled and nothing is sampled. Their character follows the PS5 system sounds as measured from recordings
(register, partials, attack, decay, level, harmony); no recording is used or shipped.

**Engine** (`ui::sound`). The first cue or the music opens the default output on a `sound` thread, which renders every cue once at
the device rate (stereo, normalised to its peak) and hands rodio one endless source. Cues and music changes travel over a channel,
so the UI thread never waits. Up to 8 cues play at once; another one fades the oldest out over 5 ms. A soft limiter above 0.9
guards the sum. With sounds and music off the device is never opened; tests and the gallery use a silent engine.

**Cues** (`ui::sound::cues`). Every tone is a few sine partials under one envelope (raised-cosine attack, then an exponential
decay with time constant τ), followed by a one-pole lowpass and a Freeverb send.

| Cue | Notes (relative level) | Partials (ratio, level) | Attack / τ | Peak | Send / RT60 |
|---|---|---|---|---|---|
| `move` | F4 | 1; 2 at 0.55; 1.5 at 0.06 | 7 / 26 ms | -22 dBFS | 0.04 / 0.3 s, lowpass 5 kHz |
| `hover` | the `move` tick | as `move` | as `move` | -25 dBFS | as `move` |
| `select` | C5, G5 (0.7) 12 ms later | 1; 2 at 0.45; 3 at 0.08 | 10 / 120 ms | -13 dBFS | 0.15 / 0.9 s |
| `back` | A3, E4 (0.7) | as `select` | 10 / 90 ms | -15 dBFS | 0.12 / 0.8 s |
| `error` | A3, B♭3 (0.9) | 1; 2 at 0.25 | 12 / 90 ms | -15 dBFS | 0.06 / 0.8 s |
| `unlocked` | F4 C5 G5 C6 (1, 0.85, 0.7, 0.55), 15 ms apart, each as two voices 5 cents apart | 1; 2 at 0.25 | 25 / 250 ms | -9 dBFS | 0.20 / 1.2 s |
| `bump` | F3, faint octave, lowpass 1.6 kHz | 1; 2 at 0.25 | 10 / 28 ms | -23 dBFS | 0.04 / 0.4 s |
| `boot` | F3 C4 G4 C5 F5 (0.9 to 0.45), 70 ms apart, two voices 5 cents apart, glitter at 0.3 s | 1; 2 at 0.25 | 350 / 650 ms | -12 dBFS | 0.35 / 2.2 s |

`unlocked` adds a faint glitter: six sines between 3.15 and 7.88 kHz at 0.12, gone within about 0.1 s. `error` is new and plays
with every warning toast (`Model::toast` with `Icon::WARNING`); the other cues keep the events listed in 7.1.

**Music** (`ui::sound::music`). Generated while it plays, so it never loops audibly. Eight chords in F minor and its neighbours,
8 s each, crossfading with a 3.5 s attack and a 4.5 s release on equal-power curves: F sus4, F minor add 9, A♭ maj7, G♭ maj7,
F sus4 (higher), F add 9, D♭ maj9, G♭ add 9 (voicings in `CHORDS`). Each pad note is two PolyBLEP sawtooths 7 cents apart, panned
75/25, through two one-pole lowpasses whose cutoff breathes between 520 and 880 Hz over 25 s. A drone (F1 sine plus 0.3 of F2)
runs under two rounds of the chords; the third round goes without it and 3 dB quieter. Every 1.4 to 3.6 s a soft note (sine plus
0.2 octave, 10 ms attack, τ 0.9 s) from the current chord, between F4 and C6, floats above, panned up to ±0.45. Freeverb RT60 5 s,
damping 0.6. The level sits around -28 dBFS RMS, well under the cues.

**Hover** (`ui::sound::hover`, `Shell::finish_frame`). When the pointer arrives on a new interactive element (control, tile, library cell, row or card: whatever asks `ui::input::hovered`), the `hover` cue plays: the `move` tick of the focus path, 3 dB lower, so mouse, keys and pad sound like one system. `move` was re-cut for this (shorter: 132 ms to -60 dBFS instead of 210 ms; softer octave; a dry tail; rms frequency 455 Hz instead of 517 Hz), and both paths use it. The decision is a pure function of one frame (`HoverGate::update`), and every rule is a unit test:

- once per arrival: nothing while the pointer stays inside, nothing over empty areas or non-interactive surfaces; leaving and coming back ticks again;
- only a pointer that moves: the arrival must follow a pointer movement by at most 150 ms, so a panel sliding under a resting mouse stays silent; while a pad is in use `hovered` is false, so nothing plays, and the mouse moving again re-arms it;
- nothing while a button is held (press, drag), while the wheel or a scroll animation moves content (and for 250 ms after), and nothing late for an element that arrived during those;
- throttle: at least 60 ms between ticks (a sweep over tiles 40 ms apart sounds every second tile, 16 a second at most); an arrival inside the gap is dropped, not delayed; the engine lets at most 2 hover ticks sound at once and cuts the oldest over 5 ms; hover ticks never dip the music;
- no doubling: another cue in the same frame, one in the 90 ms before, or a click completing in the frame silences the arrival;
- off with Interface Sounds off, while a game starts or runs, and while the launcher window is not focused. The tick is only ever an extra cue: every hover also has its glow, cursor and focus ring, and nothing depends on hearing it.

Latency: the first cue, the boot cue, opens the device and renders every cue, and `Sounds::warm` repeats that before any hover when sounds are on. The output asks for 512 frames per callback (about 11 ms at 48 kHz) instead of rodio's 50 ms default, falling back to the default when a device refuses; the engine reads commands every 64 frames, and the UI thread only sends a message.

**Mixing.** The music fades in over 2 s and out over 0.8 s (gain = position²) and dips under every cue: `move` 1.5 dB for 0.12 s,
`select`, `back` and `error` 3 dB for 0.25 s, `unlocked` 6 dB for 1.2 s (15 ms down, 400 ms back up). It plays only while the
`music` preference (Home Screen Music, on by default) is on, no game is starting or running, and the launcher window has focus.

**Game music** (`ui::sound::theme`, `anyps5_core::theme_song`, 2026-10-25). As on a console, the selected game's own music
plays on the home screen: `sce_sys/snd0.at9` of its dump, read and never written. It starts once the selection has rested on the
game for 1 s; moving on fades it out at once (0.8 s) and the next game waits its own second. The song fades in over 1.5 s and takes
the home music's place: the home music fades out while a song plays and back in when it stops, so the two crossfade rather than
leave a gap. The song plays at -10 dB (theme songs are mastered loud), dips under cues like the home music, starts after the
file's encoder delay and repeats its `smpl` loop (the whole song without one). It plays only on the home screen (no screen or
panel in front, onboarding done), with the `gameMusic` preference (Game Music, on by default) on, no game starting or running,
and the window focused. Home Screen Music and Game Music are independent: with only Game Music on, a selected game's song plays
over silence.

The ATRAC9 stream is decoded on the `sound` thread while it plays, one superframe (1024 frames) at a time, with
[LibAtrac9](https://github.com/Thealexbarney/LibAtrac9), the reference decoder (C, MIT), vendored in
`crates/atrac9/libatrac9` and built from source behind a safe wrapper (`anyplay_atrac9`). Upstream trusts every value in the
stream, so a damaged file indexed past its tables: sanitizer fuzzing found those places, each now has a bounds check marked
`AnyPlay:` (listed in `crates/atrac9/libatrac9/README.md`; one million random superframes, no finding). Its bit reader does not
check where a buffer ends either, so the wrapper copies each superframe into a buffer followed by 4 KiB of zeros. The pure Rust `atrac9dec` 0.9.1 was
tried first and rejected: it skips band extension only approximately and fails on real theme songs, which use it at low bit
rates ("invalid scale factor mode" on the first superframe). The music is resampled linearly to the device rate. A three-minute song therefore costs its compressed size (about 1.6 MB), not
35 MB of PCM. A `theme song` thread reads and checks the file (the first 8 superframes must decode); the last two songs stay in
memory, so going back and forth between two games reads no file twice. A file that does not read or decode is skipped for the
rest of the session, and the home music carries on.

### 7.5 The sound map (2026-10-25)

Sixteen cues, all synthesised, one family: sines with an octave, rounded attacks, a little room, the F-C-G fifths of the
trophy chord. New cues, on the voice of the navigation tick of 7.3:

| Cue | Notes | Peak | Heard when |
|---|---|---|---|
| `on` / `off` | C5 then F5, 55 ms apart / the same down | -18 / -19 dBFS | a switch turned on / off |
| `open` / `close` | F4 then C5 70 ms later / the fifth falling | -16 / -17 dBFS | a screen or panel opened / closed |
| `notify` | G5, C6 40 ms later, long soft tail | -16 dBFS | a toast that is not a warning |
| `message` | A4, D5 90 ms later | -17 dBFS | a message or message request arrived (also while the panel shows it) |
| `invite` | F4 C5 F5, 70 ms apart | -15 dBFS | a friend or party invitation arrived |

`back` now means one step back inside a screen (a category, a capture, a cancelled start); leaving a screen is `close`. The test
`the_sound_map_is_balanced` holds the levels: ticks (hover, move, bump) are quieter than every other cue, switches and messages sit
under select, select under the trophy, and the early loudness of the non-tick cues spans less than 12 dB. Every cue is checked to
start and end at zero (no clicks). `ANYPS5_CUE_DIR=<dir> cargo test render_the_cues_to_wav -- --ignored --nocapture` writes every cue as a 48 kHz WAV
and prints peak and early RMS. Interface Sounds and the hover gate decide whether any of it plays; the music dips under each cue (3 dB
for the longer ones).

### 7.6 Feedback: one function, every action (2026-10-25)

`Shell::play(cue)` is the one place an action is answered: it plays the sound, vibrates the pad (6.8) when the pad is the device in use
and the launcher has it, and leans the ring for a bump (6.7). A select from key or pad also presses the focused control for 0.09 s
(`ui::press`). Hover ticks come from `input::hovered`, which every hover surface (controls, rows, tiles, trophy rows, window buttons)
calls. Screens never choose a sound and a vibration separately.

| Action | Sound | Haptic | Motion |
|---|---|---|---|
| Move focus | move | none | ring glides (Motion.focus) |
| Pointer arrives | hover | none | hover fill 120 ms |
| Select, run an action | select | select tap | press 0.97 for 0.09 s |
| Choice or stepper changed | move | none | value eases |
| Switch on / off | on / off | tap, firmer for on | knob spring |
| Open a screen or panel from Home | open | open tap | overlay spring (fade only when reduced) |
| Close it back to Home | close | close tap | the same, reversed |
| Go deeper in a trail of screens or pages (a profile from a friend, a comparison from a profile, a page of a screen) | select (the press that took you there) | select tap | the next screen takes over |
| One step back in a trail (a page up, a screen back to the one it came from) | back | back tap | the previous screen takes over |
| End of a row or list | bump (once per hold) | barely-there tap | ring leans 3 dp (off when reduced) |
| Warning, refused action, a failure toast | error | two taps | toast, 0.42 s spring |
| Other toast | notify (held back, see below) | soft tap | toast |
| Message arrived | message (held back, see below) | light tap | toast when the panel is closed |
| Invitation arrived | invite | beat and two rising taps | toast |
| Call rings | ring | beat and tap | call card |
| Trophy, unlock finished | unlocked | three swelling taps | banner slide and pop |
| Launcher starts | boot | none | intro |

**One rule for nested screens (2026-10-25).** `open` and `close` belong to the layer between Home and a screen: the press on Home that raises it and the Back that returns to Home. Inside a trail of screens (Friends, Profile, Compare, and the pages of the Friends screen) there is only a way in, which is the press that was made anyway (`select`, played by the row or tile, never a second `open` on top of it), and a way out, which is `back`. The screen that is left does not choose: Back is answered by the app (`Model::close_profile`, `close_compare`, `close_friends`), which knows where it lands, so one Back is one cue, and the same Back is `close` from a first-level screen and `back` from a deeper one.

**Alerts do not chatter.** `notify` and `message` answer something the player did not do, so a burst (a group chat, three requests at sign-in) sounds once: `ui::sound::alert::AlertGate` drops every `notify` or `message` (the two share one window) that comes within 2.5 s of the last one that sounded (and its vibration with it). The toast and the badge still show. A dropped cue does not extend the quiet, so a steady trickle chimes once per 2.5 s. Invitations, errors, rings and everything the player caused are never held back.

### 6.5 Key binding names (controls panel)

Bindings are stored as `KEY:<SDL scancode name>`, `MOUSE:<Left|Right|Middle|X1|X2>`, `WHEEL:<Up|Down>`. The original maps macOS virtual key
codes to the **physical position on a US layout** (SDL scancodes); port by mapping the physical key (winit `KeyCode`, not the
produced character) to the same names. The original table covers exactly these keys:

| Physical key | Name |
|---|---|
| A..Z | `A`..`Z` |
| digits 0..9 (top row) | `0`..`9` |
| `-` `=` `[` `]` `\` `'` `,` `.` `/` `` ` `` | the same characters (the original has no `;`) |
| Enter / Space / Tab / Backspace / Esc | `Return` / `Space` / `Tab` / `Backspace` / `Escape` |
| Shift, Ctrl, Alt (left and right) | `Left Shift`, `Right Shift`, `Left Ctrl`, `Right Ctrl`, `Left Alt`, `Right Alt` |
| Windows key (macOS: Command) | `Left GUI`, `Right GUI` (the Windows key opens the Start menu: ignore it when it cannot be captured) |
| Caps Lock | `CapsLock` |
| Keypad `.` `*` `+` `/` `-`, Keypad Enter, Keypad 0..9 | `Keypad .`, `Keypad *`, `Keypad +`, `Keypad /`, `Keypad -`, `Keypad Enter`, `Keypad 0`..`Keypad 9` (all ten digits are in the original table) |
| F1..F12 | `F1`..`F12` |
| Home, End, Page Up, Page Down, Delete | `Home`, `End`, `PageUp`, `PageDown`, `Delete` |
| Arrows | `Left`, `Right`, `Up`, `Down` |

Keys not in this table are ignored while capturing (the capture keeps waiting). Display names in key caps: `Space` -> localized
(`keySpace`), `Return` -> `keyEnter`, `Escape` -> `Esc`, `Backspace` -> `⌫`, arrows -> `← → ↑ ↓`, `Left/Right Shift` -> `Shift`, `Left/Right
Ctrl` -> `Ctrl`, `Left/Right Alt` -> `Option` on macOS (**Windows: `Alt`**), `Left/Right GUI` -> `Cmd` (**Windows: `Win`**), mouse sources ->
`mouseLeft` / `mouseRight` / `mouseMiddle` / `mouseBack` (X1) / `mouseForward` (X2), wheel -> `wheelUp` / `wheelDown`; any other
key prints its name. The input file the game reads is `Action = SOURCE` lines, one per source, in action order (an action without
any source is written as `KEY:F24`, "unbound"), followed by the changed tuning lines (`LeftStickDeadzone`, `RightStickDeadzone`,
`MouseSensitivity`, `MouseVerticalScale`, `MouseCurve`, `MouseDeadzoneOffset`, `MouseMaxSpeed`, `MouseSmoothingMs`, `MouseDecayMs`,
`StickModifierScale`, `KeyboardRampMs`, `TriggerRampMs`; tenths and hundredths written as decimals); its format is documented upstream
in `docs/user/INPUT_MAPPING.md` of AnyPS5-ARM.

---

### 7.4 Discord presence (AnyPS5-ARM `4a29777` to `14ef671`, after the pinned commit)

The running game shows on the player's Discord profile ("Playing ..." with the time since it
started), ported from `DiscordPresence.swift` into `anyps5_core::discord`. A player who plays
through the AnyPlay network also gets "on AnyPlay", the size of the party and a Join button.

- **Transport.** Discord's local IPC: `\.\pipe\discord-ipc-0` to `-9` on Windows (a Unix socket
  elsewhere). A frame is a little-endian `u32` opcode and length, then JSON. Handshake opcode 0
  with `{"v": 1, "client_id": ...}`, answered by opcode 1; then `SET_ACTIVITY` frames (opcode 1).
  A connection belongs to the application it shook hands as; a different one reconnects. A failed
  send reconnects once (Discord restarted). Without Discord nothing happens.
- **Which application.** Discord's public list of detectable games
  (`https://discord.com/api/v9/applications/detectable`, cut to id, name and aliases, cached in
  `Cache/discord-games.json` for a week) is matched by lowercase words of the name or an alias.
  A known game is shown as itself with details "AnyPlay Launcher · Windows"; any other game shows
  under **AnyPlay's own application** (`1563714917998133288`, `ANYPLAY_APPLICATION_ID`; until the switch the
  launcher shared the AnyPS5 application `1562749843724566549` with the macOS launcher) with the
  game's name as details and "AnyPlay Launcher · Windows" as state. An activity under AnyPlay's
  application names the large image `anyplay` (an asset of that application) with the text "AnyPlay";
  a game shown as itself keeps Discord's own image.
- **Playing online.** Signed in, with the cloud reached and a session or a party, the activity says so
  (the offline lines above stay for everything else):

  | | details | state |
  |---|---|---|
  | game Discord does not know | the game's title | `on AnyPlay`, or `on AnyPlay · Party 2 of 4` (members of the party, its room) |
  | game Discord knows | the game's title (Discord's header already names it) | the same |

  The start timestamp is the game's. The party size is text, not Discord's `party` field, because
  Discord ignores buttons next to a party or secrets; the activity carries neither. It is sent again
  when the session, the party or the join link changes (`app/discord.rs`, once per frame, only when
  the activity differs from the one shown).
- **The Join button.** With presence on, the join button on, signed in and a session or party, the
  launcher asks the net service for a [join code](https://github.com/anyplaynet/network/blob/main/spec/cloud-api.md#join-codes)
  (`join_code`) and adds two buttons: `Join on AnyPlay` (the code's link, `<web>/join/<code>`) and
  `Get AnyPlay` (`https://github.com/anyplaynet/launcher/releases/latest`). Discord allows two buttons,
  a label up to 32 characters and an address up to 512 over https; an address that fails these gets
  no buttons. A cloud that answers `not_found` has nothing joinable: no code, no buttons, and no new
  question until the session or party is another one. A code is asked for again 10 minutes before it
  ends, with 30 seconds between two questions and 15 minutes after `rate_limited`. It is revoked
  (`join_code_revoke`) when the game ends, the player signs out (the revoke goes out before the
  sign-out) or the setting goes off; losing the connection keeps a held code. Neither the code nor its
  link is ever logged (`Debug` of the offer, the activity and the link all leave it out). The logic
  is `anyps5_core::online::JoinOffer`; the profile is `app/discord.rs`. What the button opens never
  joins by itself: see 9.6.
- **When.** Set when the game's window appears (`PipelineEvent::WindowShown`), cleared when it
  exits or when the setting is turned off. Everything runs on a `discord` thread; the UI only
  sends messages. Previews and tests never connect.
- **Settings.** Behaviour, "Show Game on Discord" (`discordPresence`, off by default since 2026-10-26; a saved choice is kept), with the
  detail "With Discord open, your profile shows the game and for how long." and the `discord-logo`
  icon. Right under it, only while it is on, "Show a Join Button on Discord" (`discordJoinButton`,
  on by default; row `discordJoin`, icon `user-plus`): "While you play online, people who see your
  Discord status can get an invitation. They choose whether to join." (the joiner is
  asked, not the player; the text says who can use the button). Presence off sends nothing at all,
  buttons included, and hides the join row. Texts: `screens/discord_strings.rs`.

## 8. Localization

Two languages: **English** (`en`) and **Türkçe** (`tr`). Lookup: current language table, then English, then the key itself.
`%d` = integer, `%@` = string (replace with the port's formatter; keep the sentences identical). Accent names (not in the table):
Blue/Mavi, Mint/Nane, Coral/Mercan, Violet/Mor, Gold/Altın, White/Beyaz. The strings below are generated verbatim from
`Localization.swift` (212 keys in each language); the Windows replacements for the macOS-specific ones are in section 9.3 and win over
this table. Some keys exist in the table without being used by the current UI (for example `continueAction`, `skip`, `update`,
`unlocking`, `allGames`, `sortName`, `playTime`, `gameEnded`, `confirm`); keep them for completeness.

| Key | English | Türkçe |
|---|---|---|
| `welcome` | Welcome to AnyPS5 | AnyPS5'e hoş geldin |
| `welcomeSubtitle` | Play PS5 games on your Mac. Let's set a few things up first. | PS5 oyunlarını Mac'inde oyna. Önce birkaç şeyi ayarlayalım. |
| `chooseLanguage` | Language | Dil |
| `chooseTheme` | Pick a theme | Bir tema seç |
| `chooseThemeSubtitle` | You can change it later in Settings, or use your own picture or video. | Sonra Ayarlar'dan değiştirebilir, kendi resmini ya da videonu koyabilirsin. |
| `chooseFolder` | Where are your games? | Oyunların nerede? |
| `chooseFolderSubtitle` | Choose the folder that holds your PS5 dumps. Subfolders are scanned too, and new games show up on their own. | PS5 dump'larının olduğu klasörü seç. Alt klasörleri de tararım, yeni oyunları kendim bulurum. |
| `pickFolder` | Choose Folder | Klasör Seç |
| `changeFolder` | Change Folder | Klasörü Değiştir |
| `gamesFound` | %d games found | %d oyun bulundu |
| `noGamesFound` | No games in this folder | Bu klasörde oyun bulamadım |
| `continueAction` | Continue | Devam |
| `back` | Back | Geri |
| `start` | Start | Başla |
| `skip` | Skip | Atla |
| `games` | Games | Oyunlar |
| `library` | Game Library | Oyun Kütüphanesi |
| `settings` | Settings | Ayarlar |
| `play` | Play | Oyna |
| `unlock` | Unlock | Kilidi Aç |
| `unlocking` | Unlocking | Kilit açılıyor |
| `cancel` | Cancel | İptal |
| `queued` | Queued | Sırada |
| `update` | Update | Güncelle |
| `retry` | Try Again | Tekrar Dene |
| `options` | Options | Seçenekler |
| `locked` | Locked | Kilitli |
| `ready` | Ready to play | Oynamaya hazır |
| `updateAvailable` | Can be installed again with the new version | Yeni sürümle yeniden kurulabilir |
| `failed` | Could not unlock | Kilit açılamadı |
| `starting` | Starting… | Başlatılıyor… |
| `startingText` | The game is opening, please wait. The first start can take a few minutes while the game gets ready for the Mac. | Oyun açılıyor, lütfen bekle. İlk açılış, oyun Mac için hazırlanırken birkaç dakika sürebilir. |
| `running` | Playing | Oynanıyor |
| `stepUnwrap` | Opening the game files | Oyun dosyaları açılıyor |
| `stepRelink` | Converting for Mac | Mac için çevriliyor |
| `stepCopy` | Preparing the game | Oyun hazırlanıyor |
| `stepBundle` | Packaging | Paketleniyor |
| `stepFinish` | Finishing touches | Son dokunuşlar |
| `titleID` | Title ID | Oyun Kimliği |
| `version` | Version | Sürüm |
| `size` | Size | Boyut |
| `status` | Status | Durum |
| `folder` | Folder | Klasör |
| `gameSettings` | Game Settings | Oyun Ayarları |
| `resolution` | Resolution | Çözünürlük |
| `resolutionDefault` | Game default | Oyunun varsayılanı |
| `display` | Display | Görüntü |
| `windowed` | Window | Pencere |
| `fullscreen` | Full Screen | Tam Ekran |
| `windowSize` | Window Size | Pencere Boyutu |
| `vsync` | Vertical Sync | Dikey Senkronizasyon |
| `on` | On | Açık |
| `off` | Off | Kapalı |
| `overlay` | Performance Overlay | Performans Göstergesi |
| `overlayOff` | Off | Kapalı |
| `overlayTopLeft` | Top Left | Sol Üst |
| `overlayTopRight` | Top Right | Sağ Üst |
| `overlayBottomLeft` | Bottom Left | Sol Alt |
| `overlayBottomRight` | Bottom Right | Sağ Alt |
| `gameLanguage` | Game Language | Oyun Dili |
| `followLauncher` | Same as the launcher | Launcher ile aynı |
| `reinstall` | Unlock Again | Kilidi Yeniden Aç |
| `showInFinder` | Show in Finder | Finder'da Göster |
| `openLog` | Open Log | Günlüğü Aç |
| `uninstall` | Remove Installation | Kurulumu Kaldır |
| `clearShaderCache` | Clear Shader Cache | Shader Önbelleğini Temizle |
| `uninstallConfirm` | Your game files stay and the game shows as locked again. | Oyun dosyaların silinmez, oyun tekrar kilitli görünür. |
| `uninstallDone` | Installation removed | Kurulum kaldırıldı |
| `cacheCleared` | Shader cache cleared | Shader önbelleği temizlendi |
| `appearance` | Appearance | Görünüm |
| `theme` | Theme | Tema |
| `accent` | Accent Color | Vurgu Rengi |
| `background` | Background | Arka Plan |
| `backgroundTheme` | Theme animation | Tema animasyonu |
| `backgroundImage` | My picture | Kendi resmim |
| `backgroundVideo` | My video | Kendi videom |
| `chooseFile` | Choose File | Dosya Seç |
| `gameArtwork` | Show the selected game's artwork | Seçili oyunun görselini göster |
| `motion` | Animations | Animasyonlar |
| `motionFull` | Full | Tam |
| `motionReduced` | Reduced | Azaltılmış |
| `sounds` | Interface Sounds | Arayüz Sesleri |
| `language` | Language | Dil |
| `storage` | Library | Kütüphane |
| `rescan` | Scan Again | Yeniden Tara |
| `installedSize` | Installed games are kept as APFS clones and take no extra space. | Kurulu oyunlar APFS klonu olarak saklanır, ek yer kaplamaz. |
| `about` | About | Hakkında |
| `aboutText` | AnyPS5 runs PS5 games on the Mac through Rosetta and MoltenVK. | AnyPS5, PS5 oyunlarını Rosetta ve MoltenVK üzerinden Mac'te çalıştırır. |
| `toolkitVersion` | Toolkit | Araç seti |
| `profileName` | Profile Name | Profil Adı |
| `behaviour` | Behaviour | Davranış |
| `hideWhilePlaying` | Hide the launcher while a game runs | Oyun açılınca launcher'ı gizle |
| `rosettaMissing` | Games run through Rosetta, which this Mac does not have yet. | Oyunlar Rosetta ile çalışır, bu Mac'te kurulu değil. |
| `rosettaInstall` | Install Rosetta | Rosetta'yı Kur |
| `rosettaInstalled` | Rosetta is installed | Rosetta kurulu |
| `moveToApplications` | Move AnyPS5 to Applications | AnyPS5'i Uygulamalar'a taşı |
| `moveToApplicationsText` | macOS opens downloaded apps from a locked, read-only copy. Let me move it to Applications and reopen it, so this never gets in the way again. | macOS indirilen uygulamaları kilitli, salt okunur bir kopyadan açıyor. Uygulamalar klasörüne taşıyıp yeniden açayım, bir daha bununla uğraşmazsın. |
| `moveNow` | Move and Reopen | Taşı ve Yeniden Aç |
| `later` | Later | Sonra |
| `noToolkit` | This copy has no conversion tools. Use AnyPS5.app from the release. | Bu kopyada dönüştürme araçları yok. Release'deki AnyPS5.app'i kullan. |
| `emptyLibraryTitle` | No games yet | Henüz oyun yok |
| `emptyLibraryText` | Put PS5 dumps in your games folder and they show up here on their own. | Oyun klasörüne PS5 dump'larını koy, burada otomatik görünsünler. |
| `scanning` | Looking for games | Oyunlar aranıyor |
| `allGames` | All Games | Tüm Oyunlar |
| `sortName` | By name | Ada göre |
| `playTime` | Last played | Son oynanan |
| `themeCosmic` | Cosmic | Kozmik |
| `themeAurora` | Aurora | Aurora |
| `themeSunset` | Sunset | Gün Batımı |
| `hintMove` | Move | Gezin |
| `hintSelect` | Select | Seç |
| `hintBack` | Back | Geri |
| `hintOptions` | Options | Seçenekler |
| `unlockFailed` | Could not unlock | Kilit açılamadı |
| `startFailed` | The game could not start | Oyun başlatılamadı |
| `gameEnded` | The game closed | Oyun kapandı |
| `logTitle` | Log | Günlük |
| `close` | Close | Kapat |
| `quit` | Quit | Çık |
| `confirm` | Confirm | Onayla |
| `controls` | Controls | Tuş Atamaları |
| `trophies` | Trophies | Kupalar |
| `advanced` | Advanced | Gelişmiş |
| `advancedDetail` | Sticks, mouse, keyboard, cache, sharing | Çubuklar, fare, klavye, önbellek, paylaşım |
| `advancedTitle` | Advanced Settings | Gelişmiş Ayarlar |
| `makeReport` | Make a Report | Tanı Raporu Oluştur |
| `makeReportDetail` | To attach on Discord or GitHub | Discord'a ya da GitHub'a eklemek için |
| `screenshotsFolder` | Screenshots Folder | Ekran Görüntüleri Klasörü |
| `showScreenshots` | Show Screenshots | Ekran Görüntülerini Göster |
| `fontsFolder` | System Fonts Folder | Sistem Fontları Klasörü |
| `fontsNone` | None | Seçilmedi |
| `fontsReset` | Remove the Fonts Folder | Font Klasörünü Kaldır |
| `fontsHint` | F12 in a game saves a screenshot. Games that ask for the console's system fonts use the files in the fonts folder. | Oyunda F12 bir ekran görüntüsü kaydeder. Konsolun sistem fontlarını isteyen oyunlar, font klasöründeki dosyaları kullanır. |
| `sectionSticks` | Controller sticks | Kumanda çubukları |
| `leftDeadzone` | Left Stick Deadzone | Sol Çubuk Ölü Bölge |
| `rightDeadzone` | Right Stick Deadzone | Sağ Çubuk Ölü Bölge |
| `deadzoneDetail` | How much of the travel reads as centered; stops a drifting stick | Ortada sayılan kısım; kayan bir çubuğu durdurur |
| `sectionMouse` | Mouse as the camera (middle button or F7) | Fareyle kamera (orta tuş ya da F7) |
| `mouseSensitivity` | Mouse Sensitivity | Fare Hassasiyeti |
| `mouseSensitivityDetail` | Higher turns fully with less movement | Yüksek değer daha az hareketle tam döner |
| `mouseVertical` | Vertical Speed | Dikey Hız |
| `mouseVerticalDetail` | Speeds up looking up and down against turning | Yukarı-aşağı bakışı yatay dönüşe göre hızlandırır |
| `mouseCurve` | Response Curve | Tepki Eğrisi |
| `mouseCurveDetail` | Above 1 gives finer control at low speed | 1'in üstü yavaş harekette daha ince kontrol |
| `mouseMinimum` | Minimum Tilt | En Küçük Eğim |
| `mouseMinimumDetail` | Raise it if slow movement does not move the camera | Kamera yavaş harekette kıpırdamıyorsa artır |
| `mouseMaxSpeed` | Maximum Speed | En Yüksek Hız |
| `mouseMaxSpeedDetail` | Keeps a fast flick at full tilt a little longer | Hızlı bir savuruşu biraz daha uzun tutar |
| `mouseSmoothing` | Smoothing | Yumuşatma |
| `mouseSmoothingDetail` | Smooths jittery movement; off follows every report | Titreyen hareketi yumuşatır; kapalı her hareketi izler |
| `mouseDecay` | Center After | Bırakınca Ortalama |
| `mouseDecayDetail` | How soon the camera stops after the mouse does | Fare durunca kameranın durma süresi |
| `sectionKeyboard` | Keyboard | Klavye |
| `stickModifierScale` | Slow Movement Amount | Yavaş Hareket Oranı |
| `stickModifierScaleDetail` | How far the sticks tilt while Slow Movement is held (set in Controls) | Yavaş Hareket tuşu basılıyken çubuğun eğimi (Tuş Atamaları'nda) |
| `keyboardRamp` | Stick Ramp | Çubuk Geçişi |
| `keyboardRampDetail` | How long a key takes to tilt the stick fully | Tuşla çubuğun tam eğime varma süresi |
| `triggerRamp` | Trigger Ramp | Tetik Geçişi |
| `triggerRampDetail` | How long a key takes to press L2/R2 fully, for racing games | Tuşla L2/R2'nin sonuna kadar basılma süresi, yarış oyunları için |
| `sectionGraphics` | Graphics | Görüntü |
| `shaderCache` | Shader Cache | Shader Önbelleği |
| `shaderCacheDetail` | When off, shaders compile again at every start | Kapalıyken her açılışta shader'lar yeniden derlenir |
| `sectionShare` | Sharing | Paylaşım |
| `exportSettings` | Export Settings | Ayarları Dışa Aktar |
| `exportSettingsDetail` | Keys and every setting in one file | Tuşlar ve tüm ayarlar tek bir dosyada |
| `importSettings` | Import Settings | Ayarları İçe Aktar |
| `importSettingsDetail` | Applies a settings file someone shared | Birinin paylaştığı ayar dosyasını uygular |
| `resetAdvanced` | Reset Advanced Settings | Gelişmiş Ayarları Sıfırla |
| `reportSaved` | Report saved | Rapor kaydedildi |
| `reportFailed` | The report could not be saved | Rapor kaydedilemedi |
| `settingsExported` | Settings exported | Ayarlar dışa aktarıldı |
| `settingsImported` | Settings imported | Ayarlar içe aktarıldı |
| `settingsImportedOther` | Settings from %@ imported | %@ ayarları içe aktarıldı |
| `settingsFileFailed` | This is not an AnyPS5 settings file | Bu dosya bir AnyPS5 ayar dosyası değil |
| `stickModifier` | Slow Movement | Yavaş Hareket |
| `diagnosisMissingModule` | Missing module: %@. Unlocking the game again may fix it. | Eksik modül: %@. Kilidi yeniden açmak çözebilir. |
| `diagnosisUnsupported` | The game asked for a system function that is not supported yet: %@ | Oyun henüz desteklenmeyen bir sistem fonksiyonu istedi: %@ |
| `diagnosisGpuLost` | The GPU stopped responding. | GPU ile bağlantı koptu. |
| `diagnosisOutOfMemory` | The game ran out of memory. | Bellek yetmedi. |
| `diagnosisEngineError` | Engine error: %@ | Motor hatası: %@ |
| `diagnosisCrashed` | The game crashed (%@). | Oyun çöktü (%@). |
| `trophyEarned` | Trophy earned | Kupa kazandın |
| `trophyHidden` | Hidden Trophy | Gizli Kupa |
| `trophyHiddenDetail` | Shows what it is once you earn it. | Kazandığında ne olduğu görünür. |
| `trophyLocked` | Not earned | Kazanılmadı |
| `gradePlatinum` | Platinum | Platin |
| `gradeGold` | Gold | Altın |
| `gradeSilver` | Silver | Gümüş |
| `gradeBronze` | Bronze | Bronz |
| `controlsHint` | Pick a row, then press the key you want for it. Esc cancels. A connected controller keeps its own buttons. | Bir satır seç, sonra atamak istediğin tuşa bas. Esc vazgeçer. Kumanda bağlıysa kendi tuşlarıyla çalışır. |
| `pressKey` | Press a key… | Bir tuşa bas… |
| `resetControls` | Reset to Default Keys | Varsayılan Tuşlara Dön |
| `noKey` | None | Yok |
| `dpad` | D-pad | Yön Tuşu |
| `leftStick` | Left Stick | Sol Çubuk |
| `rightStick` | Right Stick | Sağ Çubuk |
| `touchpadLeft` | Touchpad (Left) | Dokunmatik Yüzey (Sol) |
| `touchpadRight` | Touchpad (Right) | Dokunmatik Yüzey (Sağ) |
| `directionUp` | Up | Yukarı |
| `directionDown` | Down | Aşağı |
| `directionLeft` | Left | Sol |
| `directionRight` | Right | Sağ |
| `keySpace` | Space | Boşluk |
| `keyEnter` | Return | Enter |
| `mouseLeft` | Left Click | Sol Tık |
| `mouseRight` | Right Click | Sağ Tık |
| `mouseMiddle` | Middle Click | Orta Tık |
| `mouseBack` | Mouse 4 | Fare 4 |
| `mouseForward` | Mouse 5 | Fare 5 |
| `wheelUp` | Wheel ↑ | Tekerlek ↑ |
| `wheelDown` | Wheel ↓ | Tekerlek ↓ |
| `unreadableDump` | This dump is encrypted or damaged and cannot be opened. | Bu dump şifreli ya da bozuk, açılamaz. |

Date and number formats: dates in trophy rows use the launcher language's locale (`en-US` / `tr-TR`), medium date and short time; file
sizes use decimal units (1 GB = 10^9 bytes) with the language's decimal separator (S-home shows `8,39 GB`).

---

## 9. Deviations (what must differ on Windows)

### 9.1 Font, icons, rendering

| Original | Windows port |
|---|---|
| SF Pro / SF Pro Rounded / SF Mono (Apple licence forbids redistribution) | Inter (Text + Display static cuts, or variable), JetBrains Mono; see 1.6 |
| SF Symbols | Phosphor Icons (MIT) per 5.16 |
| `.continuous` squircle corners | circular corners (optional 60 % smoothing mesh later) |
| `.ultraThinMaterial` | GPU two-pass blur + tint per section 4 |
| Metal shader, half resolution | WGSL through wgpu, same half resolution |
| SwiftUI springs and curves | the spring solver of 2.1 and the Beziers of 2.2 |
| AVFoundation / AVAudioEngine | `rodio` per 7.1 |
| `AVPlayerLooper` video background | postponed (picture only) |

### 9.2 Window, files, system integration

| Original (macOS) | Windows equivalent |
|---|---|
| Hidden, transparent title bar; window moves by dragging the background | **No title bar** (`with_decorations(false)`); the launcher draws its own caption (`ui::caption`): a drag strip along the top edge, 32 dp high, moves the window through the system move loop (`StartDrag`, so Snap still works) and a double click maximises or restores; minimise, maximise/restore and close sit in the top-right corner at the Windows 11 size (46 x 32 points; a large window does not grow it, a small one shrinks it with the UI so it stays above the top bar; 10 dp glyphs with 1 px strokes, white 0.6 at rest and white on hover, hover fill white 0.08, close hover #C42B1C), hidden while a controller is in use unless the pointer comes near the top; while the window is not maximised a 6 point band along each edge resizes it (`BeginResize` on the press). Fullscreen shows none of it. DWM keeps the shadow (`with_undecorated_shadow`), and is asked for dark mode (system menu), a black border and round corners. Window title `AnyPS5` (taskbar, Alt+Tab). |
| Starts maximised to the screen's visible frame; green button = fullscreen | starts in borderless fullscreen on the screen it opens on, like a console (preference `fullscreen`, default on); `F11` or the caption's middle button switches to a maximised window and back, and the choice is remembered; min size 1100 x 650 |
| Menu bar: `Cmd+,` Settings, no New | no menu bar (on the Mac there is one, 19.4); `Ctrl+,` opens Settings (when onboarded); `Alt+F4` closes; the "Quit" row in About stays |
| `NSOpenPanel` / `NSSavePanel` | native Windows dialogs through the `rfd` crate: pick folder (games, screenshots, fonts), pick file (picture; video later; settings JSON import), save file (settings export, default name `"<game> - AnyPlay.json"`), dialog titles from `pickFolder`, `chooseFile`, `exportSettings`, `importSettings`. While a dialog is open the launcher ignores input. |
| Finder (reveal in Finder) | Explorer: `explorer.exe /select,"<path>"`; label `showInFinder` -> "Show in Explorer" |
| `UserDefaults` | JSON file `%APPDATA%\AnyPS5\preferences.json` (same fields, section 5.7) |
| `~/Library/Logs/AnyPS5/<titleID>.log`, `.../Reports/` | `%LOCALAPPDATA%\AnyPS5\Logs\<titleID>.log`, `...\Logs\Reports\` |
| `~/Library/Application Support/AnyPS5/Controls/<game>.ini` | `%APPDATA%\AnyPS5\Controls\<game>.ini` |
| `~/Pictures/AnyPS5` (F12 screenshots) | `%USERPROFILE%\Pictures\AnyPS5` |
| Shader cache `~/Library/Caches/<bundle id>` | the toolkit's cache folder on Windows (to be defined by the core crate); the row stays |
| Crash reports `DiagnosticReports/eboot*.ips` | Windows Error Reporting / minidumps for the game process; "crashed" shows the exception code and name (for example `0xC0000005 (ACCESS_VIOLATION)`) |
| dyld `Library not loaded` | Windows loader failure: exit code `0xC0000135` (`STATUS_DLL_NOT_FOUND`) or a "DLL not found" line in the log; still maps to `diagnosisMissingModule` |
| `VK_ERROR_DEVICE_LOST` etc. | unchanged (Vulkan on Windows) |
| NSWorkspace launch, process watch, `CGWindowList` first-window check | spawn the game process and wait on its handle; poll `EnumWindows` for a visible top-level window of that process larger than 200 x 150 (every 400 ms, 240 s timeout); bring it forward with `SetForegroundWindow` (the original "yields activation") |
| Esc on splash: `forceTerminate` | `TerminateProcess` (on the game's job object) |
| "Hide Launcher While Playing" (`orderOut`) | minimise the launcher window and restore on exit. The original's caveat (a hidden app's banner never shows) does not apply, the banner is its own window |
| Notch / menu-bar safe area for the banner | top of the primary monitor's work area, no inset |
| Trackpad scroll phases | wheel/touchpad scroll deltas (6.2) |
| Controller discovery (`GCController`) | `gilrs` events; no wireless discovery step |
| Key cap labels Option / Cmd | `Alt` / `Win` (6.5) |
| Text field keeps all keys | the port lets Enter and Esc **blur** the profile-name field (the original cannot leave it by keyboard); arrows and text editing stay with the field |

### 9.3 Mac-only features and strings

Removed outright (no Windows meaning): the **Rosetta row** (`rosettaMissing`, `rosettaInstall`, `rosettaInstalled`, the Behaviour category
row and `installRosetta`), the **app translocation** machinery (`AppLocation`, `MoveBanner`, `moveToApplications`,
`moveToApplicationsText`, `moveNow`, `later`, the About "Move to Applications" row), the Rosetta/MoltenVK wording in the About text and
the APFS-clone sentence. Strings that mention the Mac are replaced (keys unchanged; these win over the table in section 8), and so
are strings that name the macOS app as the product: they take the launcher's own name (9.7).

| Key | English (Windows) | Türkçe (Windows) |
|---|---|---|
| `welcome` | Welcome to AnyPlay | AnyPlay'e hoş geldin |
| `welcomeSubtitle` | Play PS5 games on your PC. Let's set a few things up first. | PS5 oyunlarını PC'nde oyna. Önce birkaç şeyi ayarlayalım. |
| `stepRelink` | Converting for Windows | Windows için çevriliyor |
| `startingText` | The game is opening, please wait. The first start can take a few minutes while the game gets ready for Windows. | Oyun açılıyor, lütfen bekle. İlk açılış, oyun Windows için hazırlanırken birkaç dakika sürebilir. |
| `aboutText` | AnyPlay Launcher runs PS5 games natively on Windows with AnyPS5. | AnyPlay Launcher, PS5 oyunlarını AnyPS5 ile Windows'ta yerel olarak çalıştırır. |
| `settingsFileFailed` | This is not an AnyPlay settings file | Bu dosya bir AnyPlay ayar dosyası değil |
| `installedSize` | Unlocked games sit next to your dumps and share their files where the drive allows. Choose another folder to keep them on a different drive. | Kilidi açılan oyunlar dökümlerinin yanında durur ve sürücü izin verdiği yerde dosyalarını paylaşır. Onları başka bir sürücüde tutmak için başka bir klasör seç. |
| `showInFinder` | Show in Explorer | Dosya Gezgini'nde Göster |
| `behaviour` | Behavior | Davranış |
| `hideWhilePlaying` | Hide Launcher While Playing | Oyun açılınca launcher'ı gizle |
| `noToolkit` | This copy has no conversion tools. Use the installer from the release. | Bu kopyada dönüştürme araçları yok. Release'deki kurulum dosyasını kullan. |

If the Windows "unlock" does something other than converting, keep the five step *slots* and weights (5.4) and only rename the texts.
The log-start marker `[anyps5-launcher] start` is kept as is.

### 9.4 Trophy banner over games

The macOS panel sits at screen-saver level above full-screen games. On Windows the banner is a separate topmost layered window
(`WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW`; an egui secondary viewport with no decorations, transparent, always on top,
mouse pass-through, not in the taskbar, not activating). **Limitation:** a game in *exclusive* fullscreen cannot be drawn over by an
ordinary window; it works for windowed and borderless fullscreen. When the window cannot be shown the banner is skipped and the trophy
list still updates.

### 9.5 The launcher updates itself

The old macOS launcher used Sparkle; this one has its own flow. Core module `anyps5_core::selfupdate`, host `app::selfupdate`. The Windows exe, the Linux AppImage and the Apple silicon Mac app update themselves; a Linux `.tar.gz` is replaced from the releases page.

- **Check.** At the start and then every 6 hours while the launcher stays open (after a check that could not reach GitHub, every 30 minutes), in the background, when the *Check for Updates* preference is on. *Check Now* on the About page checks once whatever the preference says. The request is conditional: the release data of `anyplaynet/launcher` is asked for with the ETag of the last answer, so a check that finds nothing new costs one small reply that GitHub does not count against its rate limit. The last copy is never taken for an answer when the network fails: an offline launcher does not call itself up to date. A newer semver (pre-release tags sort below their release) is downloaded and verified at once; garbage tags are never newer. Previews, the demo, the gallery and the tests never check.
- **Verify.** From the first trusted key on (`data/update-keys.txt` holds "anyplay 2026" since 0.2.0) the launcher takes updates only through the release's signed `latest.json` and `latest.json.sig`: an Ed25519 signature by a trusted key, an unexpired manifest, a version newer than the running one, a running version not below the manifest's `minimum_version`, and a download with exactly the manifest's SHA-256 and size. The portable `AnyPlay-Launcher-<v>-x64.exe` (or `AnyPS5-Launcher-<v>-x64.exe`, which every release also carries, 9.7) goes to `%LOCALAPPDATA%\AnyPS5\Updates` under its final name only after that. There is no fallback to the unsigned `SHA256SUMS.txt` flow. The format, the rules, the migration and the key ceremony are in [UPDATES.md](UPDATES.md).
- **Swap.** Windows cannot overwrite a running exe but can rename it. The staged file is copied beside the running exe (`<name>.new`, it may be on another drive), hashed again, the running exe is renamed to `<name>.old`, the copy takes its place. A failure in the second step moves the old one back. The next start deletes `.old` files and stale downloads (one still in use stays until a later start). It works for the installed exe (`%LOCALAPPDATA%\Programs\AnyPlay Launcher`, or `...\AnyPS5 Launcher` for an install from v0.1.x) and for the portable exe in any writable folder. The swap keeps the exe's file name; nothing depends on it. A folder that refuses new files is reported before anything is downloaded.
- **Where the player sees it.** An update is visible in two places, and never as a toast or a progress bar in the way.
  - *About*, under *Version* (which always shows the version that runs): the row *Launcher Update* with one short value and, under it, one sentence and at most one row that acts. Checking: "Checking…". Nothing newer: "Up to date" and *Check Now*. Downloading: "Downloading 42%", then "Verifying"; this is the only place with progress. Ready: "0.2.5 ready", "Restart now, or it installs the next time you close the launcher.", *Restart Now* and *What's New in 0.2.5*. Held for a game: "Waiting for your game", *Cancel Restart*. A read-only folder, a release that was refused (signature, expiry, hash, size) or any other failure: "Update failed" or "0.2.5 available" with the one thing to do, *Download from GitHub*. Offline: "Could not check" and *Check Now*; the launcher tries again by itself. A refusal is logged and says nothing anywhere else.
  - *The notice on the home screen*, after the title, only while a verified update waits and the player has not sent it away: "Launcher 0.2.5 is ready" with *Restart Now* (the one primary button) and *Later*. *Later* hides it until the next start; the About row stays. One place at a time: the notice (sentence and buttons, no glyph) while it is wanted and fits, the dot on the gear once it was sent away or has no room, and the About row as the detail in Settings; the three are never on screen together as announcements. While a restart waits for a game, the notice says "Restarts when your game closes" with *Cancel Restart*. The pad reaches it from the library button (left of it), so every action has a key and a pad path. It is out of the way of the top bar and only appears where it fits.
- **Restart Now.** Closes the launcher, swaps, starts the new exe with the same flags and **opens where the player was**: the link of the selected game (`anyps5://game/<TITLEID>`, which selects it again) or, from the settings, `anyps5://settings/about`. It never interrupts a running game: while a game runs or starts, the restart is held, the About row and the notice say so, and it goes ahead the moment no game runs. Closing the launcher by hand still installs the update, without starting it again.
- **What's New.** The notes of the staged release are taken from the release data the check fetched anyway (display only; nothing is decided by them) and shown in the same full-screen panel as the toolkit's notes: every `###` heading of the release is a group, every bullet a line, all groups unfolded. *Back* returns to About.
- **Linux.** The AppImage updates itself the same verified way, through the signed manifest only (its `linux-x86_64` entry; Linux has no unsigned path). The running image comes from `APPIMAGE` (checked against `APPDIR`, a regular `.AppImage` file in a writable folder); the verified download is copied beside it, set to 0755, hash-checked once more and renamed over it atomically, which Linux allows while it runs. Restart starts the new file with the same arguments. A launcher from the `.tar.gz` cannot replace itself: About shows "<v> available" with *Download from GitHub*, and nothing is downloaded. A manifest from before the Linux entry offers Linux nothing, quietly. Details in [UPDATES.md](UPDATES.md#linux-the-appimage).
- **Not done.** The exe is not code-signed (Windows SmartScreen); that costs money and is the owner's decision. Provenance (GitHub attestations) is separate, see UPDATES.md.

### 9.5.1 A required update

An update is *optional* by default and stays so (9.5). It becomes *required* when the version that runs has been retired on purpose: a retired launcher stops going online until it is updated. Two things can retire a version, and either is enough:

- **The signed manifest.** `latest.json` may carry `min_supported_version`. A launcher below it, which has just verified that manifest (signature, expiry, newer version), treats the update as required. The field is covered by the signature, so it cannot be lowered in transit, and an expired or unsigned manifest requires nothing. It is optional: a manifest without it is what every release before this was, and a launcher that does not know the field ignores it. It is **not** `minimum_version`, which keeps its old meaning ("the oldest version that may *take* this update"): reusing that name would have locked every launcher up to v0.2.x out of updating. How to set it: [UPDATES.md, Retiring a version](UPDATES.md#retiring-a-version).
- **The cloud.** A cloud with `min_launcher_version` set answers a launcher below it with `client_too_old` (HTTP 426, network `spec/cloud-api.md`). The net service passes it on as the control event `client_too_old`; the launcher then requires the update as well, and checks for it at once even when *Check for Updates* is off, because the player cannot go online without it. Old clients, browsers and other programs send no `X-AnyPlay-Client` header and are never turned away.

**What the player sees.** A required update opens one full-screen panel, **Update required**, once per run, as soon as the first-run setup is done and no game starts or runs (a game is never interrupted). It is the quiet kind of screen the launch splash is: glass over the home screen, one column in the middle, nothing red.

- A glyph in ink, the title *Update required*, and **one line** of reason: "AnyPlay Launcher 0.3.0 is needed to play online." (without a known version: "This version of AnyPlay Launcher can no longer play online.").
- Under it one status line and, while it downloads, a thin progress line: "Downloading 42%", "Verifying", "0.3.0 ready". The download is the ordinary one (9.5): it starts by itself, is checked against the signed hash and size, and the restart opens where the player was.
- The release notes of the update when they are known: up to three lines under "What's New in 0.3.0". They are display text; the whole notes stay in the *What's New* panel. They are left out when the window is too short for them.
- **Two buttons, one above the other, 12 apart.** The primary one follows the state of the update (`screens::required::plan`): *Update Now* when the update is ready (it restarts; held while a game runs, as everywhere); the same button waits, disabled, while it downloads (the status line says why); *Try Again* when GitHub could not be reached; *Download from GitHub* when this launcher cannot install itself (a Linux `.tar.gz`, a read-only folder, a launcher too old to update itself, a refused download). Below it, secondary and always there: **Play Offline**.
- The two buttons keep their place in every state: the progress line's room is taken whether the bar shows or not, so Update Now does not move when the download ends. The notes under them share the buttons' width and edges.
- Keys: Up and Down (Tab) move between the two, Enter presses, **Back is Play Offline**. The focus starts on the primary button, or on Play Offline while the primary one cannot be pressed, so Enter always does something. The hint bar names Back as *Play Offline*.

**Offline play stays allowed (decided).** The retirement is about the network: the online services only speak to launchers they can still support, and nobody should lose their own games because a version was retired on a day the PC has no network. *Play Offline* closes the panel, the home screen is the player's as before, and nothing offline changes: library, unlocking, playing, saves, the trophies on this PC, settings.

**The online area is closed.** From the moment the update is required the net service is stopped, and every way into the online area (the profile and friends buttons, a notice, an invitation or `anyps5://join` link, the Home button's friends entry) opens this same panel instead, with the reason "The online area needs the new launcher. Your games still play offline." The panel opens by itself once per run and comes back only when the online area is asked for. A player who has *Check for Updates* off still learns it from the cloud, but only when they go online.

**Linux.** The AppImage updates itself through the same flow. A `.tar.gz` has no self-update: the primary button is *Download from GitHub* and nothing is downloaded.

Code: `anyps5_core::selfupdate` (`Release::required`, `SelfUpdateError::RequiredByHand`), `app::selfupdate` (`SelfUpdateService::required`, `require_from_cloud`), `app::model` (`update_required`, `switch_overlay`), `screens::required` (the panel and its state machine). Gallery pages: `update-required`, `update-required-downloading`, `update-required-failed`, `update-required-download`, `update-required-offline`.

### 9.6 Deep links, single instance and folder entry

The launcher answers `anyps5://` links and runs once per user. The grammar (`crates/anyps5-core/src/links.rs`):

| Link | Shows |
|---|---|
| `anyps5://game/<TITLEID>` | the game on the home screen |
| `anyps5://game/<TITLEID>/unlock` | the game, with its Unlock button focused and a hint; the unlock starts only when the player presses it |
| `anyps5://settings[/<page>]` | the settings, on `appearance`, `library`, `language`, `behavior`, `toolkit`, `advanced` or `about` |
| `anyps5://open?path=<folder>` | a dump folder: selected when it is in the games folder, otherwise the launcher says it is outside and opens the library page |
| `anyps5://join/<code>` | the invitation a join code stands for (the Join button on a friend's Discord, 7.4): signed in, the launcher redeems the code and opens the Online panel, whose Home page lists the invitation with Join Game and Decline (or Join Party); signed out, a toast and the sign-in page, and the code waits to be redeemed after the sign-in |

`<TITLEID>` is `PPSA` or `CUSA` and five digits. Websites should write the version as the first segment, `anyps5://v1/game/PPSA16064`; the short form is the same link, any other version is refused. A folder path alone (as Explorer passes it for the folder entry) is the same as `open?path=`.

Safety rules, all in the one parser:

- A link never starts a game, never unlocks, never deletes and never writes. A join link only asks the network for an invitation (`join_redeem`, which joins nothing); accepting it is the player's, and accepting a game still runs the version check. It selects, focuses and opens. There is no `play`, `uninstall` or `env` action.
- The closed vocabulary above; unknown actions, unknown query keys, extra segments and a second `path` are errors. An error shows "This is not a valid AnyPlay link" and does nothing else.
- `<code>` is exactly 10 characters of the network's join alphabet (lower case letters and digits without `0`, `1`, `l`, `o`, from `anyplay_api`), in either case; a longer text, a shorter one, extra segments or a query are errors. A waiting code is dropped after 30 minutes, a second join link replaces it, and a join code is never written to the log.
- At most 2048 bytes. No control characters, quotes, backticks or carets (a quote could add arguments to the registered command line). Of all arguments only the first counts.
- A folder path must be an absolute drive path: no `..` or `.`, no network or device paths (`\`, `\?\`), no stream names, no empty parts, no names that end in a dot or space, no `CON`/`NUL`/`COM1`, at most 1024 bytes. It must then be a dump (`library::inspect`), and a dump outside the games folder is not added.
- A link that arrives before the first-run setup or the first scan waits (at most eight) and is shown when the library is ready.

Single instance (`crates/anyps5-launcher/src/instance.rs`): the first launcher takes the mutex `Local\AnyPS5Launcher-main` and serves the pipe `\.\pipe\AnyPS5Launcher-main-<user>` (message mode, remote clients refused, default security so only the user can write). A later launch, with a link or without, allows foreground activation, writes its arguments, waits up to 3 s for "ok" and exits; the running launcher restores and focuses its window and acts on the link. If nobody answers the later launch starts normally. Previews and `--gallery` take no part; `ANYPS5_INSTANCE=<name>` gives a test run its own mutex and pipe, so the demo can be the receiving instance without touching a real launcher, and `ANYPS5_LINK_SCHEME=anyps5-test...` makes a test run answer another scheme name.

Registration (`anyps5_core::linkreg`): `HKCU\Software\Classes\anyps5` with `URL Protocol` and `shell\open\command` = `"<exe>" "%1"`, and `HKCU\Software\Classes\Directory\shell\AnyPS5\command` for the "Open with AnyPlay Launcher" entry (folders only; the key names keep the old name). The installer writes both (`HKA`, removed on uninstall); the portable exe writes and removes the same keys with the Behavior toggle "Open AnyPlay Links with This Launcher", which shows whether the keys point at this exe.

### 9.7 The launcher's name

Up to v0.1.3 the launcher was called AnyPS5 Launcher. It is AnyPlay Launcher now: a platform trademark has no place in our names ([the line is drawn at the law](https://github.com/anyplaynet/network/blob/main/docs/adr/0021-the-line-is-drawn-at-the-law.md)). AnyPS5 still names the engine the games run on, and the About text says so.

- **Renamed:** every visible name of the product (window titles, texts in both languages, Discord, reports, the controls file header), the exe and the release files (`AnyPlay-Launcher-<v>-*`), the installer's name, folder and shortcuts, the Linux binary, desktop entry and icon, and the app id.
- **Kept, because installs and links already use them:** the data folders (`%APPDATA%\AnyPS5`, `%LOCALAPPDATA%\AnyPS5`, `~/.config/anyps5`, `~/.local/share/anyps5`, `Pictures\AnyPS5`), the `.AnyPS5` store in games folders, `org.anyps5.<titleid>`, the `anyps5://` scheme and the registry key names, the `ANYPS5_*` variables, the single-instance mutex and pipe names, the log-start marker, the settings format tag, the installer's AppId, the crate names. (The Discord application was kept until it got AnyPlay's own, 7.4.)
- **Self-update across the rename:** v0.1.2 and v0.1.3 update only from `AnyPS5-Launcher-<v>-x64.exe`. Every release therefore also carries the portable exe under that name, byte for byte the same and listed in `SHA256SUMS.txt`; the release workflow checks both. Those launchers check no signature, so the signed manifest is added beside these files and never replaces them. This launcher prefers the new name and falls back to the old one.
- **Installer upgrade:** the same AppId upgrades an old install in place and keeps its folder (`...\AnyPS5 Launcher`). It closes a running old launcher, removes `AnyPS5-Launcher.exe`, its `.new`/`.old` leftovers and the old shortcuts. Taskbar and Start pins of the old exe do not carry over.
- **Linux:** the app id is `anyplay-launcher`, the same as the desktop entry's file name, `Icon` and `StartupWMClass` (a test holds them together). A desktop entry copied from an older tarball no longer matches the window: replace it and pin the launcher again.

### 9.8 Clips

While a game runs, the launcher can keep its last seconds and save them as a video. It is Windows only for now: the recorder is the share worker (the network repository's `spec/clips.md` and ADR 0028), which captures the game's window with Windows Graphics Capture and encodes with the GPU's H.264 encoder.

- **Setting.** Settings > Behavior > Clips: Off (the default), 15 seconds, 30 seconds, 1 minute or 2 minutes (`clips_seconds`). Below it, while Clips is on, the row **Clip Key** (`clips_hotkey`, default `Alt+F12`, a key row like the talk key: choose it, press the keys, Esc keeps the old ones; a letter or digit alone is refused so that it cannot be taken from the game, a function key may stand alone) and **Show Clips**, which opens the screenshots folder and says how many clips it holds. With Clips off the Show Clips row stays only while there are clips. The row says that a clip has no sound.
- **When it runs.** Nothing runs with Clips off or without a game: no worker, no hotkey. With Clips on, the launcher starts the worker when the game has a process (`--clips SECONDS`, the window and processes as for share screen) and ends it when the game ends or Clips is switched off. A worker that ends (no hardware video encoder, no window, a failed encoder) is told once in a toast and not started again for that game.
- **Saving.** Three triggers, one effect (`Effect::SaveClip`): the global hotkey (`RegisterHotKey`, so the game does not also see it), the controller's Create button (the `Select` button of the pad backend: Create on a DualSense, View or Back on an Xbox pad) and the phone's `create` message. The worker writes `<game name> <date> <time>.mp4` to the screenshots folder under a temporary name and renames it when whole. A clip is at least the chosen length and up to 2 seconds longer, starts on a key frame and is 1280 by 720 at 30 pictures a second.
- **Feedback.** A toast "Clip saved" (camera icon) and the unlock cue. A launcher hidden by "Hide Launcher While Playing" shows no toast, so the cue is the confirmation then. Create with Clips off in a running game says how to turn it on.
- **Cost.** A hardware encoder only, 6 Mbit/s, at most 160 MiB of memory in the worker; a PC without one gets the toast "Clips need a graphics card with a video encoder" and no software encoding.
- **Not yet.** The game's sound (process loopback capture, the same open task as share screen's sound), Linux, an in-game confirmation, playback inside the launcher (the folder opens in the system's file manager and a player).
- **Gallery.** `--gallery=behavior-clips-off|behavior-clips|behavior-clips-key`.

### 9.9 Add to Steam

Players who live in Steam (Big Picture, the Steam Deck) can put AnyPlay there as non-Steam shortcuts. The module is `anyps5_core::steam_shortcuts` (one call, `install`); the launcher's side is `app/steam.rs` (which entries) and `app/steam_art.rs` (the pictures).

- **Rows.** Settings > Behavior, Windows and Linux: **Add to Steam** (the launcher itself) and **Add My Games to Steam** (one entry for each game of the library; the row is there only while the library has games). Both are commands, and the answer is one toast. Entries are named **AnyPlay Launcher** and the game's own title from its `param.json`; no platform name is in any of them.
- **Where it writes.** For every Steam user folder `userdata/<account id>` (not `0`): `config/shortcuts.vdf`, and the pictures in `config/grid/`. Nothing else, and never Steam's servers. Steam is found at `HKCU\Software\Valve\Steam\SteamPath` on Windows, and on Linux at `~/.steam/steam`, `~/.local/share/Steam` and the Flatpak folder `~/.var/app/com.valvesoftware.Steam/.local/share/Steam` (a link to the same folder counts once).
- **The file.** `shortcuts.vdf` is Valve's binary KeyValues (maps, strings and 32-bit integers, each map closed by 8). The module reads it as a tree and writes it back, so fields the player or Steam put there survive byte for byte; a file in any other form is left untouched and the toast says so. The first time the module changes a file it keeps the old one as `shortcuts.vdf.anyplay-backup`; writes go through a temporary file and a rename.
- **The entry.** `Exe` is the launcher (on Linux an AppImage names the AppImage file, not its temporary mount), `StartDir` its folder, `LaunchOptions` empty for the launcher and `anyps5://game/<TITLEID>` for a game: the deep link of 9.6, which the strict parser must take, so a game entry opens the launcher on that game and the player presses Play (a link never starts a game). The app ID is the CRC-32 of the quoted exe plus the name with the high bit set; it is stored as the entry's `appid` and is the prefix of the pictures.
- **Never twice.** An entry with the same launch options and the same exe, or the same name, is updated in place; its name, tags and app ID stay the player's. An empty `shortcuts.vdf` counts as none. A second press, a moved launcher or a game that came later adds only what is missing.
- **Steam must be closed.** Steam keeps the list in memory and writes it back when it exits, which wipes an edit made while it runs. The rows therefore write nothing while Steam runs (Windows: the process ID under `HKCU\Software\Valve\Steam\ActiveProcess`; Linux: `steam.pid`) and say "Steam is running. Close Steam, then try again." The launcher never closes Steam. After a write the toast says to start Steam again.
- **Pictures.** The launcher's: tall and wide capsule, hero, transparent logo and icon, drawn from its disc and name. A game's: its `icon0.png` as the icon, `pic0.png` (or `pic1.png`) cut to the wide capsule and the hero, and a tall capsule of the darkened picture with the icon on it. They are read from the dump and written as new files into the player's own Steam folder; they go nowhere else, and the dump is never changed. A game without pictures gets an entry without art. For an entry that is new, a leftover picture of the same kind (a JPEG of a removed entry would hide our PNG) is replaced; for an entry that was already there only the missing pictures are written, so a picture the player chose stays. Two games with one title are told apart by their title ID in the entry name ("Name (PPSA01234)"), because Steam derives an entry's ID from the exe and the name and every game shares the exe.
- **Steam Deck and Steam Input.** Started from Steam on Linux, the launcher is the same program as ever. Steam Input hands it a virtual Xbox 360 pad next to the real one; the pad layer already deals with that case (the Xbox family and glyphs for the Microsoft vendor id, and a hand-over to another pad only after the pad in use has been quiet for 600 ms; see the controller notes above), so the focus ring, A and B and the sounds should behave as with a plain Xbox pad. This is reasoned from the code and the pad layer's tests: **it has not been run on a Steam Deck, or with Steam Input on any hardware.** What no test here can show: whether Gaming Mode gives the launcher the foreground and a 1280 by 800 window, whether the AppImage starts from a Steam entry as it does from a file manager, and whether a game started from there runs (game launch on Linux is untested, and Flatpak Steam runs a shortcut inside its sandbox, which cannot see a launcher outside its folders).
- **Not done.** Removing the entries (the player removes them in Steam), keeping them in step with the library by themselves, macOS.
- **Gallery.** `--gallery=behavior-steam` shows the two rows.

---

### 9.10 Captures

Screenshots and clips have a place of their own: a full-screen gallery, like a console's. The module is `anyps5_core::captures` (reading the folder: the list, the owner of a file, the length of a clip, delete); the screen is `screens/captures.rs` (layout in `captures/layout.rs`, drawing in `captures/paint.rs`); the data and the pictures are `app/captures.rs` and `app/captures/images.rs`; the glue is `app/panels/captures.rs`. The overlay is `Overlay::Captures`, a full-screen panel with the library's glass.

- **Reached from.** The captures button in the top bar (`images`, between the library and friends; `HomeIntent::OpenCaptures`) and the row **Captures** in the quick panel the Home button opens in front of a running game. Back leaves, to where it was opened from.
- **Where the files are.** The screenshots folder (Settings > Advanced; `Pictures/AnyPS5` by default), top level only; links are not followed. A screenshot is `<TITLEID>-<yyyymmdd>-<hhmmss>[-<n>].png` (the engine's), a clip `<game name> <yyyy-mm-dd> <hh-mm-ss>[ (<n>)].mp4` (9.8). `.png`, `.jpg`, `.jpeg` and `.mp4` of any other name count too, as captures of no known game. The folder is read when the area opens, on a thread of its own, and again on **Try Again** or when the folder in the settings changes.
- **Grouped by game.** Each group has the game's own icon and name from the library; a screenshot is matched by the title ID in its name, a clip by the game name in its name (the same safe-file-name rule the recorder uses). A game the library does not know is a group under the name the file gives it, files of no game are **Other Captures**. Groups are ordered by their newest capture, captures inside a group newest first. Nothing in this is specific to one title.
- **The grid.** 16:9 tiles that fill the width (as many as fit at about 300 dp, at least two), 20 dp between them, the time under each (`14:32` today, `Oct 25, 14:32` before). A clip carries a play disc and its length in the corner. The first row of a group has the group's header above it. Filters **All**, **Screenshots** and **Clips** are chips under the title (the chosen one is the filled button); select on a chip chooses it.
- **Focus.** One 2D grid across the groups: left and right step one place and cross from a group to the next, up and down keep the column (into a shorter row the last tile), up from the first row reaches the chips, down from the chips the first tile. Every step plays `move`, an end `bump`, opening `open`, leaving `close`. Tab walks like the library. A click on a tile opens it.
- **One capture.** A screenshot fills the space between the title and the buttons (the small picture shows at once and the big one, at most 2560 px on the longer side, replaces it). A clip shows its picture and its length, and plays in the system's player (**Open in Player**); the launcher has no video decoder. The picture of a clip is the system's own thumbnail (Windows: the shell's thumbnail of the file, 512 px for the grid and 1024 px for the view, asked on a worker thread of its own so that a hung provider cannot hold up the screenshots; Linux: none), and when there is none the tile is plain with the play disc. The title names the game and "3 of 4" counts within it. Left and right step through the captures in the order of the grid, from one game to the next, and a picture that is already there appears at once (one that has to be waited for fades in); down reaches the buttons: **Open in Player** (clips, the one filled button) or **Copy Image** (screenshots, to the clipboard), **Show in Folder** (Windows and macOS select the file, Linux opens the folder) and **Delete**. Select on a clip's picture plays it; on a screenshot it moves to the buttons. Back returns to the grid on the capture last seen.
- **Delete** asks first, with the file's name in the question, and opens on **Cancel**: "Delete this screenshot?" / "Delete this clip?", "<name> is removed from your PC. This cannot be undone.", **Delete Screenshot** / **Delete Clip** and **Cancel** on the confirmation card (5.9.1). Back is Cancel (back to the Delete button). The file is removed for good (no recycle bin). Only files the engine or the recorder named can be deleted: a picture or video of the player's own that merely sits in the folder (a screenshots folder pointed at a general Pictures folder) is listed under **Other Captures**, can be viewed, played and shown in its folder, and has no Delete button; `delete` refuses it too. After a delete the view stays at the place and shows the next capture, or the previous one when it was the last, or the grid when the list is empty.
- **States.** *Loading* (a spinner and the words), *empty* ("No captures yet", how to take a screenshot, F12, and on Windows how to turn Clips on, with **Open Clips Settings**), a filter that hides everything ("No screenshots yet" / "No clips yet", **Show All Captures**), *folder missing* (the folder's path, **Open Folder Settings**) and *unreadable* (**Try Again**). Each has the chips above it and at most one button.
- **Thousands of files.** Only the rows on screen and one more are laid out and drawn; the scroll follows the focus as in the library. Pictures are decoded by two workers (and the clip thumbnails by a third) off the interface thread; what a frame asks for is the queue (what was scrolled past leaves it); the texture cache holds at most 160 MiB and gives up the ones drawn longest ago; a file that does not decode is asked for once and shows as a plain tile; a picture of more than 64 million pixels is not opened. Closing the area frees the textures.
- **Never.** Nothing is uploaded or shared, nothing is moved or edited, no game dump is touched; the only write is the delete the player confirmed.
- **Gallery.** `--gallery=captures-empty|captures-few|captures-many|captures-clips|captures-screenshot|captures-clip|captures-clip-plain|captures-confirm|captures-missing|captures-loading`. All names, times and pictures are invented.
- **Not yet.** Video playback inside the launcher, a picture for a clip on Linux, a recycle bin, sharing.

## 10. Asset list

| Asset in the ARM repo | Reuse? | Notes |
|---|---|---|
| Swift source (layout numbers, strings, shader math, sound parameters, input rules) | **Yes**, GPL-2.0 | Derived work. Credit utkuhalis/AnyPS5-ARM in the README (done) and keep GPL-2.0-only. This document records the values; the code is re-implemented in Rust |
| Localization strings (EN, TR) | **Yes**, verbatim, GPL-2.0 | Section 8 plus the Windows replacements in 9.3 |
| App icon | **Recreate** | The macOS icon is generated by `tools/macos/launcher/icon.swift` (no PNG is committed). Recipe: 1024 canvas, 824 rounded square (inset 100, radius 185) with a vertical gradient from (0.10, 0.30, 0.80) at the top to (0.02, 0.05, 0.20); radial glow (0.5, 0.75, 1.0) alpha 0.55 to 0, radius 520, centred 160 from the square's right edge and 120 from its top; five flowing curved bands (stroke (0.7, 0.85, 1.0) alpha 0.12 + 0.05 n, width 5 - 0.6 n, glow blur 18); a centred white ring (diameter 460, width 26, centre 20 above the middle) with a rounded white play triangle (corner radius 34), black 0.4 drop shadow. `crates/anyps5-launcher/assets/icon.ico` already exists in this repository; regenerate it from the recipe as a multi-size ICO (16, 24, 32, 48, 64, 128, 256) if it does not match |
| Screenshots in `docs/screenshots/` and `docs/videos/launcher.mp4` | **Link only, do not copy** | They show Hollow Knight: Silksong, Plants vs. Zombies, Minecraft, Doom, Stray and other third-party artwork and trophy icons. Make our own screenshots with our own test data |
| Sounds | **Recreate** (nothing to copy) | Synthesised; exact parameters in 7.1 |
| Fonts | **Do not reuse** | SF Pro is Apple-licensed. Use Inter (OFL 1.1) and JetBrains Mono (OFL 1.1); ship their licence files and mention them in the README |
| Icons | **Recreate** | SF Symbols are Apple-licensed; use Phosphor (MIT), ship its licence |
| Theme backgrounds | **Re-implement** | WGSL in 3.4 (derived from the Metal shader, GPL-2.0) |
| Game artwork, trophy icons | **Never bundled** | Read at runtime from the user's own dumps |
| Controller face glyphs `✕ ○ □ △` | **Recreate** | Painted vector shapes |

---

## 11. Implementation notes for egui

- **Immediate mode with retained animation state.** Keep a map `widget id -> Spring` (in `ctx.memory` or the app struct); call
  `ctx.request_repaint()` while any spring or timeline is active, `request_repaint_after(...)` for timed items (clock minute, splash
  second tick, toast and banner timers) and nothing when idle with the background paused. While only the shader animates, repaint every 30 ms (target about 30 fps, not yet measured); springs, fades and input still draw at the display rate.
- **One command pipeline.** Translate egui key events, wheel and `gilrs` events into the seven commands (section 6) in a single
  function that is their only consumer; do not rely on egui's keyboard focus traversal (draw focus from the model: `HomeFocus`,
  `panelRow`, ...). Text fields are the only widgets that see raw keys.
- **Custom widgets with the painter** (rounded rects, strokes, `epaint::Shadow`, galleys with `TextFormat`); use an inside stroke for the
  1 px hairlines (SwiftUI `strokeBorder` draws inside the shape) and an outside one for focus rings.
- **Scale**: `pixels_per_point = os_ppp * scale` (1.1).
- **Offscreen preview renderer.** The original has `AnyPS5 --render-preview <out folder> <games folder>`, which draws every screen to a
  PNG at 1600 x 900 without opening a window, with deterministic data (profile name "Utku", sounds off, Turkish unless
  `ANYPS5_PREVIEW_LANGUAGE=en`, `ANYPS5_PREVIEW_SELECT=<title id>`, `ANYPS5_PREVIEW_BANNER_BACKDROP=<png>`). Build the same for visual
  regression, with these states: `home-theme0..2` (artwork off), `home-artwork`, `home-actions` (focus on actions 0), `home-unlocking`
  (copy step, 40 %), `starting`, `game-settings` (row 1), `controls` (row 1), `controls-capture` (row 3 waiting, "Square"), `advanced`
  (row 4), `home-trophies` (focus on actions 1), `trophies` (index 1), `settings` (row 0) and `settings1..5` (each category), `library`
  (index 1), `trophy-banner` (settled card over a backdrop picture), `onboarding0..2`. Theme stills are rendered at `time = 260` (full
  frame) and `time = 200` (660 x 372 cards); the selected game in previews is index 1.
- **Checks against the original**: compare each preview with the matching screenshot from the table at the top: tile row x positions
  (64, 256, 556, 690, 824), the hero title near y 598 and info cards at y 727..823 (S-home-trophies, 1600 x 900), hint bar baseline near
  y 862, settings list x = 456 (S-settings), side panel x = 1040 (S-game-settings).

---

## 12. Open points (not decidable from the Swift source)

1. Exact material tint and blur sigma of `.ultraThinMaterial` (estimates in section 4).
2. The `egui blur = 2 * SwiftUI radius` shadow rule (1.4) comes from the usual CALayer / CSS relation, not from a measurement.
3. ~~The WGSL port is untested.~~ Resolved in the ui-foundation PR: all shaders compile and validate with naga (unit tests in
   `ui/gpu.rs`) and the Cosmic card matches `launcher-themes.jpg` to within one level per channel.
4. What "unlock" does on Windows, which fixes the step names and the `installedSize` text (9.3).
5. Whether the Windows toolkit honours the same `ANYPS5_*` environment variables (5.9).
6. The Turkish date rendering of trophy rows and the exact byte-size rounding of `ByteCountFormatter` were not observed.

---

## 13. Implemented foundation (crate `anyps5-launcher`, module `ui`)

The shared UI layer of this specification. Screens use these modules and nothing else from the foundation.

| Module | Contents (section) |
|---|---|
| `theme` | tokens 1.1-1.5 (`color::*`, `radius::*`, `size::*`, `pad::*`, `shadows::*`), type scale 1.6 (`ty::*`, `font(size, Weight)`), bundled fonts, `sync_scale` |
| `anim` | spring solver 2.1, `Bezier` 2.2, `Motion` presets with reduced-motion curves 2.3, `AnimExt` (`ctx.animate_to`, `focus_t`), endless loops 2.4 (#17, 17b, 26) |
| `background` | `Backdrop` (clock 3.1, theme cross-fade), `Background` paint callback, WGSL 3.4 |
| `blur`, `glass` | section 4: Gaussian chain on 1/8 resolution, `GlassStyle` presets, `paint_glass` |
| `components` | `ConsoleButton`, `IconButton`, `Switch`, `ProgressLine`, `IndeterminateLine`, hint bar, key caps, `PadGlyph`, focus glow and ring |
| `chrome` | `paint_tile` (5.1), `info_card` / `facts_line` (5.3), `PanelRow` with all kinds (5.8), `fullscreen_panel` (the frame of 5.9 is `screens::side`), `toast` (5.6), `language_card`, `onboarding_dots` |
| `text`, `icons` | line height, tracking, middle truncation, tabular figures; Phosphor constants (5.16) |
| `sound` | section 7.3 (replaces 7.1): eight cues rendered once, home music generated live, one mixer on its own thread, lazy device |
| `i18n` | section 8: 212 keys, `Language::text/format`, `byte_text`, Windows replacements of 9.3 applied |
| `input`, `focus`, `gamepad` | section 6: `Command`, `Input::poll`, `HomeFocus::navigate`, `PanelNav::command` |
| `gallery` | hidden `--gallery[=components|panels|themes|bg0|bg1|bg2] [--screenshot=out.png] [--size=WxH] [--time=T]` |

Decisions made while implementing:

- **Focus is driven by the model**, never by egui's keyboard focus. Widgets take `focused: bool`; egui only supplies pointer state.
- **Press feedback** (Windows addition, 9.2): buttons and cards scale to 0.97 while the mouse button is down. Hover stays instant.
- **Focus glow before the element, ring after it**: egui shadows are filled, so the glow is painted behind the element
  (`paint_focus_glow`) and only the stroke on top (`paint_focus_stroke`).
- **Scaled text** (focused button 1.05, language card 1.04) uses font sizes quantised to 0.25 dp so animation reuses a small set of glyph rasters.
- **Glass** composes in the shader: `mix(sharp, material, 0.55)` then black, then the 1 px inner stroke; the material is the
  blurred backdrop with saturation 1.25 and a 6 % white tint (both estimates, tune against the original).
- **Keyboard repeat** of select/back/options is suppressed (a held Enter must not re-fire); held arrows repeat at the OS rate.
- **Row padding**: panel rows keep 10 dp above and below their text even when the title wraps (the original has none).
- The gamepad thread only translates events; the 0.36 s / 0.10 s / 0.06 s repeat runs on the UI clock (`input::Repeater`).

---


## 14. Implemented panels (crate `anyps5-launcher`, module `screens`)

One module per panel; each has a state struct and a `show(...)` that takes the `Shell`, the egui context and the frame's
`InputEvent`s and returns the actions for the host (the home screen). Previews: `--gallery=settings|settings1..6|game-settings|confirm|
controls|controls-capture|advanced|trophies|trophies-hidden|trophies-revealed|banner|start|unlocking|log|library` over invented data and generated artwork.

| Module | Screen | Notes |
|---|---|---|
| `settings` | 5.7 app settings | Categories: Appearance, Library, Language, Behaviour, **Toolkit** (new), Advanced, About |
| `game_settings` | 5.9 "..." panel, uninstall confirm | `GameSettings` of the core edited in place |
| `controls` | 5.9 controls panel | key capture by physical key (egui `physical_key`), modifiers capture as Left Shift/Ctrl/Alt/GUI |
| `advanced` | 5.9 advanced panel | export/import through native dialogs, `SharedSettings` of the core |
| `trophies`, `banner` | 5.11, 5.14 | banner timeline is a plain state machine; `BannerHost` shows it in its own viewport (9.4) |
| `start`, `unlock`, `log` | 5.5, 5.4, 5.12/5.13 | steps and diagnosis are the pipeline's `Step` and `Diagnosis` |
| `library` | 5.10 grid | host supplies icon, overlay and state line per game |

Deviations (all Windows additions, the original stays the reference):

- **Toolkit category** in the settings: installed version, update status, "check for updates" switch (`checkForUpdates`), check now,
  install or update, use a toolkit folder, CPU conversion (`cpuConversion`). Texts in `screens/strings.rs` (EN/TR).
- The **video background** choice is hidden (picture only, DESIGN 3.5); a stored video choice stays visible.
- Controller face glyphs in the controls rows are painted shapes (`PanelRow::pad_glyph`), not font glyphs.
- Row detail lines wrap at words (the first foundation version broke inside words).
- A trophy row's focus has the ring only; the glow of the foundation would show through the translucent row.
- Keypad keys arrive as their top-row twins (egui reports one key for both).
## 15. Implemented home and app (crate `anyps5-launcher`, modules `app` and `screens`)

| Module | Contents |
|---|---|
| `app::model` | the state machine of `AppModel.swift` without I/O: game states in priority order, home/onboarding commands, unlock queue and progress (coalesced), start/running/exit, toast (3.2 s), overlay open/close, library bookkeeping. Work for the outside is queued as `Effect`s; results arrive through `on_core` and `on_pipeline` |
| `app::pipeline` | the seam: `PipelineCommand` (Unlock, Retry, Cancel, Play, Stop, UnlockAgain, Activate), `PipelineEvent`, trait `Pipeline`; `NoPipeline`, `SimPipeline` (demo and tests) |
| `app::core_pipeline` | `Pipeline` on top of `anyps5-core` (`UnlockQueue`, `Launcher`, diagnosis texts in the launcher language) |
| `app::overlay` | `Overlay` enum and trait `OverlayHost` that the panel modules implement (`command`, `ui`, `captured_key`) |
| `app::services`, `app::textures` | scan + watcher, size and trophy workers, folder dialog (`rfd`), icon and artwork decoding off the UI thread |
| `screens::{home, onboarding, splash}` | 5.3, 5.15, 5.5 |
| `app::preview` | hidden `--gallery=<scene>` and `--demo` with made-up games (see the module doc) |

Decisions and deviations:

- **Glass cards over the artwork.** The foundation's glass blurs the theme backdrop only. The info cards now also show the frosted game artwork: the artwork is baked once into an 80 px blurred, saturated, tinted texture and drawn under the card through the same uv rectangle as the artwork, darkened by the glass black and the two scrims. The tint (0.30) is tuned by eye against `launcher-home-trophies.jpg`.
- `GlassStyle` got an `alpha` (and `paint_glass_on` multiplies the painter's opacity), so glass inside a fading screen fades with it.
- Onboarding to home is a fade-out then fade-in (0.55 s in total), not a cross-fade of both screens.
- The hero title has no text shadow (the scrims carry the contrast); the empty-library icon is the regular weight (no thin weight in Phosphor).
- The artwork is decoded to at most 2048 px (the original: 1.6 x the window).
- Dev builds compile dependencies with `opt-level = 2` (image decoding is unusably slow otherwise).
- Previews for local comparison can take real art from a dump on the machine (`ANYPS5_PREVIEW_REAL_ART=<dump folder>`); nothing of it is copied.

## 16. Implemented panel wiring and window chrome (crate `anyps5-launcher`, modules `app::panels`, `app::toolkit`, `app::window_chrome`)

`app::panels::PanelHost` is the real `OverlayHost`; the launcher and `--demo` run with it (`NoPanels` stays for tests of the model).

| Overlay | Panel | Opened from | On close |
|---|---|---|---|
| `Settings` | `settings` | options button, `Ctrl+,` | back to home |
| `Library` | `library` | library button | pick selects the game (focus on the tiles) and closes |
| `GameSettings` | `game_settings` | "..." button | back to home |
| `Controls`, `Advanced` | `controls`, `advanced` | rows of the game settings | back (the Back button too, the same step) returns to the game settings on the row that opened them (no exit) |
| `Trophies` | `trophies` | trophy button, row of the game settings | back to home |
| `Log` | `log` | "Open log" row | back to home |
| `ConfirmUninstall` | `UninstallPanel` | "Uninstall" row | both answers close everything, as in the original |

- **Keeping in step.** A panel opens when `Model::overlay` becomes its overlay and closes (leaves like a full-screen area) when another overlay or none is open. The panels of the game frame (5.9) share one area, so switching between them swaps the content without an exit. Commands routed by the model are queued in the host and handed to the panel with the raw captured keys of the controls panel.
- **Persistence.** The settings panel edits `Model::prefs` in place; game settings, controls and advanced edit a copy of the game's `GameSettings` that is stored back (`Preferences::set_game_settings`) when the panel reports `changed`. Either way `Model::mark_prefs_dirty` makes the host write `preferences.json` atomically at the end of the frame (and on exit). A panel that was only looked at writes nothing.
- **Theme and accent** reach `Shell::set_theme` inside the settings panel while the choice is made (the backdrop cross-fades) and are saved with the other preferences.
- **Sounds.** The panels play their own cues, so the host closes and switches overlays silently (`Model::dismiss_overlay`, `Model::switch_overlay`).
- **Actions.** Reinstall closes the panel and queues `UnlockAgain`. Show in Explorer selects the unlocked copy (the dump folder when there is none). Open log shows the failure message and the last 60 000 bytes of the game's log, and the diagnosis sentence when the game ended on an error. Make a report (also from the log view) writes `Logs\Reports\<TITLEID>-<stamp>.txt` on a worker thread, reveals it in Explorer and toasts `reportSaved`/`reportFailed` (5.13, no save dialog). Clear shader cache deletes the game's cache folder. Uninstall deletes the unlocked copy on a worker thread (the dump is never touched).
- **Toolkit category** (`app::toolkit`): "check now", the check at start (when `checkForUpdates` is on and the launcher is onboarded; only the latest release is requested), download and install, and "use a toolkit folder" run on worker threads; the pipeline looks for the toolkit again afterwards (`Effect::ReloadToolkit`). The demo never goes online.
- **Trophies while a game runs.** `Services::watch_trophies` starts the core's `TrophyWatcher` for the running game; earned trophies queue `BannerHost` banners (own viewport, 9.4) and reload the trophy set.
- **Window chrome** (`app::window_chrome`, Windows only): after the first frame `DwmSetWindowAttribute` sets immersive dark mode (attribute 19 before 20H1), caption colour black, caption text white, border colour black (Windows 11) and round corners. A refused attribute keeps the system look; other platforms do nothing. The backdrop type stays the system default (no Mica): 9.2 asks for a black title bar that merges with the black backdrop.

Deviations: the diagnosis sentence in the log view is derived from the log text only (a crash known only from its exit code appears in the toast and the hero line, not in the log box); the diagnosis stored by the model stays a localised sentence.

## 17. Online area (crate `anyps5-launcher`, modules `screens::friends`, `screens::online`, `app::online`, `app::panels::friends`)

The launcher's side of the AnyPlay network ([anyplaynet/network](https://github.com/anyplaynet/network)): signing in, friends and their presence, requests, invitations, joining a friend's game, parties and messages. The launcher shows; the net service (`anyplay-net`) acts. They talk over the net service's standard streams with the control protocol (`spec/control-protocol.md` there); `anyps5_core::online` holds the process and the state, and `Model::online` is that state.

**Entry.** Two buttons of the top bar lead into the online area: the profile (the last stop of `HomeFocus::Top`: library, friends, settings, profile) and the friends button beside the library. Both are pills as tall as the icon buttons, on the icon buttons' surface (white 0.10, 0.16 on hover, white with dark ink when focused), with the same focus grow and glow as their neighbours. Signed out the profile reads "Sign In" and never shows the PC's name; signed in it shows the account's handle. While messages wait it ends in a count (a 20 dp capsule, light ink on the dark pill and dark on the light one it becomes when focused, `9+` above nine; the accent stays the online dot's alone), and the button's name says it: "Mira, Online, 3 unread". Its radius is the avatar's plus the 4 dp inset, so pill and avatar stay concentric. While the cloud answers a 4 dp accent dot sits at the avatar's lower right in a 6 dp ring of the pill's own colour. The friends button shows how many friends are online (section 5.3). Select on the friends button opens `Overlay::Friends`, the **Friends screen** (below), on the friends list once signed in and on sign-in otherwise. Select on the profile opens the player's own **profile screen** once signed in (below) and the Friends screen on sign-in before. The profile screen's second button, Friends, leads on into the Friends screen, so nothing the profile reached before is out of reach; the unread count of the profile is repeated there, "Friends (3)". There is no drawer any more: the online area is never a side panel.

**Answering in place.** A request row is a row with a *buttons* control: two small pills, 32 dp high and 12 dp apart, at the row's end (Accept, Decline). At rest they are quiet chips on the dark row; the row focused turns white, the pill the keys would press turns dark on it and the other stays a pale chip. Left and right choose between them, select presses the chosen one (Accept to begin with), and a click presses the pill it lands on. A click on the row's body presses neither: it opens the request's own page (where it can be blocked), so a stray click never answers. No motion beyond the row's own focus and hover. The friends list says what a friend plays ("Playing Lanternfall", "· You can join") but not for how long: presence carries no start time today, so the minutes are left out until the cloud sends one.

**Friends screen** (`screens::friends`, its host `app::panels::friends`, the return path `app::profile`). A full-screen overlay (`Overlay::Friends`) in the family of the profile screen: glass, 90 dp of side padding, the hint bar at the bottom right like on Home (without Back, since the Back button is on screen), a Back button (the console button, small, with the caret) at the top left for the pointer. It replaced the drawer of the first online area; the pages below are unchanged and are now drawn in its page column.
  - *Frame.* A header (the avatar, 64 dp, the first letter on one of six calm colours without a picture; the handle as the title, 40 light; under it the status, "Online · 3 friends active", "Connecting…" or "Offline, trying again", with the accent dot while connected). Under it, on the left, a **rail** 248 dp wide with the sections Friends, Party, Messages, Activity and Settings, and below them, set apart, Your Profile (an action: it opens the profile screen). A rail entry is 52 dp high with a 24 dp glyph and a 17 medium title; nothing sits in a box at rest. The section on screen is a faint surface (white 10 %) with a 3 dp bar at its edge, so it does not rest on colour alone; the focused entry has the white ring of the control family (3.9). What waits shows as a count capsule at the entry's end (light ink on dark, `9+` above nine): requests and invitations on Friends, party invitations on Party, unread messages and message requests on Messages. The accessible name says it: "Messages, 3 new". Right of the rail, 56 dp away and at most 940 dp wide, is the page: its title (30 light) and the line under it (15), then the rows. The title and the line stand on the left edge of the rows' text (the list's 6 dp inset plus a row's 18 dp padding), so a page has one left edge; the line is a label for the screen reader. Rows of the list fade out as the edge reaches them (opaque while a row rests inside the inset, gone when the cut reaches its text, the heights taken from the frame before), so nothing slides under the title and is cut mid-line. Before sign-in there is no rail: the page stands alone in the middle, 640 dp wide.
  - *Sections and pages.* Every page of the table below belongs to a section (`Page::section`): Friends holds Home, a friend, the requests, an invitation, Add a Friend, Report and the invitation picker; Party holds the party, a member, Invite Friends, a party invitation, Voice and the devices; Messages holds the list, a conversation, its options, quick replies and the on-screen keyboard, a request and a message; Activity is the feed; Settings is the account page, a blocked account and (when opened from there) a restriction. A restriction page belongs to the page it was opened from. The first page of each section (`Page::is_root`) is where Back leaves the screen; any other page goes up one.
  - *Friends page.* The default. From the top: the state of the connection when it is not up (Connecting… as a note; offline as a note with Try Again, which starts the net service again), a limit on the account, friend requests (up to five, each with its avatar, "Wants to be your friend", Accept and Decline), invitations (a game's, then a party's), Playing Together (only once someone besides you is in the session), Add a Friend, then the friends in three groups with their own captions: Playing Now (joinable first), Online, Offline, then Sent Requests. A friend's row is the avatar (36 dp; a dot in the accent while they are here, quieter while they are not), the handle, what they play, and the quick actions at its end: **Join** (only when their session is open to friends), **Message**, **Invite to Party** (when the player may add to the party and there is room), **View Profile** and **Options** (their page: call, remove, block, report). The actions rest hidden and come up with the focus or the pointer; select presses the chosen one (the first to begin with), left and right choose, and a click on the row's body opens Options. A friend with no profile picture on this PC shows the monogram. With no friends yet the page says "No friends yet. Add one by the name they chose." over Add a Friend.
  - *Party page without a party.* The first page of Party stands without one: a note ("A party keeps you together across games, with chat and voice."), the party invitations that wait, Start a Party and Voice. In a party it is the party page of the table.
  - *Party room.* In a party the page is a room, not a list (`screens::online::room`): the members as large avatars in a grid of equal tiles (88 dp avatar, the handle, one line in words: speaking, muted, deafened, not in voice, else online or offline, and the game they play), the leader with a chip, a ring in the accent round a member who speaks and a mark (a muted microphone or speaker) on an avatar that is muted or deafened; an empty seat for the leader while there is room. Under the tiles the cards of what the party can do together (take the party into the session, the streams: one button each), then the player's voice (Mute, Deafen, Voice Settings; Turn On Voice while it is off; Open in Discord with a link on Discord) and the party's actions (Invite Friends, Party Chat, Share Screen), with Leave Party alone at the far end of the same row, away from the everyday ones; the voice buttons carry no caption of their own. The groups are 24 dp apart. One primary action: Invite Friends in a party of one, else the button that takes the party into the game. Pressing a member opens the member page (volume, mute, leader) and Back returns to that member. The focus is a grid of its own (`room::step`): left and right within a row, up and down between rows, the column kept as far as the next row reaches; Left on the first stop leaves for the rail. A page that goes on below the screen fades its bottom edge (party room); without a party the page has one picture, one sentence and Start a Party, with the invitations that wait as cards. The rows of the page stay the model of its title and tests. Gallery scenes: `friends-party`, `friends-party-pair`, `friends-party-four`, `friends-party-muted`, `friends-party-alone`, `friends-party-voice`, `friends-party-none`. Others' own mute and deafen are not reported by the net service, so a tile shows only the player's own and what the player muted.
  - *Focus.* Two zones (`friends::nav::step`, pure and tested): the rail and the page. Up and down walk the rail and show the section they stop on; right or select crosses to the page; left on the page crosses back to the rail once the row under the focus has nothing left to step through (a row of buttons or a choice keeps left until it is on its first). Back from the rail or from the first page of a section leaves the screen. The pad, the keyboard and the pointer reach every entry and every row; the screen reader follows the ring (`ui/a11y.rs`) and reads the rail as buttons, the current section selected.
  - *Back.* Back returns to where the screen was opened from, like the profile screen: home for the friends button and the profile button before sign-in; the profile screen when it was opened from there (`Model::friends_back`). A friend's View Profile opens their profile screen; Back from it comes back to the Friends screen on the page and the row it left.
  - *Motion.* The screen comes in and goes out with the overlay spring of the other full-screen panels (opacity and a scale from 1.03); nothing else moves except the rows' own focus and hover, and the quick actions fade with the focus. Reduced motion swaps the springs for the short eases of every other overlay (`AnimExt`); no motion of this screen is the only carrier of a state.
  - *States.* Signed out: the Start page (Sign In, Create Account, then the same sentence as a note with two rows, Terms of Service and Privacy Policy, which open the pages in the browser (2026-10-26), then the cloud) in the middle. Offline and connecting: the note and Try Again. Loading and failed: the feed has its own block (3.x); the friends page shows what it has. No friends: above. A ban: the Start page keeps the first row. A mute: the banner row at the top of Home (Restrictions below).
  - *Words.* Titles and captions in title case, notes in sentence case. English and Turkish in `friends_strings.rs`.

**Pages.** One screen whose pages swap in place (no slide); Back leads up a page and leaves the screen at the first page of a section. Each page opens on its first focusable row.

| Page | Title / subtitle | Rows |
|---|---|---|
| Start | "Online" / "Play with your friends over the internet" | Sign In and Create Account (commands; both open the cloud's page in the system browser, detail "Set up the cloud first." without a cloud), Cloud (action, detail "AnyPlay" or the address), note: signing in and creating an account happen in the browser, nothing goes online before |
| Cloud | "Cloud" / "The address of the cloud you sign in to" | Address (text), Use This Cloud (command; detail when the address is not https or loopback http), Use the AnyPlay Cloud (only with an own cloud), note |
| Browser | "Online" / "Continue in your browser" | Open the Page Again, Cancel, note: the launcher signs in by itself when the browser is done |
| Home | "Friends" / none (the header says who and how many) | The state of the connection when it is not up (a note; offline also Try Again), a limit, Friend Requests (up to five, each a row with the avatar, the handle, "Wants to be your friend" and Accept and Decline at its end; with more, See All Requests with "5 waiting" opens the Requests page), Invitations (a game's, then a party's), Playing Together (only once someone besides you is in the session; members with Direct, Through a relay or Connecting…), Add a Friend, Friends in the groups Playing Now (joinable first), Online and Offline (by name), each with its quick actions (above), Sent Requests |
| Friend | the handle / what they play | Join Game (only when their session is open to friends; detail "2 of 4 players"), Send Message (detail "Opening…" or "2 new"), Invite to Party (when the player leads a party or has none, and the friend is not in it), Invite to Your Game (when you are in a session they are not), View Profile, Trophies (their gamer card: the total, the grades they have below it, highest first; "None yet" or "Hidden"), then the trust actions in a group of their own: Report, and after a gap Remove Friend and Block (both destructive, both ask first, see Confirmations) |
| Requests | "Friend Requests" / "5 waiting" | Every request as the same row, none capped; the page ends by itself with the last answer |
| Request, Sent, Blocked, Invitation | the handle or "… invites you" | Accept / Decline / Block; Withdraw Request; Unblock; Join Game / Decline |
| Party | "Party" / "3 of 8 members" | Party Chat (detail "2 new") and, for the leader, Invite Friends (not full) and Take the Party to My Game (detail the game; only in an open session and with company), in that order, except that a party of one starts with Invite Friends; the Voice group (Party Voice, and while it is on the microphone, the mode and their notes, then Voice Settings); the Members (each with "Speaking", "Muted" or "Not in voice" while voice is on, "Leader" and the friend's presence in words, "You" on your own row; the others open the member page); Leave Party (destructive, its own group, asks first) |
| Party member | the handle / presence | Volume (stepper, 0 to 200 %, steps of 10) and Mute (toggle), for everyone; for the leader a group below with Make Leader (detail "Lumen will lead the party") and, in a group of its own under it, Remove from Party (destructive, asks first; detail "Lumen is taken out of the party") |
| Voice | "Voice" / "Talk with your party" | Party Voice (toggle; detail the state), a note when the net service switched it off, Mode (Push to Talk, Voice Activity) with its note, Talk Key (keys row; only in push-to-talk mode), a group with Microphone and Speakers (actions; detail the device or "System Default"), Refresh Devices, a note when the PC has no microphone or sound at all |
| Voice devices | "Microphone" or "Speakers" / "Choose the microphone" | System Default first, then every device by name; the chosen one says "In use", or "This device is not connected right now." when it is gone; a note that a change restarts voice for a moment. Choosing leads back to the voice page |
| Invite Friends | "Invite Friends" / "Pick up to 5 friends" | one toggle per online friend not in the party, with their avatar and presence; a pick beyond the free places is refused; Send Invitations (detail "Pick a friend first" or "The party is full" while it cannot send), Cancel. The party exists once the cloud answers |
| Party invitation | "… invites you" / "Party of 3" | Join Party, Decline; while the player has a party of their own Join Party is not offered, a note says to leave it first, and the page opens on Decline |
| Messages | "Messages" / none (the rail counts what is unread) | Message Requests (a group of its own: the sender, a "Request" tag), then Conversations: pinned first, then the newest; each is its picture, name, newest message in one line and time (see *The list*, drawn in `chat.rs`, not as rows; the rows of this page are the model of its tests); the party's chat is "Party Chat"; with none, a note on how to start |
| Conversation | the friend, the group, or "Party Chat" / presence, "n people", "3 of 8 members" | Show Earlier Messages (while there are more), the messages oldest first as bubbles with the newest just above the field (see *The thread*; the time, Edited, Delivered or Seen stand once under a group), "Loading…" until the first fetch came back, the Message field (wide, Enter sends), Send; a group adds Leave Conversation (asks first). It opens on the field |
| Message | the sender / the time | the whole text (at body size), Reply, Edit (your own, for fifteen minutes), Unsend (your own, destructive, asks first), Copy Text, Report (not on your own messages; Back leads here from the report) |
| Message request | the sender / "Wants to message you" | their first messages (at most five), Accept, Decline, Block (destructive, asks first), a note: they cannot see that you read this, links do not open. It opens on the first message: read before answering |
| Restriction | the restriction's title ("Messages and Friend Requests Paused", "New Account Limits", "Account Banned") / "Until Oct 25, 2026 at 9:14 PM" or "Until further notice" | What This Means (one note), Reason (the rule, then "Note From the Moderator" and the moderator's words as plain text), Appeal. A mute with an appeal still open for filing: a note with the last day, the field "Why do you think this is a mistake?" (wide) and Send Appeal; an appeal that was sent, decided or out of time is one note instead. A ban: Appeal in Browser and its note. A new account's limit has no reason and no appeal. The page ends by itself when the restriction does (back to where the player came from) |
| Add a Friend, Report, Account | | the name field and Send Request (while a restriction stops friend requests, one row "Friend Requests Paused" with the end in its detail stands in their place and opens the Restriction page); the reason choice and Send Report; Manage Account (opens the account web: e-mail, passkeys, devices, deleting the account), cloud, Hide Your IP Address (toggle) with its note, Who Sees Your Trophies (Friends, Everyone, Only You) with its note, the blocked, Sign Out. A **Restrictions** group opens Home and Account whenever one is in force: a row for each, with its end as the detail |
| Invitation picker | "Invite to …" / "Pick up to … friends" | one toggle per online friend, with their avatar, Send Invitations, Cancel |
| Feed | "Feed" / "Recent activity from you and your friends" | cards, newest first (below); or one note while it loads, is empty, offline or failed (Try Again; Add a Friend for a player without friends) |

A text row can be **wide** (`Row::wide`): its field takes the whole row, written from the left, outlined (1 dp, 22 % of the row's ink) so it does not pass for one more row, and the title is its hint while it is empty (62 % of the row's ink). Enter ends the editing and runs the row (`Change::Submit`): a message goes out, a friend request is sent. A conversation keeps its draft per conversation in the core, so a refused message, a page change or a closed panel loses nothing.

Rows keep their identity by their `RowId` and their place among rows of the same id, not by their place in the list, and a conversation or a request keeps the focus on the same row when others come in above it (`Anchor`: a message by its number, any other row by its distance from the end). A message that arrives while the player types leaves the caret in the field, and the first fetch of a conversation does not move the focus off it.

Hints that explain a text field or a toggle stand under it with the normal 8 dp row gap, not attached: the focused row grows 3 dp and would touch an attached note. A command waiting for the net service says "Sending…" in its detail line.

**Confirmations** (`screens::online::confirm`, `Page::Confirm`). What cannot be taken back asks first: Block (a friend's page, a request, a conversation's options, the answer bar of a message request), Remove Friend, Unsend, Leave Party, Remove from Party and Leave Conversation. The question is the card of 5.9.1 over the section it was asked in (the rail and the header stay, dimmed): the headline asks ("Block Lumen?"), under it one sentence says what it ends and whether it can be undone, then the confirming button in the danger ink and `Cancel`. The card opens with the focus on `Cancel`; Left and Right walk the two buttons; Back leads to the page it was asked on like Cancel, with the focus on the row it was asked from. Confirming does the action and goes to the page that shows its result (Home, Messages, the conversation, the party). A question whose subject is gone meanwhile (the friend removed you, the message was unsent, the party ended) leads back by itself. Nothing in the launcher asks about what can be redone: ending a call, Decline, Mute, Pin. English and Turkish in `online_confirm_strings.rs`.

**Profile screen** (`screens::profile`, its host `app::panels::profile`, the return path `app::profile`; network `spec/cloud-api.md` Profiles, ADR 0024). A full-screen overlay (`Overlay::Profile`) for a player's profile, the player's own or another's, on the frame of the trophy list: glass, 90 dp of side padding, the hint bar at the top right. It replaced the profile page of the side panel, whose banner is gone. `profile` answers a `ProfilePage`: the parts the player may see (`about`, `favourites`, `avatar`, `playtime`), the trophy counts (null when hidden) and the linked accounts. What is shown is decided in `screens::profile::view`, a function from what the launcher knows to plain data (`ProfileView`), and tested without a window; the painter decides nothing.
  - *Entry and Back.* The profile button of the top bar (own, once signed in), Your Profile in the Friends screen's rail, View Profile on a friend's page, and a card of the feed (the account of the card). Back leaves for where the player came from (`app::profile::Back`): Home for the button; for the others the Friends screen opens again on the page it was left on, with the focus on the row it was on. Signed out, the button keeps its old meaning and opens sign-in. On the player's own profile the buttons under the name are Edit Profile (opens the account web, as before; the one prominent button), Friends and, while the cloud answers, **Appear Offline** (Appear Online while it is on; the same button both ways, `Press::AppearOffline`) (the Friends screen on its first page). On a friend's: Friend Options (their page in the panel: Join Game, Message, Invite, Remove, Block, Report). On someone who asked: Answer Request. On a stranger: Add Friend. Entering through a profile never closes the way back to anything the old page offered. **Back button** (2026-10-26): as on the Friends screen, at the top left; the page starts 60 dp under the top and scrolls out under that row.
  - *Header.* One column, no banner: the avatar (120 dp; the first letter of the handle on one of six calm colours without a picture), the handle as the title (40 light), under it a line with a chip for how you know each other ("Friend", "Request sent", "Wants to be friends"; nothing for a stranger or for yourself) and the status (their presence: Online, Playing a game, Offline, with a dot in the accent while they are online; for the own profile "This is what other players see", and "Appearing offline · This is what other players see" while appearing offline, without a dot), then the text about them (17, white 86 %, at most three lines, 640 dp wide; the status line above it is the small one: 15 medium, 70 %), then the buttons. On a friend's profile a quiet line under the text says what only a friend's page can: "You have 3 of their games" (their titles that are in the player's library; left out at none). "Playing Neon Orchard" says what they play and nothing about joining: the profile has no Join button (that is on their page in the panel, one press away through Friend Options). The only colour that is the player's own is a soft wash behind the top-left of the page: the avatar's colour (`banner_colour`, or the monogram's colour) at 22 % in the corner, to nothing 520 dp down and 70 % across; text keeps its contrast with the glass because the wash is never stronger than that. Behind the 120 dp avatar and the handle it is the one thing on the page that could only be this player's.
  - *Figures.* On the right of the header, plain, no boxes: games, hours (minutes below one hour), trophies, each a 34 light tabular figure over a small tracked label, the trophies with their grades as four coloured trophy glyphs and counts under the label (platinum first, a grade without trophies left out). After the trophies comes **Level**: the level (34 light) over its label, under it the points ("1,240 points", 14 medium tabular) and a 96 dp or text-wide track with the way to the next level in the accent. Level, points and the track are the cloud's (`ProfilePage.level`, network ADR 0029); the launcher only draws the fraction, and without the cloud's answer (hidden trophies, loading, offline) there is no Level. A figure whose part is hidden is left out and the others close ranks; with only one figure left there is no strip at all (one number alone only repeats the caption of its section).
  - *Sections.* Games, Recent Trophies and Recent Activity, in at most two columns, and a section with nothing to show for another player is not there at all, so no one sees that a part was hidden: three sections put the games on the left (45 % of the width) and the other two stacked on the right; two sections one column each; one stands on the left. The own profile keeps all three and says in one sentence what fills an empty one ("Earn a trophy in a game and it shows up here."). A section has a 20 semibold caption (with the count of games in 14 beside it), rows 8 dp apart and 40 dp between sections. Nothing sits in a box at rest: a row is text on the page; the pointer makes it a faint surface (white 7 %), the focus makes it the white card of every list (3 dp grow, dark ink, shadow).
  - *Compare.* A game row of another player's profile whose trophies the player may see and that is in the player's library says "Compare trophies" in its detail and opens the trophy compare screen (it replaces "open on Home" for these rows; the player's own games and rows without trophies keep it).
  - *Games.* The own profile lists the library from this PC, the last played first, then the most played: the art (52 dp), the name, "42 h · Last played 2 h ago" (or "Not played yet"), and at the end the trophy progress as "9/40" with a 4 dp bar in the accent under it (counted from the trophy file once read). Another player's: their titles from `playtime` and `favourites`, favourites first in their order, then by time (at most eight; the figure counts them all): the name and a detail ("Favourite", "Not in your library") and at the end their time. **A game's name and art come only from the player's own copy**: a title the player does not have is a neutral tile with its title ID as the name and no press; one they have opens on Home with that game selected.
  - *Trophies.* The own profile: the five newest earned trophies from the trophy files (first set only, as the network records), by name, the game and when. Another player's: the trophy items of their feed, a trophy named from the player's own copy of the game ("First Light") or, without one, "Gold trophy", the game's name or "In a game", when, and at the end the rarity ("Rare, 2.3%") when the cloud has one. A grade is a 44 dp tile in the grade's colour (glyph and a 14 % wash) and never colour alone: the words carry it too ("Gold trophy" for an unnamed one; "Gold · Ember Drift · 40 min ago" under a named one). The rarity is the last part of that line ("… · Rare, 2.3%"), not a fact hanging at the far end of the row. Select on another player's trophy opens the feed on that item, where the reactions are; on the player's own trophy it opens the game on Home.
  - *Activity.* Their feed items without the trophies (those have their own section): avatar (36 dp), the sentence with the handle ("Lumen earned a gold trophy in …", "You and Sol are friends now"), the time and the reactions that were given ("GG 3 · Same 1"; read only). Select opens the feed on that item.
  - *States.* Loading ("Loading profile…" over a spinner on a quiet disc; static under reduced motion), offline ("You are offline"), failed (Try Again, the one focus stop), private (a stranger who shares nothing: "This profile is private"), and a friend with nothing yet ("Nothing to show yet"), each one block of words in the middle of what the header leaves; the own profile never waits for the cloud, because games and trophies are on this PC: while offline a note under the header says that activity comes back.
  - *Focus.* One treatment: the console buttons' and the rows' (white card, 3 dp grow). Only a row that does something is a stop (a game the player does not have is text, not a dead white card). Three kinds of place: the buttons under the name (left and right between them), the left column and the right column (up and down within; left and right cross to the row at the nearest height; up from the top of a column reaches the buttons, the first one from the left column and the last from the right). `screens::profile::nav::step` is pure and tested. The page is one scrolling surface, header included (the wheel freely; the pad by following the focus, centred, with the focus spring), so a small window gives the rows the whole height once the header has gone. What scrolls past the top edge under the hint bar or the bottom of the screen fades out over its last 20 dp as the other lists do; the header fades as a whole once its lower half has passed the edge. The page opens on the first button, or on the first row without buttons. Back and Options leave. Hover and press come from the shared components; reduced motion is the animator's.
  - *Avatars.* `app::avatars` fetches `/v1/avatars/{hash}` of the cloud in use on a worker thread, checks that the bytes hash to the name, keeps the file under `Network\avatars` and hands it to the texture service like a game icon; a failed fetch is tried again after a minute.
  - *Trophy names.* Never from the cloud. A sentence names a trophy only when the player has the game, its trophy file is read (`Effect::LoadTrophies`, off the UI thread, once per game) and the trophy is not hidden, or hidden and earned by the player.
  - *Words.* Sentence-style notes, title-case buttons and captions, no exclamation marks. English and Turkish in `profile_strings.rs` and `online_feed_strings.rs`.

**Trophy compare** (`screens::compare`, view `screens::compare::view`, host `app::panels::compare`, state `app::compare`; network `spec/cloud-api.md` Comparing, ADR 0029; control command `compare`). A full-screen overlay (`Overlay::Compare`) on the frame of the trophy list: the game's art and name (36 light) with "Compare Trophies" over it and "With Lumen" under it; on the right two heads over two 236 dp columns, each with avatar and name ("You" or the handle), "Level 9 · 915 points" with the track to the next level, the game's trophies by grade, and "130 points in this game"; then one row per trophy (picture 52 dp, name, "Gold · what it asks for") with, in each column, a check and the day it was earned or "Not yet". Everything numeric is the cloud's. **Names, texts and pictures come only from the player's own trophy file**; for a game that is not in the library the rows read "Trophy 4", the grade, and one quiet line says why. A hidden trophy the player has not earned is a secret in both columns ("Hidden Trophy", a lock, the Show pill on the focused row; select or Show reveals it and sends `reveal_trophies`, as on the trophy list); "Always Show Hidden Trophies" shows it. The other column is what that player's profile shows the player, so their secrets are not in it. States, one block of words under the header: loading, offline ("You are offline"), failed (Try Again, the one focus stop), hidden ("Their trophies are hidden"), empty ("Nothing to compare yet"). Up and down walk the rows; a row at the top or bottom edge of the list fades out as the edge reaches it (opaque while it rests inside the margin, gone when the cut reaches its text), so no line is cut half way under the heads. Back and Options return to the profile on the row it was left from (the cue is "back", 7.6). **Back button** (2026-10-26): the console button of the Friends screen (small, caret, "Back") at the top left of the 90 dp frame, the hint bar level with it at the right; the header starts 60 dp under the top (`screens::back_row`, `BACK_ROW`), so the mouse can leave without the keyboard. English and Turkish in `compare_strings.rs`.

**Appear Offline** (network ADR 0028, control command `set_appear_offline`, `account.appear_offline` of the `state` event). `Online::set_appear_offline` shows the choice at once, a refusal takes it back with a toast, and the `state` event settles it (also when another device changes it). Where it shows: the switch "Appear Offline" (detail "Friends see you as offline") on the Account page of the online panel; the button on the own profile; "Appearing offline" in the panel's subtitle, the profile's status line and, in quiet ink (white 72 %), after the name in the top bar's profile pill, whose accent dot becomes a ring while it is on (its accessible name says "Appearing offline" instead of "Online"). Offline from the cloud the player is simply offline: nothing is shown.

**Secret trophies.** The launcher tells the cloud which hidden trophies of a game the player has not earned (`secret_trophies`, once per title and sign-in, whenever the trophy file is read). An earned trophy is never secret: the hidden ones the player has earned are revealed (`reveal_trophies`) as soon as the trophy file shows them earned, as consoles do. The player reveals an unearned one by showing it on the trophy list or the compare screen.

Gallery scenes (1600 x 900 and 1920 x 1200): `profile-own-appearing`, `profile-friend-compare`, `online-account-appearing`, `home-appearing` (the real home with the pill), `compare`, `compare-secret`, `compare-revealed`, `compare-numbers`, `compare-loading`, `compare-offline`, `compare-error`, `compare-hidden`, `compare-empty`, and `profile-own`, `profile-own-focus`, `profile-own-end` (scrolled to the last row, the header faded), `profile-friend`, `profile-friend-hidden` (trophies and playtime hidden), `profile-request`, `profile-new` (a new account's own profile), `profile-new-friend`, `profile-private`, `profile-loading`, `profile-error`, `profile-offline`.

**Feed page.** `feed` answers a page of items; `react` sets or takes back the player's one reaction. The page asks when it opens (and for the player's own profile, for the avatar), again for each `feed` event, and for older items near the end of the list.
  - *Card.* A card is a row's surface: 40 dp avatar, the handle ("You" for the player's own items; 17 medium) with the time at the trailing edge (13), one sentence (14, at most three lines) and, for a trophy with a rarity, a line with the grade's trophy icon in its colour and "Rare, 2.3%"; under them the four reactions Nice, GG, Wow and Same as 36 dp pills 12 dp apart, all of one width so the columns line up from card to card, each with its count when it is above zero (99+ above ninety-nine). The player's own reaction is stronger (22 % of the ink instead of 9 %) and carries a check, so it never rests on colour alone. The sentences: "earned the gold trophy “First Light” in Ember Drift", "earned a gold trophy in Ember Drift" (trophy not readable here), "earned a gold trophy in a game" (game not owned), "became friends with Nox", "played Paper Moon for the first time" or "played a game for the first time", "started a party with Nox and Kit". Rarity steps: under 1 % Extremely rare, under 5 % Rare, under 20 % Uncommon, else Common; none below ten players. Times: Just now, 5 min ago, 3 h ago, Yesterday, 3 d ago, then the day (Oct 12).
  - *Focus.* A card is one stop; left and right walk into its reactions (and back), up and down keep the reaction, so a run of GGs is down, select, down, select. A focused card turns white like a focused row (3 dp grow); a focused reaction is solid ink inside it. Select on a card opens the profile screen of its account (Back returns to the feed, where it was), on a reaction sets it, and on the player's own reaction takes it back. Back leaves the page. Near the last three cards the next page is fetched; "Show Older Activity" is the same by hand. Moving the focus plays the Move cue and a press the Select cue; nothing else moves.
  - *Reactions.* Applied at once and settled by the cloud's answer (`Online::react`): a refusal puts the counts back and toasts "The reaction did not go through. Try again." A page that lands meanwhile (a `feed` event, a refresh, an older page) never overwrites a reaction still on its way: the item keeps what the player made until the cloud answers or refuses, and only a refusal rolls it back. A second press on an item whose reaction is still on its way is ignored, so a rollback never meets a newer press.
  - *Paging.* `FeedState` merges: a refresh puts new items on top and keeps the older pages, drops items the cloud no longer lists, and replaces everything when more than a page of news arrived (the held items would leave a gap). The focus stays on its item when items come in above it.
  - *States.* One block in the middle of the room, an icon on a disc with a headline (17 semibold) and a note: loading ("Loading…"), offline ("You are offline"), failed ("The feed did not load" with Try Again under it), empty ("Nothing here yet" and what shows up here; Add a Friend for a player who has none). After a refusal the next page is not asked for by itself any more: "Show Older Activity" says "Could not load. Select to try again." and the player asks again.
  - *Anchor.* The focus stays on its card when items come in above it, except on the newest card: a player resting there sees what arrives.
  - *Not in the launcher.* Who sees the player's feed (`Privacy.feed`) is set on the web; the launcher keeps the value it reads and sends it back unchanged. Saving the trophy visibility sends the whole `Privacy` with only `trophies` changed, so `feed` and every part the page does not show go back as the cloud last said them (a test sets all six parts apart and checks the command).

**Icons** (Phosphor, added to `tools/gen-icons.mjs`): `users`, `user-plus`, `user-minus`, `envelope-simple`, `paper-plane-right`, `cloud`, `sign-in`, `sign-out`, `prohibit`, `flag`, `key`, and for voice `microphone`, `microphone-slash`, `speaker-slash`, `headphones` (with `speaker-high` that was there), `steam-logo`. A friend's row shows a game controller while they play, the user circle while online and a dashed circle while offline; the words say the same, so the icon is never the only cue.

**Rules.**

- Nothing goes online before the player signs in. The net service starts when the Online area opens (it stays local until the player chooses Sign In or Create Account) or, once the player signed in on this PC (`Preferences::online`), with the launcher. Signing out stops it.
- **Accounts live on the web** (network ADR 0016). Sign In and Create Account send `browser_sign_in`; the net service answers the cloud's page and waits on a loopback address for the browser to finish, and the launcher opens the page in the system browser. Manage Account sends `account_page` and opens the link it answers. The launcher opens only addresses under the cloud in use. A sign-in that ends in the browser turns into the signed-in state; one that runs out of time toasts "The sign-in timed out. Try again." Nothing is typed in the launcher: no e-mail, code, name or age.
- The cloud is AnyPlay's own unless the player picks another (`Preferences::online_cloud`). A build names AnyPlay's with `ANYPLAY_DEFAULT_CLOUD`; without one the player has to enter a cloud. Only https addresses, and http on loopback for a cloud under test, are accepted.
- Signed in, Play first asks the net service for the bridge variables and holds the start (the splash already shows) until they come, at most 3 s; without an answer the game starts offline. A join is checked first (`check_join`: the game is in the library, unlocked, same version and AnyPS5 build) and called off with a toast when it fails, since a join makes no sense offline. The game's end is reported once.
- A game that opens its invitation dialog brings the launcher forward on the picker; closing the panel answers "cancelled".
- **Restrictions** (network ADR 0015, `spec/control-protocol.md`). `anyps5_core::online::Restrictions` is the pure state; the panel only reads it. The player is told what is limited, until when in their own time zone, why, and how to appeal, never just "that did not work".
  - *Sources.* The `restrictions` event lists the mutes in force (after going online and whenever a moderator acts or an appeal is decided); a refused command carries the `restriction` that stopped it, which also teaches a new account's limit (it belongs to the action that met it: friend requests, or writing to a stranger); the `banned` event carries a ban. A restriction with an end that has passed is dropped by the launcher at that moment, without an event. Of several of one kind the one that lasts longest shows. A net service that predates the field sends none: the toast says "This account cannot do that right now."
  - *Where it shows.* (1) Where the action would be: the Add a Friend page and a conversation swap the field and the Send row for one row, "Friend Requests Paused" or "Messages Paused", with the end in its detail; it opens the Restriction page, and the draft waits. A new account's limit replaces the field only in a conversation with someone who is no friend. (2) The Restrictions group on Home and Account. (3) The Restriction page. (4) A toast for a refused action ("Messages and friend requests are paused until Oct 25, 2026 at 9:14 PM.", or the new-account sentences for a friend request and for a stranger), for a mute that began while the launcher ran, and for a ban.
  - *Ban.* The account is signed out, so the ban stays on the Start page as the first row ("Account Banned", the end as detail) until the next sign-in. A ban is appealed on the account web: Appeal in Browser sends `browser_sign_in`, and the cloud's sign-in ends on the page with the statement and the form. A mute is appealed in the panel with the `appeal` command; the answer arrives as a new `restrictions` event.
  - *Words.* Calm and without blame: what is paused, until when, what still works, what to do. English and Turkish in `screens::online_restriction_strings`. The moderator's words are untrusted text and shown as plain text. Times use the system's time zone (`format::date_text`).
- Toasts: signed in, signed out, request sent, now friends, a request or an invitation arrived, blocked, report sent, account deleted, joining; failures say what to do (code wrong or expired, name taken, no such account, too many tries, cloud unreachable, game full, version mismatch naming both sides).
- The net service is found through `ANYPLAY_NET`, next to the launcher, or in the launcher itself: a release build embeds it (`ANYPLAY_NET_EMBED` at build time) and writes it once per version to `Network\bin`, removing older copies. Its log goes to `Logs\network.log`.
- *Record Network Calls* (Settings, Advanced, off by default) adds `ANYPS5_NET_OBSERVE=Logs\<TITLEID>-network.jsonl` to every game start, signed in or not.
- *Hide Your IP Address* (Account page, off by default; `Preferences::online_relay_only`) keeps every tunnel on a relay, so other players never learn the PC's address. The net service starts with `--relay-only`; a change while it runs goes out as `relay_only`, which starts its tunnels again, so during a game it waits for the game's end.

- **Parties and messages** (network `docs/SOCIAL.md` S2, `spec/control-protocol.md`, ADR 0013). `anyps5_core::online::Social` is the pure state; the panel only reads it.
  - *Join codes.* `join_code`, `join_code_revoke` and `join_redeem` are the three commands of the Discord join feature. The first two keep the Join button of the player's Discord status (7.4); `Online::join` holds the code and never prints it, and a sign-out or the end of the service forgets it. `join_redeem` answers the invitation the usual `invitation` or `party_invitation` event brings too; the launcher opens the panel and joins nothing (9.6).
  - *Party.* Kept by `revision`: an older copy of the same party is dropped. `party_answer` with a refusal changes nothing. The invitations of a party list with a game's on Home and toast when they arrive, whatever the panel shows. Taking the party into a game sends `party_move` with the session the player is in; the members get ordinary invitations, so a game's session has to exist first, which the page checks (not closed, the player in it, somebody to take).
  - *Messages.* The launcher keeps the highest number it holds for each conversation. A `message` event raises the conversation's last number at once (the unread mark follows) and fetches `messages` `after` that number; the first look at a conversation fetches the newest 50, and a conversation that is behind its last number follows by 100 until it has caught up. After a reconnect the `conversations` event is compared with each loaded thread and what is missing is fetched; threads never opened need no fetch, since unread is `last - read`. A short page ends a catch-up, and numbers a block hid do not loop. 200 messages stay per conversation; "Show Earlier Messages" shows 40 more and reads back with `before` until the memory is full. An own message joins the thread when it is the next number, else the answer is a fetch.
  - *Read.* Looking at a conversation page marks its newest held number read, in the launcher at once and in the cloud with `mark_read`. Writing reads up to the message written. A message request is not marked. The cloud counts what is unread (`Conversation.unread`: others' messages above the read mark that the player can see and that were not unsent), so every device shows the same number; between two lists the launcher adds one for each new number and sets it to zero when it reads. The Messages row, the Messages page and the button show one number: the unread messages of conversations that are not muted, plus one for each message request.
  - *The list.* The page is the title "Messages" and its list (the rail already counts what is unread), drawn in `screens/online/chat.rs`. One row per conversation, 72 dp high, with no box at rest (a surface appears under the focus and the pointer): the picture (48 dp; two overlapping ones for a group; a request shows the sender's picture like any other), the name (17 medium, semibold while unread), the newest message in one line under it (15; "You: ...", "Juno: ..." in a group, "Lumen is typing…" while someone types; "No messages yet" in note ink when there is none; the ink steps up to label ink while unread), the time at the trailing end of the first line (13, note ink, label ink while unread) with the pin or the muted bell before it on the same line, and the unread count as one light badge at the end of the second line (none for a muted conversation). A request carries a small "Request" tag after its name instead of a count. Section labels ("Message Requests", "Conversations") are the caption of every list of the area and stand on the avatar's edge. Pinned conversations come first, then the newest; message requests are a section above. The previews are fetched one message at a time for the first conversations in view. Up and down walk the list, select opens, Left goes to the rail.
  - *The thread.* A conversation is its own full-screen view, without the rail, on one column 940 dp wide: the header (picture 48, name 22 semibold, presence 14, and the options button at the trailing end as a 44 dp control with a surface at rest), the bubbles between, the composer pinned at the bottom, all on the column's two edges. The player's bubbles are one step lighter than the others' (`bubble_own` 0.16 against `bubble_other` 0.10, both with `text` ink, never a white fill), on the trailing side; bubble text is 17 regular at leading 1.45, radius 16 with the corner on the sender's side 7. Messages of one sender within five minutes form a group, an answer among them (its quote is inside its own bubble): 4 dp between bubbles of a group, 16 dp between groups, and the thread starts 16 dp under the header (the first day label adds its own 24). In a group chat another's group carries the name once above its first bubble (13 semibold) and the picture once beside it (32 dp, aligned with that bubble); the time stands once under the last bubble of a group (13, note ink) with Edited, Delivered or Seen after it (Seen by N in a group), while the screen reader reads every bubble as who, words, time and state. A label names each day (Today, Yesterday, the date) before its first message, centred, 13 semibold in note ink with no box. A reply shows a thin bar, the sender in label ink and one or two lines of the quoted text inside its bubble. A message taken back is a system row: centred "Message removed" in 14 note ink, no bubble, 16 dp above it, still a stop of the focus. Somebody typing is three quiet dots in the bubble column (their words are for the screen reader and the list). A short thread sits at the bottom of the screen; a long one rests at its end and stays there while messages come in, and walking down to the composer takes it back to the end. Up and down walk the bubbles (select opens that message's actions), Back returns to the list with the conversation still focused, Options opens the conversation's options. No reactions: a message stays plain.
  - *Writing.* One calm field, a capsule 44 dp high on the column's width in the composer surface with the control hairline (the focus ring only for keys and the pad), the text and the placeholder at body size (17), starting on the edge the bubbles' text starts on, and Send in its trailing end, quiet until there is text and then the one primary of the view; Enter sends. A pad has no keys: select on the field opens the on-screen keyboard (rows of keys that a pad walks, Shift for one capital, 123 for numbers and symbols, Space, Delete, Send), which also offers six quick replies. A draft that answers or edits shows a line above the field ("Replying to …", "Editing your message") that cancels when pressed. Typing is told to the others while the field has text. Where a restriction pauses writing the field is replaced by one bar that says so; where the player blocked the other it says that and offers Unblock; a message request has Accept (primary), Decline and, alone at the other end of the bar, Block (destructive, asks first) instead of the field.
  - *Actions.* A message's page offers Reply, Edit (the player's own, 15 minutes), Unsend (the player's own, any time; removes the text for everyone), Copy Text and Report (others'). A conversation's options page offers Mute (no toast, no badge), Pin to Top (five at most), Report and Block (direct chats) and Leave (groups). A report from a message names the message.
  - *States.* No conversations: the list says how to start one. Loading: a "Loading…" row. A list or a thread that could not be loaded says so with a Try Again row. A message request opens read-only with Accept, Decline and Block. A blocked chat replaces the field with a note and Unblock.
  - *Toasts.* A new message (the newest unread one a `message` event brought, the player's own never), a message request that arrives after the first list, and a party invitation. The two message toasts only show while the online panel is closed; a recovery after a reconnect toasts nothing. The text of a toast is the first 60 characters on one line.
  - *Text.* Message text is untrusted and shown as plain text: control characters other than line breaks are dropped, and so are the characters that reorder text (U+061C, U+200E/F, U+202A to U+202E, U+2066 to U+2069). Nothing is a link: the launcher never opens an address from a message, in a request or out of it. A message can be copied from its page.
  - *Limits.* A message has 1 to 2,000 characters and is not only white space; the field keeps at most 2,001 so the Send row can say so. New accounts may not write to strangers for 24 hours, and a muted account writes to nobody (*Restrictions*, below). The launcher opens conversations with friends only, and a stranger's request can be answered.
  - *Typing* needs a keyboard (the field takes the system's text input); a pad reaches every other row.

- **Trophies on the network** (network `docs/SOCIAL.md` S1, ADR 0009). Signed in, the launcher records what the player earned with the cloud: number, grade and time of each trophy of a title's first set (service label 0), never its name, text or picture. It sends a title's trophies whenever its set loads (selecting the game, the window coming back, the end of a game, each unlock while it runs) and, once each time the player comes online, reads every unlocked copy one after the other, 250 ms apart, so earlier trophies arrive too (`Trophies::earned_records`, `CoreEvent::TrophyRecords`). A title goes up again only when its count grew; the cloud keeps the first time of each trophy, so a repeat changes nothing. Failures are only logged.
- **Playtime on the network.** Besides the total per game, the launcher keeps the seconds of each UTC day for the last 60 days (`Preferences::playtime_days`). Signed in, it sends minutes per title and day, the copies of a title added up, when it comes online and whenever a minute more was counted (at most once a minute while a game runs). A day goes up again only when its minutes grew; the cloud keeps the highest count of a day. Time played before this record existed is not sent, because its days are unknown.
- **Gamer cards.** The friends' cards (trophy counts by grade, or hidden) come with the friend list, for new friends as they appear, and again for everyone when the window comes back, at most once a minute. *Who Sees Your Trophies* (Account page) is read from the cloud at sign-in and sent at once when changed; a refused change toasts and reads the setting back.

**Party voice** (network `spec/control-protocol.md`, `spec/voice.md`, ADR 0014 and 0017). The net service opens the microphone and speakers, mixes the members and reports who speaks; the launcher chooses and shows. `anyps5_core::online::Voice` is the state (the `voice` event, the device lists, why the service gave up), `VoicePrefs` (in `Preferences::voice`) what the player chose, and the methods of `Online` (`set_voice`, `set_voice_mode`, `set_talking`, `set_peer_volume`, `set_peer_mute`, `set_voice_device`) the commands.
  - *Where.* Voice is not a settings category: the devices come from the net service, which runs only for a signed-in player, so the controls live in the online panel. The party page has the switch, the microphone and the mode; Voice Settings and the Home row lead to the voice page with the talk key and the devices. Volume and mute are on the member page, which every member can open (it was the leader's alone).
  - *Words first.* A member who speaks gets the detail "Speaking" and the speaker icon in place of the presence icon; a member the player muted gets "Muted" and the slashed speaker; "Not in voice" says the member has voice off. The microphone row says "Off", "Muted · hold V to talk", "Open", "Listening" or "Talking". There is no level meter and nothing moves: a speaking state changes at most ten times a second and a row that jumps in the corner of the eye distracts during play. The icon only repeats the word.
  - *Push to talk and the game window.* The launcher reads no keys outside its own window: nothing in it is a global hook, and a hook that sees every key of every program is not something a launcher should install for a convenience. So push to talk works while the launcher has the focus, and says so in a note under the mode (Mode, Push to Talk: "Push to talk works while the launcher is in front."). While a game runs the note changes to the honest advice: "A game has the keyboard, so the key does not reach the launcher. Switch to Voice Activity to talk while you play." Nothing switches the mode by itself. The key counts as up when the launcher loses the focus, when a text field has the keyboard, when a dialog is open, while the key row waits for a key, and with Ctrl, Alt or the Windows key held: a key can never stay stuck open. Only changes are sent (`push_to_talk`), and the net service ignores the key in voice-activity mode.
  - *The key.* One key, default V, a keys row like the controls panel's (the same capture, Esc keeps the old key, a pad's back too). Keys the launcher itself reads (arrows, Enter, Space, Esc, Backspace, Tab, O, comma, F11) and modifiers cannot be chosen: held, they would act on the screen or are not tracked as keys.
  - *Devices.* System Default first, then the names the net service lists (`voice_devices`, asked after `hello`, when the voice page opens and with Refresh Devices). A chosen device that is not listed stays visible as "not connected right now" instead of vanishing. Choosing sends `set_voice_device`, which restarts voice for a moment. A refusal (`not_found`) toasts only while the online panel is open.
  - *States and errors.* Off, "Starts when you are in a party" (switched on, no party), "Connecting…", "On". The net service switches voice off by itself when the devices cannot be opened and says so in a `voice_device` notice; the toggle goes off, a toast says "Voice could not start. Check your microphone and speakers." (or "The system did not allow the microphone." when the reason reads like a refusal) and a note under the toggle says what to do. The reason itself goes to the log. No microphone and no sound at all are told apart by the device lists: "No microphone found…" and "The net service reports no sound devices…" (a net service built without sound devices lists none).
  - *Kept.* On or off, mode, key, devices and each member's volume and mute (only values that differ from 100 % and unmuted, at most 256) are in the preferences and sent again after every `hello`, since a new net service starts with voice off. The mode counts as chosen (`VoicePrefs::mode_chosen`) once the player picked one, Push to Talk included; until then it is only the default. Preferences from before this was tracked count a non-default mode as chosen. The event's `enabled` and `mode` are not copied back: one sent before the service heard a choice would undo it.

**Calls** (network ADR 0026, `spec/control-protocol.md`). A call is a party that rings: the cloud's party invitation with a `call` flag that lives 30 seconds. `anyps5_core::online::Calls` is the pure state (who the player is calling, whether the party is a call, the missed calls), `screens::calls` the two surfaces, `screens::online_call_strings` the words (English and Turkish).
  - *Starting.* Call is a row on a friend's page, after the message rows and before the trophies, so a press on the friend does not ring them by accident. It is offered for a friend who is online, to a player who is in no party or leads one (then it reads Add to Call); it turns voice on (not on Discord, below) and sends `party_invite` with `call`. The strip says "Calling Lumen" with Cancel. An empty `sent` toasts "Lumen cannot take a call right now."; a call that rang out or was declined toasts "Lumen did not answer." or "Lumen declined the call." and the launcher leaves its party of one.
  - *Ringing.* A call invitation does not toast. The window comes forward and a card hangs at the top centre, below the toast: the caller's avatar (the monogram colour and initial the friends list and a thread give the same player), the handle at button size, "is calling you" in a note, and two Medium buttons 12 dp apart, Accept (the prominent one) first and Decline after it, like every dialog. The card comes in and goes out with the overlay spring (fade and 24 dp of slide); nothing on it moves while it stays: no pulse, no countdown ring. A soft ring (Cue::Ring, two stacked fifths at F4 and C5, low and warm) plays at once and every 3.2 s while the card stays, through the sound settings like every cue. The card is for the pointer and for Tab (each button shows the focus ring); the pad and the keyboard answer from the Invitations row of the Online panel. A call that is not answered in 30 s leaves the card and becomes a missed call.
  - *In a call.* The same place holds a strip while the player is on a call: "In call" with the others' handles under it, a handle followed by "· speaking" and set in brighter ink while the member speaks, "You are muted" or "You are deafened" after the title; Mute, Deafen and, a little apart at the end (24 dp instead of 12), Leave Call as Medium icon buttons in that reading and Tab order (the microphone and the headphones turn to their slashed forms when on, the sign-out glyph is destructive; each has its name for assistive technology). Deafen silences the microphone as well, so Mute is disabled while deafened. Per-person volume is on the member page of the party. There is no level meter and no waveform; who speaks is said in words.
  - *Mode in a call.* Push to Talk is the default, and a call that asks for a key makes the first words hard. A call (calling or connected) therefore talks with Voice Activity while it lasts, unless the player chose Push to Talk themselves (`Voice::mode_in_use`; the setting in the preferences is never touched). The mode goes to the net service when the call starts and the setting comes back when it ends (`Online::sync_voice_mode`, after every line and after Call, Accept and Hang Up); a key held at that moment is let go. Ordinary parties keep the setting. The strip says it honestly: the title reads "In call · Voice Activity" while the call overrides the setting (a mute or deafen takes that place first), and the Mode note on the party and voice pages reads "On a call, the microphone opens whenever you speak. Push to Talk is back when the call ends." The Mode row itself keeps showing the setting.
  - *Missed calls.* A friend's row says "Missed call · 5 min ago" in place of their presence until the player has looked at the friend's page (`calls_seen`) or called them; a toast says "Missed call from Lumen" when one arrives.
  - *Blocks, mutes, new accounts.* The cloud applies the rules of messages: only friends ring, a block in either direction stops it, a muted account cannot call (the refusal's toast is the usual restriction text), calls are rate-limited, and a friend who is in a party is not rung.
  - *Voice on Discord.* Voice Service (AnyPlay or Discord; "Voice On" read like the switch under it) is the first row of the voice rows on the party and voice pages and is kept in the preferences. On Discord, AnyPlay never opens the microphone for a party or a call: the toggle, the microphone, the mode and the key are replaced by "Voice on Discord", a note, and, when the party has a link, Open in Discord. The leader of the party sees a wide field for the Discord link (`discord.com/channels/…` or `discord.gg/…`, checked by the launcher before it is sent and by the cloud), Save Link and Remove Link; everyone in the party sees the link. A call still rings in Discord mode, the card saying "Your voice is set to Discord"; accepting joins the party and the strip says "Voice on Discord" with Answer on Discord when the party has a link; without one the card and strip say plainly that the voice is set to Discord. Only an address that passes `is_discord_link` is ever opened.
  - *Words.* Row titles in title case, details and notes in sentence case, no exclamation marks, no blame. "Add to Call" and "Call" are the only new verbs on a friend's page.

Gallery scenes: `incoming-call`, `incoming-call-discord`, `in-call`, `in-call-open` (the strip while a call overrides Push to Talk), `in-call-discord`, `calling`, `missed-call`, `party-discord`.

Gallery scenes: `friends-signin`, `friends-browser`, `friends`, `friends-friend`, `friends-settings`, `friends-picker`, `friends-party`, `friends-party-picker`, `friends-messages`, `friends-conversation`, `friends-message`, `friends-request`, `friends-party-voice`, `friends-party-member`, `friends-voice`, `friends-voice-key`, `friends-voice-devices`, `friends-voice-denied`, and `home-messages` (the profile button with unread messages), `home-friends`, `home-friends-quiet`, `home-friends-focus` and `home-signin-focus` (the top bar's pills).

**Share screen** (network `spec/control-protocol.md`, "Share screen"). Watch only, like a Go Live: a player shows the game they play to their party and members choose to watch; no input crosses. The share worker, a process the net service starts, captures, encodes, decodes and draws; the launcher neither decodes nor draws video. It chooses and says in words what the `screen` event reports. `anyps5_core::online::Screen` is the state (`can_share`, `can_watch`, the stream the player shares, the offers, the stream they watch), `ScreenCommand` (`Start`, `Stop`, `Watch(account)`, `Leave`) what the player asks for (`Online::screen_send`, the commands `screen_start`, `screen_stop`, `screen_watch`, `screen_leave`), `ScreenNotice` what the player hears of. Code: `anyps5-core/src/online/screen.rs`, `screens/online/screen.rs`, `screens/online_screen_strings.rs`, `app/online_screen.rs`.
  - *The worker and the switch.* A release carries the worker inside the launcher (`ANYPLAY_SHARE_EMBED`, like the net service) and writes it once per version to `Network\bin`; a build without it looks for `anyplay-share` beside itself or at `ANYPLAY_SHARE`. The launcher hands the path to the net service (`screen_worker`) right after `hello` only while share screen is on: `Preferences::online_share_screen` (default off, no control in the interface), or `ANYPLAY_SHARE_SCREEN=1` for one run. Off, the net service has no worker, `can_share` and `can_watch` stay false and nothing shows. On Linux the worker encodes and decodes with Cisco's OpenH264, which the launcher never carries: the Advanced row `openh264` (shown while share screen is on) downloads it from Cisco into `Network/bin` when the player turns it on (`anyps5_core::openh264`, checked by SHA-256) and removes it when they turn it off, with Cisco's notice under the row as Cisco's licence asks. The net service starts with `ANYPLAY_OPENH264` naming that file, and the launcher hands the worker over again whenever the file comes or goes, so the `screen_worker` answer (`libraries`) and the row say the same. The Share Screen row sends `screen_start` with the game's window and processes (`RunHandle::window`, `RunHandle::pids`, through `Effect::ScreenStart`), so the worker captures that window and no other.
  - *Nothing by default.* `can_share` is false until the worker can capture (and it needs a running game and a party), `can_watch` until it can show a stream, so a build without the worker shows no trace of the feature: no caption, no button, no Home line. The group also stays out while `can_watch` is the only thing true and nobody shares.
  - *Where.* One group on the party page, between the Voice group and the Members, under the caption "Screen". Rows like the neighbours', no new chrome: Share Screen (command, detail "Show your game to the party"; only while `can_share` and nothing is shared). While sharing: a sentence as a note ("Your screen is shared with 2 people", "…with 1 person", "Your screen is shared"), the game as a row with its state as the value ("Live", "Starting…", "The picture has stopped"), one row per member the stream reaches with the way to them as the value ("Direct, 24 ms", "Through a relay, 80 ms", "Not watching" for one who only knows) and from 5 % lost the line "Some of the picture is lost" under it, and Stop Sharing. Offers: one row per sharer, "Watch Lumen" with the detail "Sharing Astro Bot"; where `can_watch` is false the note "Lumen is sharing Astro Bot" and under it "This version cannot show it yet." instead of a button. Watching: "Watching Lumen" with the state as its value ("Connecting…", "Live", "Waiting for the picture") and "Astro Bot · Direct, 24 ms" below it (an info row without a value draws as bare text, without icon or a line under the title, so every row here that has a line under its title has a value), a note "The picture opens in its own window.", and Stop Watching. The stream watched is not offered again. One action per state: the group offers Share Screen, Stop Sharing, Watch or Stop Watching, never two competing primaries.
  - *Other pages.* The launcher has no chrome outside the panel for voice or anything like it, so there is none for this either. The Home page shows one row "Screen" (icon, detail "Watching Lumen", "Your screen is shared with 2 people" or "Lumen is sharing Astro Bot"; only in a party and only while there is a stream) that leads to the party page, and a toast says when a member starts sharing ("Lumen is sharing Astro Bot", only while the panel is closed), when a stream the player watched ended by itself ("Lumen stopped sharing.") and when their own ended by itself, for example because the game closed ("Your screen is no longer shared."). Leaving or stopping oneself toasts nothing.
  - *Words.* The path is "Direct, 24 ms", "Through a relay, 80 ms" (without a measurement just "Direct" or "Through a relay", while it is made "Connecting…"). No protocol words.
  - *Errors.* A refused command toasts one plain sentence: `no_game` "Start a game first, then share its screen.", `not_in_party` "Join a party to share your screen.", `busy` "You are already sharing or watching a screen.", `no_encoder` "This PC cannot share its screen.", `not_found` "That screen is not shared any more."; anything else the general failure text.
  - *Not here.* The picture, its size, the codec and the numbers of the stream (frame rate, bit rate) stay with the worker's window. Keyboard and pad reach the buttons like every row; the info rows are read as text.

Gallery scenes: `friends-screen-ready`, `friends-screen-hosting`, `friends-screen-offer`, `friends-screen-cannot`, `friends-screen-watching`, `friends-screen-stalled`, `friends-screen-home`.
Gallery scenes: `friends-muted` (Home with a mute), `friends-restriction` (the mute's page), `friends-paused` (a conversation while muted), `friends-banned` (the Start page after a ban), `friends-signin`, `friends-browser`, `friends`, `friends-friend`, `friends-settings`, `friends-picker`, `friends-party`, `friends-party-picker`, `friends-messages`, `friends-conversation`, `friends-message`, `friends-request`, and `home-messages` (the profile button with unread messages).
Gallery scenes: `friends-activity` (a card focused), `friends-activity-reaction` (a reaction focused), `friends-activity-empty`, `friends-activity-loading` and `friends-activity-error`; `friends-muted` (Home with a mute), `friends-restriction` (the mute's page), `friends-paused` (a conversation while muted), `friends-banned` (the Start page after a ban), `friends-signin`, `friends-browser`, `friends`, `friends-friend`, `friends-settings`, `friends-picker`, `friends-party`, `friends-party-picker`, `friends-messages`, `friends-conversation`, `friends-message`, `friends-request`, and `home-messages` (the profile button with unread messages).
Gallery scenes of the Friends screen itself (1600 x 900): `friends` (the default page, the focus on the first request), `friends-busy` (five requests, friends playing), `friends-empty` (no friends yet), `friends-focus` (a friend's row with its quick actions up), `friends-rail` (the focus on the rail), `friends-offline` (the note and Try Again), `friends-connecting`, `friends-party-none` (the Party section without a party), `friends-signin` and `friends-browser` (no rail), `friends-banned`; every page of the table has a `friends-` scene of its own.

### Play Together (2026-10-25, part of section 17)

The launcher's side of [Play Together](https://github.com/anyplaynet/network/blob/main/spec/play-together.md): the owner's picture is "I start Minecraft from the launcher, my friend presses Join, his launcher starts Minecraft in the same session, and my world shows up as a LAN world". Nobody sets up a VPN, a port or an address. The pure rules are `anyps5_core::play_together` (the table, the join check, the presence wording); the flow is `app::together` and `app::online`; the card is `screens::together`; the words are `screens::together_strings`.

**The table.** `data/lan-titles.v1.json` lists the games that can be played together (first entry: PPSA17221, Minecraft, up to 8 players). A copy is built in; an entry with a mode the launcher does not know is left out. Play Together exists only for a game in the table. Each entry may say how far the host's and the joiner's versions may differ, `"version"`: `exact` (the default when the field is missing, and Minecraft's, because Bedrock clients refuse another protocol version), `major` (the first dot-separated part must match: `01.044.000` joins `01.050.000`, not `02.000.000`) or `any`. The rule is data read by generic code: the join check applies it, and the launcher sends it with `check_join`, `game_start` and the cloud's join, so the net service and the cloud compare two version texts and know no game ([spec](https://github.com/anyplaynet/network/blob/main/spec/play-together.md#the-lan-title-table)).

**Hosting.** On the game's page the action row reads Play, Play Together, trophies, "...". Play stays the one primary action; Play Together is a secondary control of the same size, shown only for an unlocked game in the table (a game that is locked, starting or running shows no second button). The party page has a row of the same name with the game's name as its detail. Pressing it starts the game through the ordinary start with `game_start` carrying `host` (`max_members` from the table, `visibility` friends, `party` when the player is in a party); a plain Play carries none. When the net service cannot open the session, the game runs anyway and a toast says why nobody can join ("Nobody can join: Play Together could not start. Your game runs anyway."). A start that the net service does not answer within three seconds runs alone and says so.

**Joining.** One function, one card. An invitation of kind `lan`, a friend of the party who plays in a Play Together session, a friend's Join row and an accepted invitation in the panel all end in `together::join`: the local checks first (signed in, online, online play on, owned, version, toolkit, place, not busy, in the order of the spec's table), then the net service's `check_join`, then `game_start` with `join`. A game that is owned but not unlocked is unlocked first and starts in the session when the unlock is done; a failed unlock drops the join and says so. An invitation of kind `lan` is not a toast and not an invitation row: it is the card "Mira is playing Minecraft. Join?".

**The card.** A glass card at the top centre, on the call card's place and below it while a call rings (12 dp apart), entering with the overlay spring. Left a 56 dp lead (the friend's initial, or a quiet warning glyph), right a headline (semibold 16, three lines at most) and one line under it (14, 70 % white). Under both, the answers on one row, 12 dp apart: the quiet one on the left, the primary one (ink fill) on the right, one primary per card.

| State | Headline | Line | Answers |
|---|---|---|---|
| Offer | Mira is playing Minecraft. Join? | 2 of 4 places taken (an invitation: Your game starts in their session) | Not now, **Join** |
| Signed out | Sign in to play together | Play Together uses your AnyPlay account. | Close, **Sign in** |
| Offline | You are offline | Play Together needs a connection. Try again in a moment. | **Close** |
| Online play off | Online play is switched off | Switch it on to play together. | Close, **Switch on** |
| Not owned | You need Minecraft to join | It is not in your library. Add it, then join again. | **Close** |
| Wrong version | You need Minecraft 01.044.000; you have 01.008.000 | Update the game, then join again. Trying anyway may not connect or may fall out of sync. (You are ahead of the session: Ask your friend to update, or install Minecraft 01.008.000. and the same second sentence.) | Try anyway, **Close** |
| Wrong toolkit | You need toolkit v0.1.5; you have v0.1.4 | Choose it under Toolkit for This Game, then join again. | **Close** |
| Full | This game is full | Try again when someone leaves. | **Close** |
| Busy | Another game is running | Close it, then join. | **Close** |
| Ended | That session has ended | Ask your friend to start a new one. | **Close** |

**The pad reaches the card** (2026-10-26). The card is a stop of the home screen: `HomeFocus::Card(i)`, one stop per button in the order they are drawn (the quiet answer first, the primary one last). From the library button, Left goes to the card's primary button (before the update notice, if both show), Left again to the quiet one; Right walks back to the notice or the top bar; Down goes to the tiles; Select presses the button (sound `select`, then the focus returns to the tiles); Back keeps its job (a blocker closes, an offer is put away). The ring is the one of 3.11, drawn only for keys and the pad. Tab visits the card first. Nothing moves the focus onto the card by itself, so a card that appears never takes a key press from the game page. Scene `together-wrong-version-pad`.

**Try anyway.** The version blocker is the one blocker with a second way forward. It keeps its message and its one primary action (**Close**, ink fill); "Try anyway" is the quiet answer on the left, the same place and size as the other blockers' quiet answer, 12 dp from Close. The note about the risk is the second sentence of the line, in the same 14 / 70 % white, so it is read before the button is pressed and there is no second dialog for a thing the player can undo by closing the game. Pressing it runs the same join again with the version check off: `check_join` and `game_start` carry `override_version`, the net service and the cloud skip only the version part of their guard (a different toolkit build still blocks, and says so), and the launcher's log records the session, both versions and the rule. A rule of `major` or `any` that already lets the versions through never shows the blocker. The button is the sentence-case "Try anyway" like its neighbours ("Not now", "Sign in"). Like every button of the card it is reached by the pointer, Tab and the pad (the pad's stops of the card, above).

A friend who plays the game with no session (their online play is off, or they pressed Play) has no Join row; their page says "Online play is off on Mira's PC" above the actions. "Not now" remembers the offer by its session and declines the invitation; the card does not come back for that session. When the host's session ends while the player's game runs, a toast says "Mira's session ended. Your game keeps running." and the game is left alone.

**Presence.** A friend in a Play Together session reads "Playing Minecraft together (2/4)" in the friends list, on the friend's page and in the party; the player's own session caption reads the same. A session of any other kind keeps the wording it had.

**Strings.** `together_strings.rs`, English and Turkish (the Turkish is unreviewed). Headlines say what is the case; the line under says what to do. The wrong-version headline is the owner's sentence.

**Gallery.** `home-together` (the button, focused), `together-offer`, `together-offer-invitation`, one scene for each blocker (`together-signed-out`, `-offline`, `-online-off`, `-not-owned`, `-wrong-version`, `-wrong-version-ahead` (both with Try anyway), `-wrong-build`, `-full`, `-busy`, `-ended`, `-no-session`), and the panel pages `together-friends`, `together-friend`, `together-friend-nosession`, `together-party`.

## 18. Phone as controller

Code: `anyps5_core::remote` (server, protocol, virtual controller) and, in `anyps5-launcher`, `app::phone` and `screens::phone`.

The AnyPS5 Remote phone app of the macOS fork ([utkuhalis/AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM), `mobile/`, Android and iOS) works with this launcher: the phone browses the library, starts and closes games, receives trophies, and is a gamepad. The feature is **off by default and LAN-only**. The wire protocol is the fork's (`tools/macos/launcher/Sources/AnyPS5Core/Remote/RemoteProtocol.swift`; the fork is GPL-2.0 like this launcher, and the code here was written against the protocol, on top of the offline start in the branch `feat/phone-controller`). The pad takes a different road than on the Mac: **no engine change**. The phone's pad goes to an operating-system virtual controller (ViGEmBus on Windows, uinput on Linux) and the engine reads it like any controller. The engine-side `RemotePad` of the ARM fork (a loopback bridge the game reads) stays a documented option: the loopback relay and `RemoteServer::bridge_environment()` are kept and tested, and the launcher does not set `ANYPS5_REMOTE_PAD` until an engine build reads it.

**Protocol.** One JSON object per WebSocket text frame on port 47700 (a busy port falls back to a free one and the settings say so), tagged by `type`: `hello` (protocol 1, `deviceID`, `deviceName`, `key`), `pairing`, `welcome` (`session` as a decimal string, `udpPort`, `macName` which the phone shows as the PC's name, `key`), `rejected` (`protocol`, `busy`, `denied`), `latency`, `library`, `wantArt`, `art`, `command`, `select`, `launch`, `close`, `home`, `saveLayout`, `trophy`. Not implemented and ignored on both sides: `activity`, `look`, the stream and broadcast messages and the together handoff (screen streaming and the Mac's social extras; share screen has its own design in the network repository). The pad is UDP on port 47702 (80-byte input packets in, 32-byte output packets out, little-endian, magics `APRI`, `APRO`, `APRH`; golden-hex tests in `remote/protocol.rs`). The phone finds the PC through mDNS `_anyps5-remote._tcp` (no TXT records, the control port in SRV); only the address of the real LAN adapter is announced, never WSL, Hyper-V, Docker or VPN adapters. If discovery fails the settings show `address:port` for the app's manual entry.

**Pairing.** An unknown phone sends `hello`; the launcher answers `pairing` with a random 4-digit code, shows the same code on the PC, and the player compares and answers. Allowed, the phone gets a 32-byte random key (base64) that it sends in later `hello`s and is welcomed without a question. The paired phones are in `RemoteDevices.json` under the config folder (the macOS file's shape; mode 0600 on Linux; keys compared in constant time), at most eight. The layouts the phone saves per game are in `RemoteLayouts.json`.

**Security.** The server starts only when the player turns the setting on. A peer needs a private address **and** has to sit on a network this PC is plugged into (the same subnet as a real adapter; loopback for tools): a routed, VPN or port-forwarded peer is dropped at accept. A browser cannot talk to it (a WebSocket upgrade with an `Origin` header is refused). One phone at a time: a second is told `busy`, and a connected phone is replaced only by proof of its key, so knowing a device id is not enough. Before the welcome only `hello` is heard (twenty other frames close the socket); an unknown phone causes at most six questions a minute and a denied phone waits ten seconds; a question times out after a minute and an unanswered one is replaced by the same phone's retry. After the welcome, requests are limited to 40 a second (burst 80) and a phone that keeps flooding is dropped; pad packets are limited to 2000 a second. A message over 512 KiB ends the connection. Pad packets count only with the session id from the `welcome` and only from the phone's address. Removing a phone in the settings forgets its key and disconnects it at once. Nothing here touches the internet.

**What the phone may do.** `command` (left, right, up, down, select, back, options; `l1` and `r1` page through the games) is the home screen's own input and is ignored while a game runs. `select` picks a game. `launch` does what the primary button does for the game (play, unlock, bring forward when it runs); a different game that runs is closed first. `close` ends the running game or cancels the start. `home` closes the open panel and brings the launcher forward (the game keeps running). `wantArt` is answered on a thread of its own with a 256 px icon and a 1280 px background as small JPEGs in base64. The library message carries id, name, state (`installed`, `update`, `locked`, `installing`, `starting`, `running`, `unavailable`), the trophy count, an art tag (FNV-1a of path and change time, base 36, like the Mac) and the phone's layout for the game; it is sent again only when it differs. A trophy earned by the running game is forwarded with its grade and icon.

**Virtual controller.** `anyps5_core::remote::vpad`. One Xbox 360 style pad: Cross, Circle, Square, Triangle are A, B, X, Y; L1 and R1 the bumpers; Options is Start; the touchpad click is Back; L2 and R2 are the triggers (a digital L2 or R2 bit counts as a full trigger); the PS button has no bit and is the `home` message. Sticks are 0 to 255 with 128 in the middle and up at 0, so Y is inverted. The pad is plugged in on the phone's first packet while a game starts or runs and unplugged when the game ends or the phone leaves, so no controller shows up while the phone only browses (the launcher would read it itself).
  - *Windows: ViGEmBus* through the `vigem-client` crate (MIT; its one dependency is `winapi`). The driver is **never installed by the launcher**. The settings show "Virtual controller: Ready" or "Not installed", say in two sentences what the driver does and that its author has retired the project (archived since November 2023, still working), and open the official release page (`github.com/nefarius/ViGEmBus/releases/latest`) in the browser. The state is looked at again every two seconds while the page is open, so the line turns to Ready after the install. Rumble from the game goes back to the phone in the output packet.
  - *Linux: uinput*, written directly against `libc` (no crate). The device is "AnyPlay Phone Controller" with the Xbox 360 USB ids (045e:028e) so SDL and games map it. When `/dev/uinput` is missing (module not loaded) or not writable, the page says so and shows the udev rule `KERNEL=="uinput", GROUP="input", MODE="0660"`, your user in the `input` group, then sign in again; "Open the Guide" opens this section. No rumble on Linux yet.
  - *macOS* builds of this launcher have no virtual controller: the page shows "Not available".

**Interface.**
  - *Settings, "Phone as Controller"* (the category after Behaviour, glyph `device-mobile`). Rows: the switch (detail: "Use your phone as a controller. Only phones on your home network can connect, and each one has to be paired first."); with it on: Status ("Waiting for a phone", "Connecting to Pixel 8", "Connected to Pixel 8 · 4 ms": the delay is part of the status while connected), the pairing question when a phone asks (the title "Pair Pixel 8?" with the code as its value, the note "Pair only if your phone shows the same code.", then Pair Phone and Deny as two commands), Address (globe glyph, so every title shares the status row's edge) with the note for manual entry (and notes when the port was busy or the PC could not announce itself), the Paired phones caption with one "Remove Pixel 8" command per phone (ink, not danger red: pairing again undoes it) ("Paired on 2026-10-24"), and the Virtual controller status with its notes. When the server cannot start, one note says so and the switch stays.
  - *The question comes to the player.* When a phone asks to be paired while nothing else is open and no game starts or runs, the settings open on this page (the link `anyps5://settings/phone` does the same). Otherwise a toast says who asks and where to answer; the question stays up for the minute the server waits.
  - *Top bar.* While a phone is welcomed, a status of 32 dp height (no surface, detail ink) with the `device-mobile` glyph and the phone's name (14 medium, cut in the middle past 150 dp) sits 48 dp after the title, clear of the scanning spinner. It is a status, not a button: not focusable, named "Phone connected: Pixel 8" for screen readers. It is ink, not accent: the accent is for being online with the network.
  - *Words.* English and Turkish like the rest (`screens/phone_strings.rs`). The product text names no console brand.

**LudusCtrl** (2026-10-26). [LudusCtrl](https://github.com/parl0v/ludusctrl) is a second way in, for a real controller: the controller is connected to an Android phone or tablet over Bluetooth, and the LudusCtrl app passes its input on to this PC over the home network (its own UDP protocol on port 45989 with a beacon on 45990, a six-digit pairing code, signed input). It is an **option of Phone as Controller**: the preference `phoneLudusctrl`, off by default, and its server runs only while the phone switch is on too. With it on, the browser client (below) runs as well: a phone without the app scans a code and plays, so the switch covers both ways in. The phone only carries the pad: it does not browse the library or start games, and the pad it feeds is the player's controller for the launcher as well as for games. The server is the core's, `anyps5_core::remote::ludusctrl` (its interface in `docs/ludusctrl-api.md`); the launcher meets it in `app::ludus` (the state, the `LudusReceiver` trait and `start_receiver`, the one place the core plugs in), and the rows and words are `screens::ludus` and `screens::ludus_strings`.
  - *Settings.* A section of its own on the Phone as Controller page, after the phone server's rows and only while its switch is on: the switch is a `section_start` row, with `space::XL` more above it than a group gets (SYSTEM.md 5), so it does not read as one more row of the phone server. It has no caption (5.7, "One designed place"): the switch is its heading, and a caption "LudusCtrl" over a switch "LudusCtrl" would say the same twice; its own groups below (Other Addresses, Paired LudusCtrl Phones) have theirs. The switch "LudusCtrl" (glyph `game-controller`, detail: "Play with a controller connected to your phone: scan the code with the phone's camera, or use the LudusCtrl app on Android. A phone with the app has to be paired first."); with it on, the app's server and the browser client start together and stop together, and the group reads, top down:
    1. *The pairing question*, only while a phone of the app asks, in the phone server's form (`phone::pairing_rows`: "Pair Galaxy Tab S9?", the note, Pair Phone and Deny) with the code in two groups of three as the app shows it ("260 329"). It sits right under the switch, above the QR code: it waits a minute for an answer, and the code block would push it out of view.
    2. *The browser client's QR code, link and Copy Link* (below), right under the switch: the first thing a new player needs, since the app is not in the store yet. In the same group under them, *the status*: App (glyph `device-mobile`: "Waiting for a phone", "Connected to Galaxy Tab S9", "Connected to Galaxy Tab S9, no controller" while the phone holds the pad with no controller on it) and Browser (glyph `globe`, the browser client's status, below), then the browser client's notes, Other Addresses, New Link and the firewall note (below).
    3. *The app's rows*: Address (the PC's address with LudusCtrl's port, "192.168.178.20:45989", from LudusCtrl's own address list, `LudusServer::addresses`, so it stays when the phone server could not start) with the note for manual entry, the Paired LudusCtrl Phones caption with one destructive "Remove Galaxy Tab S9" command per phone ("Paired on 2026-10-25") or the line "No phone is paired yet. Open the LudusCtrl app on your phone and start the link.".
    4. *Where the app comes from*: the note "LudusCtrl runs on Android phones and tablets. Pair your controller with the phone over Bluetooth, then open the app." and the command "Open the LudusCtrl Page" (glyph `github-logo`, detail `github.com/parl0v/ludusctrl`).

    *In view at once* (2026-10-26, after the test of the build without the browser switch, at 1920 x 1080). The code has its place from the first frame: while the client starts, the QR row, Link and Copy Link stand there with an empty white card, so nothing above or below moves when the link comes. With the focus on the switch, the list keeps the QR code in view with it (`Row::keeps_in_view`, `scroll::Follow::begin_with`): switch and code are centred together, or the switch goes to the top when the two do not fit, so turning LudusCtrl on, with the mouse or the pad, brings the whole card into view. Before, a click gave the switch the focus and the list centred the switch alone, which pushed the code under the bottom edge.

    No Delay row: the protocol carries no figure to the PC. When the server cannot start for the app, the note "The connection for the LudusCtrl app could not start. Another program, such as the LudusCtrl receiver, may be using its network port." stands in place of App and the app's rows (pairing, address, paired phones) are left out, while the browser client still runs: the core starts the server without the app (`LudusServer::start_without_app`). When the browser client cannot start, its note stands in place of Browser and the app's server still runs. The switch stays either way, and nothing is tried again until it goes off and on. The Virtual controller rows close the page once for both servers (when the phone server could not start, LudusCtrl's look at the driver fills them).
  - *The link.* The row opens the project page, which works for everyone and points to the app. The Google Play listing (`com.parlov.ludusctrl`) opens only for the app's testers until it is in production; the row moves to it then.
  - *The question comes to the player* as it does for the phone server: an idle launcher opens the page, otherwise a toast says who asks and where to answer. An Allow past eight paired phones gets the "Eight phones are paired" toast.
  - *Top bar.* While a LudusCtrl phone holds the pad, the phone's pill (32 dp, `game-controller` as on the switch, so it differs from the phone server's `device-mobile` pill without reading, the phone's name cut in the middle past 150 dp, ink on the buttons' surface) sits 12 dp after the phone server's pill, or 48 dp after the title, named "LudusCtrl connected: Galaxy Tab S9" for screen readers. It is a status, not a button, and is left out when the strip has no room for it; the controller's pill (6.9) follows it. A browser client that holds the pad wears the same pill in the same place, with what its browser runs on as its name ("iPhone"; "Browser", "Tarayıcı", when it did not say), named "Browser connected: iPhone"; only one of the two can hold the pad, so they never stand side by side.
  - *One controller.* Both servers drive the same virtual controller (`app::pad::SharedPad`), so with both on the system sees one. LudusCtrl gets it as soon as its server runs (its pad drives the launcher too) and plugs it in with the first input; the phone server takes it while a game starts or runs, as before, and the game's rumble goes to that phone. A LudusCtrl phone that leaves lets the pad go to neutral at once and unplugs it after 10 s.
  - *Credits.* About carries "LudusCtrl protocol and app by Josip Parlov (@parl0v)" under the credits line, and so does the README.
  - *Words.* English and Turkish in `screens/ludus_strings.rs`; the words the two share (status, the question, Remove, the toasts) are `PhoneText`. No console brand.
  - *Gallery.* `phone-ludus`, `phone-ludus-pairing`, `phone-ludus-connected`, `phone-both` (a phone of each kind connected, the whole page), and `home-ludus` (the top bar with both pills).

**LudusCtrl browser client** (2026-10-26, T55 step 2). A phone without the app opens a page from this PC and its browser passes on the controller connected to it (the Gamepad API), as LudusCtrl's `PROTOCOL.md` describes it (browser section). It is for phones whose browser supports the Gamepad API; on iPhone, iOS 15.2 or newer, because older Safari refuses `wss://` to a self-signed certificate even after the page's certificate was accepted (WebKit bug 145819). It has run on a phone against this launcher on Windows 10, but not yet on an iPhone (with this launcher's ECDSA certificate). It has **no switch of its own** (2026-10-26, after the first test on a phone): it runs whenever LudusCtrl does, so turning LudusCtrl on shows the QR code at once. It started as an option of LudusCtrl, `phoneLudusctrlBrowser`, off by default; a preferences file that still has the key loads as before and keeps it unread (`Preferences::extra`). The bridge is a part of the core's LudusCtrl server and feeds the same pad slot, so the browser and the app share the one virtual controller and only one of them holds it: a browser is refused while the app is linked, and while a browser holds the pad the app's packets are acked but not applied. The launcher meets it in `app::ludus_web` (the state, `BrowserConfig`, `BrowserInfo`, `Certificate` and the `BrowserBridge` trait, which every `LudusReceiver` is; the types mirror the core's of `docs/ludusctrl-web-api.md` field for field, and the trait's defaults say "not built" until the core's bridge lands, `impl BrowserBridge for LudusServer` in `app::ludus` being the one place it plugs in); the rows are `screens::ludus_web` and the words `screens::ludus_web_strings`.
  - *The server, as the launcher expects it.* HTTPS and a WebSocket on TCP 45991 (`LUDUS_WEB_PORT`; a busy port falls back to a free one), one page, a certificate this PC makes and signs itself (`<config>/LudusCtrlWeb.json`, ECDSA P-256 where the PC app uses RSA, made again only when one of its LAN addresses is not covered or it is about to expire), a random token per start in the link's fragment (`https://192.168.178.20:45991/#t=...`). There is no pairing question: the token in the QR code proves the player can see this screen, and a new start or New Link voids it. `start_browser` returns the links (one per LAN address, the most likely first; none when the PC has no LAN address), the port, whether it is the wanted one, what happened to the certificate (`Kept`, `New`, `Reissued`) and its fingerprint; `new_browser_link` returns the same with a new token and the addresses looked at again; `stop_browser` ends it. Who holds the pad comes through the server's status, `LudusVia::Browser { peer, device }`, `device` being a coarse name from the browser's User-Agent ("iPhone", "Android") or none.
  - *Settings.* In the LudusCtrl group (above), right under the switch (after the pairing question when there is one), a group of its own: the QR code of the first link (5.8) with "Scan the code with the phone's camera to open the page, then press a button on the controller." beside it; Link (5.8, the same address in the mono face); Copy Link (detail "Open it on a phone on the same network."; it puts the link on the clipboard and the toast says "Link copied"); App's status and the Browser status row under it ("Waiting for a browser", "Connected to a browser on iPhone (192.168.178.41)", or "Connected to a browser on 192.168.178.41" when the browser did not say what it runs on); the note about the phone's one warning, in the detail ink rather than the quiet note ink because it is the one thing the player has to do on the phone (`Row::step`): "The phone warns once that the connection is not private, because this PC signs its own certificate. On iPhone choose Show Details, then visit this website; in Chrome on Android choose Advanced, then Proceed." (the Turkish names the steps without quoting the phone's own labels), with attached under it, when they apply, the warning "This PC's address changed, so the phone will warn once more." (a `Reissued` certificate; a first one needs no more than the note) and "Port 45991 is in use, maybe by the LudusCtrl receiver. The link uses port 51234 instead."; with no link at all, the warning "This PC is not on a network a phone can reach. Connect it to your home network, then choose New Link." in their place; when the PC has more than one LAN address, the caption Other Addresses with "If the phone is on another network of this PC, open the link of that network." and one command per address (the address as its title, the whole link as its detail, select copies it); then New Link (glyph `arrows-clockwise`, detail "Makes a new link and disconnects the browser. The old QR code stops working.") with the note attached under it "The link opens the page for anyone who has it. Choose New Link after you share this screen." (the link on screen carries the token). The token does not change when the settings close: the player usually closes them to play, and a new token would disconnect the phone. After the browser's rows, on Windows, the group's one firewall note, for the app and the page alike (`ludusFirewall`, also when the browser client could not start): "The first time LudusCtrl runs, Windows asks whether AnyPlay Launcher may use the network. Allow it on private networks, or neither the app nor the page can reach this PC.". When the bridge cannot start, one note says so in place of the Browser row ("The browser client could not start. The log has the reason.") and the code and its notes are left out; it is not tried again until LudusCtrl goes off and on.
  - *The page's language* is English: the core serves one page and takes no language (`docs/ludusctrl-web-api.md`).
  - *Toasts.* "A browser is connected as the controller" when a browser takes the pad; "Link copied".
  - *Words.* English and Turkish in `screens/ludus_web_strings.rs`, the status title shared with the page (`PhoneText::Status`). No console brand. The status names what the browser runs on ("iPhone") because the browser said so, not as a promise.
  - *Gallery.* Every LudusCtrl scene shows the browser client too (`phone-ludus` has the focus on the switch, as just turned on, with the code in view). `phone-browser` (waiting: the QR code, the link, two other addresses), `phone-browser-linked` (a browser holds the pad), `phone-browser-reissued` (a new certificate and a busy port), `phone-browser-failed` (the app's server runs alone), `phone-ludus-failed` (the browser client runs without the app), and `home-browser` (the top bar with the phone's pill and the browser's).

**Tests.** `fake-phone` (`crates/fake-phone`, never shipped) is a phone and a game in one binary: `fake-phone phone --addr 127.0.0.1:47700 --key-file k.txt --script "select:ID,launch:ID,press:cross,..."`, `fake-phone phone --discover` finds the PC through mDNS, `fake-phone game` greets on the bridge. `crates/fake-phone/tests/e2e.rs` drives the real server with it: protocol round trip, pairing and key reuse, an unpaired phone rejected, a denied phone rejected, a request flood limited. The unit tests of `remote` cover the packets (golden hex), the device file, the layouts file, the rate limiter, the on-link rule, the relay's identity checks, the mDNS adapter choice and the pad mapping. The scenes render in the gallery; `docs/media/phone-off.jpg` is a real screenshot of the off state. Gallery scenes: `phone-off`, `phone-waiting`, `phone-pairing`, `phone-connected`, `phone-driver` (Windows without ViGEmBus), `phone-access` (Linux without access to uinput), `phone-failed`, and `home-phone` (the top bar). Not tested without hardware: a real phone on a real network and a real virtual pad in a real game (any game needs the user's go).

---

## 19. Mac style (2026-10-26)

The Mac version is this launcher: the same screens, layout, features and version as on Windows and Linux. On macOS only, it is dressed in **Mac style**: a see-through window with the system's blur behind it, the Mac's window chrome, and the Mac's conventions. Windows and Linux keep their look unchanged. All of it is one module, `macstyle` (`crates/anyps5-launcher/src/macstyle.rs`); the AppKit code in it is compiled on macOS only, and elsewhere the module does nothing. Nothing here comes from another launcher; it is our own design and code.

### 19.1 The window

| Item | Value |
|---|---|
| Window | transparent (`with_transparent`), standard decorations with a transparent title bar, no title, the content under the title bar (`with_fullsize_content_view`). The traffic lights are the system's. Dark appearance, always: the launcher is dark only. |
| Start | a window the size of the screen's usable area, not full screen: the default of the `fullscreen` preference is off on a Mac (on elsewhere), because the glass needs the desktop behind it. The green button and Ctrl+Cmd+F use the system's full screen; F11 and the preference still work. |
| Drag | the system's title bar drags. A strip along the top edge (28 points, never into the home screen's top bar at 34 dp) is the fallback if the title bar passes the press on. |
| Caption | none: the Windows caption buttons (9.2) are not drawn. |
| Traffic lights | they occupy about 84 x 34 points in the top-left corner. The home top bar starts at 34 dp and its centre line is at 56 dp, so even at the smallest window (scale 0.69) the title sits below the lights; a test (`macstyle::tests`) keeps it so. Panels start at 64 dp or lower. |

### 19.2 Glass

The window is **see-through** when all of these hold: the player's setting "Glass Window" is on (default on, Settings > Appearance, `glassWindow`; EN "Glass Window", TR "Cam Pencere", after "Show the Selected Game's Artwork"), the window is not in full screen, no game runs, and the system setting Reduce Transparency is off (`NSWorkspace.accessibilityDisplayShouldReduceTransparency`, read at most once a second, when a frame is drawn, so a window nobody touches wakes for nothing; Increase Contrast counts the same). Otherwise everything is solid, as before: the animated theme background on an opaque black window. The rule is one function, `macstyle::glass_active`.

Layers, from the back:

1. **The system blur**, a view behind the content view in the window's frame view: `NSGlassEffectView` (Liquid Glass, style Regular) on macOS 26 and newer, found at run time by class lookup; `NSVisualEffectView` (material sidebar, behind window, always active) on earlier systems. `ANYPLAY_GLASS_MATERIAL=blur` forces the older one. Hidden, and the window made opaque, when the window is not see-through.
2. **A plate**: black 0.05 over the whole window (`GLASS_PLATE`).
3. **The theme**, a still of the theme background (the shader frame at 260 s, the same as the gallery stills) at opacity 0.20 (`GLASS_VEIL`). Nothing moves: the backdrop clock stops and asks for no frames, and the offscreen image is not redrawn, so an idle launcher costs nothing. The theme's colour still shows through as a tint, which is what makes the glass this launcher's and not any window's.
4. The game artwork at 0.40 of its opacity (`WINDOW_GLASS_ARTWORK`), under its scrims at full strength, so the window shows through the picture and the text keeps at least the contrast it has on a solid window; then the UI, unchanged.
5. **Glass panels** (section 4), the side panel and the full-screen panels keep their recipe and stay opaque: they sit on top of the screen's own UI, so making them see-through would show that UI, not the window. Text sits on the panels and on the plate, never on the bare blur.

Reduce Transparency, full screen and a running game each switch to the solid look; a game in front also hides the effect view, so the window server has no blur to compute for a window nobody sees.

**Contrast.** The system blur's brightness is not under our control, so the worst case is a bound. With the dark look asked for (19.1), the macOS 15 and macOS 26 runners show the blur over a white wallpaper at sRGB 92 to 96 (gray; the probe images of the `macos-style` job), and over saturated bars at the same luminance. The bound is 105 (`WORST_BLUR` in `ui::contrast`). Over that, plate, theme (deep and lit tones) and the side panel are composited like every ground in 3.10; `ui::contrast` measures them (grounds "glass window", panels "glass") and every ink reaches its ratio there too (the weakest pair, a hint key on Aurora, is 5.5:1). The artwork under the glass is drawn at 0.45 under scrims of full strength, which is never brighter than the full picture under the same scrims that 3.10 measures. If a system brings a brighter blur, raise `GLASS_PLATE`.

### 19.3 Chrome and the top of the screen

The traffic lights sit in the top-left margin above the top bar (19.1). In full screen they are hidden by the system, and nothing else changes. The window's size limits (1100 x 650) and scaling (1.1) are unchanged.

### 19.4 Menu and keys

The menu bar (replacing the default, whose items would carry the executable's name):

| Menu | Items |
|---|---|
| AnyPlay Launcher | About AnyPlay Launcher (the system's about panel, from the bundle: name, version, icon), Settings… ⌘, (opens Settings when onboarded, like the shortcut), Hide AnyPlay Launcher ⌘H, Hide Others ⌥⌘H, Show All, Quit AnyPlay Launcher ⌘Q (closes the window like its red button, so the launcher saves and ends through its own path) |
| Window | Close ⌘W, Minimize ⌘M, Zoom, Enter Full Screen ⌃⌘F |

The menu's items use the system's own actions; only Settings is ours. The menu is in English: the launcher's language setting does not reach AppKit's menu.

Keys: wherever Windows and Linux use Ctrl, a Mac uses Cmd. There is one such shortcut, Settings (`Ctrl+,` / `Cmd+,`); egui's `command` modifier is that key on each system. Everything else is as in 6.1: any key with Cmd (or Ctrl, Alt) held is ignored by the launcher, so the system's own shortcuts (Cmd+H, Cmd+Q, Cmd+M) are never taken. In the controls panel the key caps say `Option` and `Cmd` on a Mac (6.5), `Alt` and `Win` elsewhere. The ⌘ appears in the menu, where the system draws it; the launcher itself shows no Ctrl hint anywhere.

### 19.5 What is checked where

Compiled and linted for `aarch64-apple-darwin` on the build laptop (`cargo check` and `clippy`, with the C build steps of dependencies stubbed) and in the CI job `macos`. The job `macos-style` renders a few scenes with and without the glass (`--solid`) on the Metal runner and saves the launcher's own frame and a `screencapture` of the screen with the window on it. What needs a real Mac (the system blur's look over real wallpapers, Liquid Glass, the traffic light positions, dragging, native full screen, Reduce Transparency, the menu) is listed in the pull request that added this section.
