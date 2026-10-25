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
18 Phone as controller.

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
| Scrim behind side panel | black 0.35 |
| Full-screen panel | black 0.74 over blurred backdrop (settings, library, trophies); black 0.65 for the log |
| Side panel | `Color(white: 0.06)` at 0.92 over blurred backdrop, 1 px left edge white 0.08. SwiftUI's `Color(white:)` is a **gray level** (rgb 15,15,15), not white with alpha; the same holds for the next row |
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
| Home tile row | top 34 below the top bar; leading 64 |
| Home hero info | horizontal 72 |
| Home hint bar | horizontal 64, vertical 28 |
| Onboarding | horizontal 110, vertical 80 |
| Settings | horizontal 90, top 70, bottom 50 |
| Library | horizontal 80, vertical 60 |
| Trophies | horizontal 80, vertical 56 |
| Log | 70 all around |
| Side panel | 36 all around, width 560 |
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
| 2 | Tile row move on selection change | `Motion.layout` on `selected` | per tile spring layout on x, y offset, size (see 5.3); previous tiles fade to 0 opacity as they leave left |
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
| 23 | Side panel in/out | move from the trailing edge + opacity, `Motion.overlay` | spring overlay: `offset_x = (1 - v) * panel_width`, opacity v |
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
**half the physical pixel size** (`drawableSize = bounds * backingScale * 0.5`) at 60 fps, is soft by design, and leaves the GPU to
the games.

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
- **Hero.** One left edge with the top bar (`HERO_X = 64`). The four boxed info cards become a
  plain facts line (Size, Title ID, Version: 13 dp tracked overline over a 17 dp value, 48 dp
  apart, no card). The status card is gone; the state stands above the title. Switching games
  cross-fades without the 14 dp slide (it happens on every arrow press). The "more" button is a
  54 dp square with three painted dots.
- **Hint bar.** Follows the device in use (`input::device`): keycaps `Enter`, `Esc`, `O` on a
  keyboard, ✕ ○ △ in circles on a gamepad. No combined labels, no Move hint. 26 dp keys, 15 dp
  labels, 28 dp between hints.
- **Top bar.** Outline icons at 20 dp (`squares-four` for the library, `users` for friends, `gear-six`
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
  (Stable, Pinned, Local Build) and shows only the rows of that channel: stable has the update
  toggle and "Check Now" (its detail line carries the status or the last check), pinned has a
  `Version` choice over the last ten releases and no update, local has the folder row. "What's New"
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
  backdrop is untouched and still draws at 60 Hz while focused. A window without focus, and a game
  running, stop all drawing (0.2 to 2 % of a core). The gamepad thread polls every 100 ms while no
  pad is connected (4 ms with one). `ANYPS5_GPU=dx12|vulkan|both` and `ANYPS5_POWER=high` undo the
  choice for diagnosis; the log carries `startup t=...ms <step>` lines and the demo an `idle ...
  frames/s` line every 5 s. The demo no longer forces a repaint every frame (it idled at 40 % of a
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
| Medium | 44 | 16 semibold, 20 padding | 18 (20 alone) | the top bar: library, friends, settings, profile |
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

**Focus is a ring.** One treatment everywhere: a white 0.95 ring outside the shape with a 1.2 dp gap, 2 dp wide on controls
(`size::FOCUS_RING`) and 3 dp on tiles, cards and cells, plus a fill one step lighter. Nothing grows, nothing swaps its ink and no
glow sits behind a translucent fill (a shadow under a translucent fill shows through as a seam). The layout never moves when focus
moves, so the 12 dp gap holds. The ring is the focus indicator for keyboard and gamepad: it is white on dark artwork and on every
fill of the family (a primary focused is a white ring around a white capsule, separated by the gap). Containers (rows, cards,
cells) use `components::paint_surface`: rest fill, +5 % white on hover, +9 % and the ring on focus, +6 % pressed.

**Hover** exists on every control the pointer can reach and answers like the tiles do: the tiles draw the focus ring at 45 % under
the pointer, controls lighten and show a brighter hairline. Only colour and opacity change, in 120 ms (`Motion::Hover`, the same
under reduced motion because it moves nothing); the press scale (0.97) is the only transform.

**The clock** is a status line, not a control: 17 medium, white 0.78, set in a box four digits wide with the colon fixed in the
middle (two dots on the digits' cap height), so the minute ticking over moves nothing and the digits need no tabular cells
(whose spaced 1s read as a typo). Centred on the bar's centre line with the buttons.

**Failed unlock** says what happened and what to do in one headline ("Unlocking stopped. Try again. ..."), with the technical
message as a quiet line under it.

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

**The question of a confirmation** (Close Game, Remove, Restore, the toolkit pin) is the sentence the player decides on: 17 dp
regular in `text_label` ink with 1.45 leading under the panel title, not the quiet 15 dp line of a subtitle (`SideView::question`).

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
| starting | black 0.42 + small spinner | hourglass, accent, `starting` | hourglass, `starting` (inactive) |
| running | accent dot (0.09 size) top-right with glow | play-circle, accent, `running` | play-circle, `running` (press re-activates the game window) |

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
library button (glyph: 3x3 grid), friends button (glyph: two people; see below), settings button (glyph: gear), 12 dp apart; then 32 dp
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
x = offset < 0 : 64 + offset * (118 + 16)            // tiles left of the selection sit off to the left (opacity 0)
    offset == 0: 64
    offset  > 0: 64 + 176 + 16 + 300 + (offset - 1) * (118 + 16)     // after the 300-wide label column
y = selected ? 0 : (176 - 118) * 0.18               // small tiles hang 10.44 lower
size = selected ? 176 : 118
opacity = offset < 0 ? 0 : 1
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
onboarding show it without "Options". The glyphs are the fixed text above (PlayStation face names), not adapted to the pad in use.

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

Layout: full-window overlay (blurred backdrop + black 0.55); centred column, gap 26: tile 200 (radius 24, same placeholder rules),
breathing glow (animation 26); game name (34 light, centred, 2 lines); a 380-wide block (gap 14): row `starting` (16 medium, white
0.88) and the elapsed `m:ss` (16 semibold tabular, counting from 0:00, tick 1 s), the indeterminate sweep (accent), and
`startingText` (14, white 0.6, centred, wraps). At the bottom, 40 above the edge, a hint `esc / ○` "Cancel". After the window shows,
the splash leaves (overlay spring), the launcher optionally hides ("Hide Launcher While Playing"). (S-starting.)

### 5.6 Toast

Pill at the top centre (top 26): glyph 17 semibold + text 16 medium (max 2 lines); padding 22 x 14; glass radius 28 with black 0.6;
shadow (1.4). 3.2 s on screen, spring in/out (animation 27). Events that raise toasts: unlock ready/failed, unreadable dump, no
toolkit (`noToolkit`), start failed, uninstall done, shader cache cleared, report saved/failed, settings exported/imported/file
invalid, "game: diagnosis".

### 5.7 Settings (global)

Two columns on a black 0.74 blurred panel (S-settings). Left (330 wide + 36): title `settings` (40 light) with 26 below, then
six category rows (height 52, gap 6, radius 12, glyph 18 medium in 26 box + 14 gap + title 19). Selected category: white 0.12 fill,
text semibold; **focused** (keyboard focus in the category list): white fill, black text. Unselected text white 0.7. Right: the
category's rows (5.8).

| Category (key) | Glyph | Rows (id, kind, key, notes) |
|---|---|---|
| Appearance (`appearance`) | brush | `theme` choice (Cosmic/Aurora/Sunset), `accent` choice (six), `backdrop` choice (`backgroundTheme` / `backgroundImage` / `backgroundVideo`; picking image/video opens a file dialog when no file is set or the kind changed), `file` action `chooseFile` (only when backdrop is not theme; detail = file name), `artwork` toggle `gameArtwork`, `motion` choice (`motionFull` / `motionReduced`) |
| Library (`storage`) | 2x2 grid | `folder` action `folder` (detail = path or an em dash; opens the folder dialog), `rescan` action `rescan` (detail `gamesFound` with count; sound `select`), info `installedSize` |
| Language (`language`) | globe | `language` choice (Türkçe, English; shown in their own language) |
| Behaviour (`behaviour`) | sliders | `name` text field `profileName` (live edit), `sounds` toggle, `music` toggle (Home Screen Music, 7.3), `hide` toggle `hideWhilePlaying`, `prepare` toggle `prepareGames` (Prepare New Games Automatically), `hiddenTrophies` toggle `showHiddenTrophies` (trophy glyph; detail `showHiddenTrophiesDetail`; the trophy list's only option, 5.11) (+ platform row, see 9) |
| Phone as Controller (`phone`) | phone | Windows and Linux addition, section 18. Sits between Behaviour and Toolkit. |
| Advanced (`advanced`) | wrench+screwdriver | `screenshots` action `screenshotsFolder` (detail = path, `~` abbreviated), `showScreenshots` action, `fonts` action `fontsFolder` (detail = the picked path, else the built-in Noto set and whether Japanese, Chinese and Korean are included, not downloaded or downloading), `fontsReset` action (only when a fonts folder is set), `fontsCjk` command (download the Japanese, Chinese and Korean fonts; only without a picked folder and while they are missing), info `fontsHint` |
| About (`about`) | info circle | info `aboutText`, info `version` (app version), info `toolkitVersion`, (platform row), `quit` action `quit` (destructive) |

Settings are persisted per user (`Preferences`): `onboarded`, `language` (system default: Turkish if the OS UI language starts with
`tr`, else English), `theme` (Cosmic), `accent` (nil = theme default), `backdrop` (theme), `backdropFile`, `gameArtwork` (true),
`reducedMotion` (false), `showHiddenTrophies` (false), `sounds` (true), `music` (true), `discordPresence` (true), `hideWhilePlaying` (false), `prepareGames` (true), `profileName` (first word of the user's full name, fallback
"Player"), `gamesFolder`, `screenshotsFolder` (nil = `Pictures/AnyPS5`), `fontsFolder`, per-game settings, `lastPlayed`.
Every change saves immediately and re-applies language, motion and sound flags.

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

Row chrome: min height 58, padding h 18, radius 12; fill white 0.07 (focusable) or none (info); **focused**: white fill, black
foreground, glow, scale 1.015. Destructive actions: title `#FF998C`, glyph `#FF8C80` (white-on-focus rule unchanged: black when
focused). Focused row scrolls to the centre of the list (animation 31). List scroll bars are hidden.

**Key cap** (controls): min 30 x 28, radius 7, padding h 9, text 13.5 semibold; fill white 0.12 for keyboard keys and white 0.06 for
mouse/wheel sources (black 0.12 / 0.06 when focused), 1 px stroke at 0.18. No binding: the text `noKey` (15 medium, 0.5). Waiting for
a key: `pressKey` (15 semibold, pulsing, animation 17b).

### 5.9 Game settings side panel (the "..." menu)

Opened by "..." or `O` / Y / Menu / Options. Right-aligned panel 560 + 72 wide over a black 0.35 scrim (click on the scrim closes), see
1.2 for fills. Title = game name (30 light), subtitle `gameSettings` (15, white 0.6), rows below (18 gap). (S-game-settings.) Rows
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

**Confirm uninstall panel**: title `uninstall`, subtitle `uninstallConfirm`; two rows: `uninstall` (destructive, trash glyph) and
`cancel`. Opens in rows focus on the first row.

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
to the centre (animation 31). (S-library. The tile shown "Playing" uses the coral accent.)

### 5.11 Trophy list and trophy button

Home: the trophy button between Play and "..." appears only for games with a trophy list: label `"8/34"`. The list overlay (S-trophies):
black 0.74 blurred; header row (gap 26): set icon 112, then a column (gap 12): overline `trophies` (13 semibold uppercase tracking
0.8, 0.55), title (set title or game name, 36 light, 1 line), a row (gap 16): percent `"8%"` (22 semibold tabular), progress bar
260 wide (accent), and per grade with at least one trophy a count `"unlocked/total"` (trophy glyph in the grade colour + 15 semibold
tabular, white 0.85); hint bar (no options) right. Below, a scrolling list (gap 10, padding 8) of trophy rows in package order:

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
(animation 19), focus on the tiles, rescan if empty. Step 3 focus: 0 Sign In, 1 Create Account, 2 Not Now. Every choice
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

When the system is older it says one sentence, the same in three places: "Games need glibc 2.38 or newer; this system has 2.35. Update to Fedora 39, Ubuntu 24.04 or newer." (the next step, and the install-guide row in Settings, are from round 3, 3.10) A note under the version row in Settings > Toolkit (like the "newer than tested" note, gallery scene `toolkit-glibc`), and a warning line under the primary button of the hero, in the states Locked, Queued, Installing, Failed and Installed (scenes `home-glibc`, `home-glibc-locked`; it takes the line the friends playing use, since it says why a start may fail). It is a notice and never a block: Unlock and Play work as before. Nothing is shown when the system is new enough, when a version cannot be read, on Windows and on macOS.

## 6. Input

Original: `Input.swift`, `AppModel.handle`, `panelCommand`, `onboardingCommand`. One handler consumes seven abstract **commands**:
`left right up down select back options`.

### 6.1 Keyboard

| Key (original macOS) | Command | Windows / egui key |
|---|---|---|
| Left / Right / Up / Down arrows | left / right / up / down | `ArrowLeft` .. `ArrowDown` |
| Return, keypad Enter, Space | select | `Enter`, `NumpadEnter`, `Space` |
| Esc, Delete (backspace key) | back | `Escape`, `Backspace` |
| letter O (any layout, ignoring shift) | options | `O` |

Rules: any key with the **Command** modifier is ignored (Windows: ignore when Ctrl, Alt or the Windows key is held, except the
shortcut below). While a native dialog is open the launcher does not handle keys. While a text field has focus (profile name) keys go
to the text field (the original never handles them; the port additionally lets Enter and Esc blur the field, see section 9). While a
binding waits for a key (controls panel) every key is captured by the panel. Key repeat is ours, not the OS's (2026-10-25): the OS repeat events of an arrow
key are dropped and a held arrow runs the same repeat as a held direction on the pad (6.3), so a list moves the same under a finger
and under a thumb. It ends when the key is up, when a dialog opens or when a binding starts to wait for a key. **Shortcut**: `Cmd+,` opens Settings when onboarded -> `Ctrl+,`. F11 toggles fullscreen (Windows addition).

### 6.2 Mouse and wheel

Everything also works with the mouse (clicks listed per component). Mouse wheel / horizontal scroll on the **home screen with no
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
settings row 0, confirm row 0, controls row 1, advanced row 1; settings starts at the category level with category 0.
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
preference, focus) as atomics; the UI fills it every frame (`LauncherApp::sync_home_button`). Three sources press it, each on its
own thread, because a minimised launcher draws no frames: the gilrs thread (`Button::Mode`), a thread polling `XInputGetStateEx`
for the Guide bit (Windows; Windows.Gaming.Input, which gilrs uses, hands a pad to the focused window only and never shows
Guide), and the phone server. Presses within 350 ms count once (gilrs and XInput can both see one press). The pure `route`
function is the state machine and carries the tests. The UI takes two flags once per frame, `take_summoned` and `take_returned`,
and opens or closes the panel.

**The quick panel** is a side panel (`Overlay::InGame`, `screens::ingame`) in the family of 3.9 and 5.9: the game's name, the line
"The game keeps running behind the launcher.", and the rows *Back to Game* (the focused row on open), *Trophies*, *Game
Settings* (the game's own sheet), *Friends and Party* (only when signed in) and *Close Game* (destructive). *Close Game* opens the
question `Overlay::ConfirmCloseGame` on *Keep Playing*, so Enter never ends a game. The panel selects the running game first, so
trophies and settings are that game's. Back on the panel, a click beside it and *Back to Game* all return to the game. Back on the
home screen does the same while the launcher is there because of the Home button (`Model::summoned`); it is cleared when the
launcher loses the focus, and when the game ends the panel closes.
While the panel is up, the phone's navigation reaches the launcher and its virtual pad is detached from the game.

*Windows.* A background process may not simply take the foreground, and a gamepad press counts as no input event.
`win::activate_window` restores the window if minimised, calls `SetForegroundWindow`, and when that does not take, attaches the input
queue of the foreground window's thread to its own for one `BringWindowToTop` plus `SetForegroundWindow` (`AttachThreadInput`), with
three tries. It sends no key to the game and changes no setting. The same function serves the second-launch hand-off.

*Linux.* The launcher asks its own window to show and take focus. X11 window managers may honour that or only flash the window;
a Wayland compositor ignores it. The game's window is not found there, so the way back minimises the launcher and leaves the
game what is behind it.

*The game's own use of the button.* The mapping of AnyPS5 v0.1.3 names no PS or Guide button, so nothing the game gets is taken
away. If a later AnyPS5 maps it to the PS5 home button, both would react; the preference turns the launcher's side off.

*Not verified with hardware:* that Windows delivers a physical pad's Guide button to the launcher behind a game, and the
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
- **Settings.** Behaviour, "Show Game on Discord" (`discordPresence`, on by default), with the
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
| `uninstallConfirm` | Remove the installation? Your game files stay; the game shows as locked again. | Kurulum kaldırılsın mı? Oyun dosyaların silinmez, oyun tekrar kilitli görünür. |
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
| Menu bar: `Cmd+,` Settings, no New | no menu bar; `Ctrl+,` opens Settings (when onboarded); `Alt+F4` closes; the "Quit" row in About stays |
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
| `installedSize` | Unlocked games are kept in the .AnyPS5 folder inside your games folder. | Kilidi açılan oyunlar oyun klasörünün içindeki .AnyPS5 klasöründe saklanır. |
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

No counterpart in the macOS launcher (Sparkle there). Core module `anyps5_core::selfupdate`, host `app::selfupdate`. The Windows exe and the Linux AppImage update themselves; a Linux `.tar.gz` is replaced from the releases page.

- **Check.** At the start and then every 6 hours while the launcher stays open (after a check that could not reach GitHub, every 30 minutes), in the background, when the *Check for Updates* preference is on. *Check Now* on the About page checks once whatever the preference says. The request is conditional: the release data of `anyplaynet/launcher` is asked for with the ETag of the last answer, so a check that finds nothing new costs one small reply that GitHub does not count against its rate limit. The last copy is never taken for an answer when the network fails: an offline launcher does not call itself up to date. A newer semver (pre-release tags sort below their release) is downloaded and verified at once; garbage tags are never newer. Previews, the demo, the gallery and the tests never check.
- **Verify.** From the first trusted key on (`data/update-keys.txt` holds "anyplay 2026" since 0.2.0) the launcher takes updates only through the release's signed `latest.json` and `latest.json.sig`: an Ed25519 signature by a trusted key, an unexpired manifest, a version newer than the running one, a running version not below the manifest's `minimum_version`, and a download with exactly the manifest's SHA-256 and size. The portable `AnyPlay-Launcher-<v>-x64.exe` (or `AnyPS5-Launcher-<v>-x64.exe`, which every release also carries, 9.7) goes to `%LOCALAPPDATA%\AnyPS5\Updates` under its final name only after that. There is no fallback to the unsigned `SHA256SUMS.txt` flow. The format, the rules, the migration and the key ceremony are in [UPDATES.md](UPDATES.md).
- **Swap.** Windows cannot overwrite a running exe but can rename it. The staged file is copied beside the running exe (`<name>.new`, it may be on another drive), hashed again, the running exe is renamed to `<name>.old`, the copy takes its place. A failure in the second step moves the old one back. The next start deletes `.old` files and stale downloads (one still in use stays until a later start). It works for the installed exe (`%LOCALAPPDATA%\Programs\AnyPlay Launcher`, or `...\AnyPS5 Launcher` for an install from v0.1.x) and for the portable exe in any writable folder. The swap keeps the exe's file name; nothing depends on it. A folder that refuses new files is reported before anything is downloaded.
- **Where the player sees it.** An update is visible in two places, and never as a toast or a progress bar in the way.
  - *About*, under *Version* (which always shows the version that runs): the row *Launcher Update* with one short value and, under it, one sentence and at most one row that acts. Checking: "Checking…". Nothing newer: "Up to date" and *Check Now*. Downloading: "Downloading 42%", then "Verifying"; this is the only place with progress. Ready: "0.2.5 ready", "Restart now, or it installs the next time you close the launcher.", *Restart Now* and *What's New in 0.2.5*. Held for a game: "Waiting for your game", *Cancel Restart*. A read-only folder, a release that was refused (signature, expiry, hash, size) or any other failure: "Not installed" or "0.2.5 available" with the one thing to do, *Download from GitHub*. Offline: "Not reachable" and *Check Now*; the launcher tries again by itself. A refusal is logged and says nothing anywhere else.
  - *The notice on the home screen*, after the title, only while a verified update waits and the player has not sent it away: "Launcher 0.2.5 is ready" with *Restart Now* (the one primary button) and *Later*. *Later* hides it until the next start; the About row stays. While a restart waits for a game, the notice says "Restarts when your game closes" with *Cancel Restart*. The pad reaches it from the library button (left of it), so every action has a key and a pad path. It is out of the way of the top bar and only appears where it fits.
- **Restart Now.** Closes the launcher, swaps, starts the new exe with the same flags and **opens where the player was**: the link of the selected game (`anyps5://game/<TITLEID>`, which selects it again) or, from the settings, `anyps5://settings/about`. It never interrupts a running game: while a game runs or starts, the restart is held, the About row and the notice say so, and it goes ahead the moment no game runs. Closing the launcher by hand still installs the update, without starting it again.
- **What's New.** The notes of the staged release are taken from the release data the check fetched anyway (display only; nothing is decided by them) and shown in the same full-screen panel as the toolkit's notes: every `###` heading of the release is a group, every bullet a line, all groups unfolded. *Back* returns to About.
- **Linux.** The AppImage updates itself the same verified way, through the signed manifest only (its `linux-x86_64` entry; Linux has no unsigned path). The running image comes from `APPIMAGE` (checked against `APPDIR`, a regular `.AppImage` file in a writable folder); the verified download is copied beside it, set to 0755, hash-checked once more and renamed over it atomically, which Linux allows while it runs. Restart starts the new file with the same arguments. A launcher from the `.tar.gz` cannot replace itself: About shows "<v> available" with *Download from GitHub*, and nothing is downloaded. A manifest from before the Linux entry offers Linux nothing, quietly. Details in [UPDATES.md](UPDATES.md#linux-the-appimage).
- **Not done.** The exe is not code-signed (Windows SmartScreen); that costs money and is the owner's decision. Provenance (GitHub attestations) is separate, see UPDATES.md.

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

---

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
  second tick, toast and banner timers) and nothing when idle with the background paused. While the shader animates, cap at 60 fps.
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
| `chrome` | `paint_tile` (5.1), `info_card` / `facts_line` (5.3), `PanelRow` with all kinds (5.8), `side_panel` (5.9), `fullscreen_panel`, `toast` (5.6), `language_card`, `onboarding_dots` |
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
| `Controls`, `Advanced` | `controls`, `advanced` | rows of the game settings | back returns to the game settings on the row that opened them (no slide); a click on the scrim closes everything |
| `Trophies` | `trophies` | trophy button, row of the game settings | back to home |
| `Log` | `log` | "Open log" row | back to home |
| `ConfirmUninstall` | `UninstallPanel` | "Uninstall" row | both answers close everything, as in the original |

- **Keeping in step.** A panel opens when `Model::overlay` becomes its overlay and closes (slides out) when another overlay or none is open. The four side panels share one frame, so switching between them swaps the content without a slide. Commands routed by the model are queued in the host and handed to the panel with the raw captured keys of the controls panel.
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

**Friends screen** (`screens::friends`, its host `app::panels::friends`, the return path `app::profile`). A full-screen overlay (`Overlay::Friends`) in the family of the profile screen: glass, 90 dp of side padding, the hint bar at the top right, a Back button (the console button, small, with the caret) at the top left for the pointer. It replaced the drawer of the first online area; the pages below are unchanged and are now drawn in its page column.
  - *Frame.* A header (the avatar, 64 dp, the first letter on one of six calm colours without a picture; the handle as the title, 40 light; under it the status, "Online · 3 friends active", "Connecting…" or "Offline, trying again", with the accent dot while connected). Under it, on the left, a **rail** 248 dp wide with the sections Friends, Party, Messages, Activity and Settings, and below them, set apart, Your Profile (an action: it opens the profile screen). A rail entry is 52 dp high with a 24 dp glyph and a 17 medium title; nothing sits in a box at rest. The section on screen is a faint surface (white 10 %) with a 3 dp bar at its edge, so it does not rest on colour alone; the focused entry has the white ring of the control family (3.9). What waits shows as a count capsule at the entry's end (light ink on dark, `9+` above nine): requests and invitations on Friends, party invitations on Party, unread messages and message requests on Messages. The accessible name says it: "Messages, 3 new". Right of the rail, 56 dp away and at most 940 dp wide, is the page: its title (30 light) and the line under it (15), then the rows. The title and the line stand on the left edge of the rows' text (the list's 6 dp inset plus a row's 18 dp padding), so a page has one left edge; the line is a label for the screen reader. Rows of the list fade out as the edge reaches them (opaque while a row rests inside the inset, gone when the cut reaches its text, the heights taken from the frame before), so nothing slides under the title and is cut mid-line. Before sign-in there is no rail: the page stands alone in the middle, 640 dp wide.
  - *Sections and pages.* Every page of the table below belongs to a section (`Page::section`): Friends holds Home, a friend, the requests, an invitation, Add a Friend, Report and the invitation picker; Party holds the party, a member, Invite Friends, a party invitation, Voice and the devices; Messages holds the list, a conversation, a request and a message; Activity is the feed; Settings is the account page, a blocked account and (when opened from there) a restriction. A restriction page belongs to the page it was opened from. The first page of each section (`Page::is_root`) is where Back leaves the screen; any other page goes up one. **Messages is a seam**: the Messages section hosts the existing pages as they are (`social.rs`), and the messaging work replaces them without touching the frame.
  - *Friends page.* The default. From the top: the state of the connection when it is not up (Connecting… as a note; offline as a note with Try Again, which starts the net service again), a limit on the account, friend requests (up to five, each with its avatar, "Wants to be your friend", Accept and Decline), invitations (a game's, then a party's), Playing Together (only once someone besides you is in the session), Add a Friend, then the friends in three groups with their own captions: Playing Now (joinable first), Online, Offline, then Sent Requests. A friend's row is the avatar (36 dp; a dot in the accent while they are here, quieter while they are not), the handle, what they play, and the quick actions at its end: **Join** (only when their session is open to friends), **Message**, **Invite to Party** (when the player may add to the party and there is room), **View Profile** and **Options** (their page: call, remove, block, report). The actions rest hidden and come up with the focus or the pointer; select presses the chosen one (the first to begin with), left and right choose, and a click on the row's body opens Options. A friend with no profile picture on this PC shows the monogram. With no friends yet the page says "No friends yet. Add one by the name they chose." over Add a Friend.
  - *Party page without a party.* The first page of Party stands without one: a note ("A party keeps you together across games, with chat and voice."), the party invitations that wait, Start a Party and Voice. In a party it is the party page of the table.
  - *Focus.* Two zones (`friends::nav::step`, pure and tested): the rail and the page. Up and down walk the rail and show the section they stop on; right or select crosses to the page; left on the page crosses back to the rail once the row under the focus has nothing left to step through (a row of buttons or a choice keeps left until it is on its first). Back from the rail or from the first page of a section leaves the screen. The pad, the keyboard and the pointer reach every entry and every row; the screen reader follows the ring (`ui/a11y.rs`) and reads the rail as buttons, the current section selected.
  - *Back.* Back returns to where the screen was opened from, like the profile screen: home for the friends button and the profile button before sign-in; the profile screen when it was opened from there (`Model::friends_back`). A friend's View Profile opens their profile screen; Back from it comes back to the Friends screen on the page and the row it left.
  - *Motion.* The screen comes in and goes out with the overlay spring of the other full-screen panels (opacity and a scale from 1.03); nothing else moves except the rows' own focus and hover, and the quick actions fade with the focus. Reduced motion swaps the springs for the short eases of every other overlay (`AnimExt`); no motion of this screen is the only carrier of a state.
  - *States.* Signed out: the Start page (Sign In, Create Account, the cloud) in the middle. Offline and connecting: the note and Try Again. Loading and failed: the feed has its own block (3.x); the friends page shows what it has. No friends: above. A ban: the Start page keeps the first row. A mute: the banner row at the top of Home (Restrictions below).
  - *Words.* Titles and captions in title case, notes in sentence case. English and Turkish in `friends_strings.rs`.

**Pages.** One screen whose pages swap in place (no slide); Back leads up a page and leaves the screen at the first page of a section. Each page opens on its first focusable row.

| Page | Title / subtitle | Rows |
|---|---|---|
| Start | "Online" / "Play with your friends over the internet" | Sign In and Create Account (commands; both open the cloud's page in the system browser, detail "Set up the cloud first." without a cloud), Cloud (action, detail "AnyPlay" or the address), note: signing in and creating an account happen in the browser, nothing goes online before |
| Cloud | "Cloud" / "The address of the cloud you sign in to" | Address (text), Use This Cloud (command; detail when the address is not https or loopback http), Use the AnyPlay Cloud (only with an own cloud), note |
| Browser | "Online" / "Continue in your browser" | Open the Page Again, Cancel, note: the launcher signs in by itself when the browser is done |
| Home | "Friends" / none (the header says who and how many) | The state of the connection when it is not up (a note; offline also Try Again), a limit, Friend Requests (up to five, each a row with the avatar, the handle, "Wants to be your friend" and Accept and Decline at its end; with more, See All Requests with "5 waiting" opens the Requests page), Invitations (a game's, then a party's), Playing Together (only once someone besides you is in the session; members with Direct, Through a relay or Connecting…), Add a Friend, Friends in the groups Playing Now (joinable first), Online and Offline (by name), each with its quick actions (above), Sent Requests |
| Friend | the handle / what they play | Join Game (only when their session is open to friends; detail "2 of 4 players"), Send Message (detail "Opening…" or "2 new"), Invite to Party (when the player leads a party or has none, and the friend is not in it), Invite to Your Game (when you are in a session they are not), View Profile, Trophies (their gamer card: the total, the grades they have below it, highest first; "None yet" or "Hidden"), Remove Friend, Block (destructive), Report |
| Requests | "Friend Requests" / "5 waiting" | Every request as the same row, none capped; the page ends by itself with the last answer |
| Request, Sent, Blocked, Invitation | the handle or "… invites you" | Accept / Decline / Block; Withdraw Request; Unblock; Join Game / Decline |
| Party | "Party" / "3 of 8 members" | Party Chat (detail "2 new") and, for the leader, Invite Friends (not full) and Take the Party to My Game (detail the game; only in an open session and with company), in that order, except that a party of one starts with Invite Friends; the Voice group (Party Voice, and while it is on the microphone, the mode and their notes, then Voice Settings); the Members (each with "Speaking", "Muted" or "Not in voice" while voice is on, "Leader" and the friend's presence in words, "You" on your own row; the others open the member page); Leave Party (destructive, its own group) |
| Party member | the handle / presence | Volume (stepper, 0 to 200 %, steps of 10) and Mute (toggle), for everyone; for the leader a group below with Make Leader (detail "Lumen will lead the party") and Remove from Party (destructive; detail "Lumen is taken out of the party") |
| Voice | "Voice" / "Talk with your party" | Party Voice (toggle; detail the state), a note when the net service switched it off, Mode (Push to Talk, Voice Activity) with its note, Talk Key (keys row; only in push-to-talk mode), a group with Microphone and Speakers (actions; detail the device or "System Default"), Refresh Devices, a note when the PC has no microphone or sound at all |
| Voice devices | "Microphone" or "Speakers" / "Choose the microphone" | System Default first, then every device by name; the chosen one says "In use", or "This device is not connected right now." when it is gone; a note that a change restarts voice for a moment. Choosing leads back to the voice page |
| Invite Friends | "Invite Friends" / "Pick up to 5 friends" | one toggle per online friend not in the party, with their presence; a pick beyond the free places is refused; Send Invitations (detail "Pick a friend first" or "The party is full" while it cannot send), Cancel. The party exists once the cloud answers |
| Party invitation | "… invites you" / "Party of 3" | Join Party, Decline; while the player has a party of their own Join Party is not offered, a note says to leave it first, and the page opens on Decline |
| Messages | "Messages" / "5 unread" or "Talk to your friends" | Message Requests (a group of its own: the sender, "Wants to message you"), then Conversations: unread first, then the newest; a friend by name with "n new" or their presence, a group by name or members, the party's chat as "Party Chat"; with none, a note on how to start |
| Conversation | the friend, the group, or "Party Chat" / presence, "n people", "3 of 8 members" | Show Earlier Messages (while there are more), the messages oldest first and the newest just above the field (title the text on one line, detail "Lumen · 11:41", "Oct 21, 19:36" for another day, "You · …" and "Seen" on your last one a friend read; in a group or the party's chat the sender comes first in the title, "Lumen: …", and the player's own words keep "You" in the detail), "Loading…" until the first fetch came back, the Message field (wide, Enter sends), Send; a group adds Leave Conversation. It opens on the field |
| Message | the sender / the time | the whole text, Copy Text, Report (not on your own messages; Back leads here from the report) |
| Message request | the sender / "Wants to message you" | their first messages (at most five), Accept, Decline, Block (destructive), a note: they cannot see that you read this, links do not open. It opens on the first message: read before answering |
| Restriction | the restriction's title ("Messages and Friend Requests Paused", "New Account Limits", "Account Banned") / "Until Oct 25, 2026 at 9:14 PM" or "Until further notice" | What This Means (one note), Reason (the rule, then "Note From the Moderator" and the moderator's words as plain text), Appeal. A mute with an appeal still open for filing: a note with the last day, the field "Why do you think this is a mistake?" (wide) and Send Appeal; an appeal that was sent, decided or out of time is one note instead. A ban: Appeal in Browser and its note. A new account's limit has no reason and no appeal. The page ends by itself when the restriction does (back to where the player came from) |
| Add a Friend, Report, Account | | the name field and Send Request (while a restriction stops friend requests, one row "Friend Requests Paused" with the end in its detail stands in their place and opens the Restriction page); the reason choice and Send Report; Manage Account (opens the account web: e-mail, passkeys, devices, deleting the account), cloud, Hide Your IP Address (toggle) with its note, Who Sees Your Trophies (Friends, Everyone, Only You) with its note, the blocked, Sign Out. A **Restrictions** group opens Home and Account whenever one is in force: a row for each, with its end as the detail |
| Invitation picker | "Invite to …" / "Pick up to … friends" | one toggle per online friend, Send Invitations, Cancel |
| Feed | "Feed" / "Recent activity from you and your friends" | cards, newest first (below); or one note while it loads, is empty, offline or failed (Try Again; Add a Friend for a player without friends) |

A text row can be **wide** (`Row::wide`): its field takes the whole row, written from the left, outlined (1 dp, 22 % of the row's ink) so it does not pass for one more row, and the title is its hint while it is empty (62 % of the row's ink). Enter ends the editing and runs the row (`Change::Submit`): a message goes out, a friend request is sent. A conversation keeps its draft per conversation in the core, so a refused message, a page change or a closed panel loses nothing.

Rows keep their identity by their `RowId` and their place among rows of the same id, not by their place in the list, and a conversation or a request keeps the focus on the same row when others come in above it (`Anchor`: a message by its number, any other row by its distance from the end). A message that arrives while the player types leaves the caret in the field, and the first fetch of a conversation does not move the focus off it.

Hints that explain a text field or a toggle stand under it with the normal 8 dp row gap, not attached: the focused row grows 3 dp and would touch an attached note. A command waiting for the net service says "Sending…" in its detail line.

**Profile screen** (`screens::profile`, its host `app::panels::profile`, the return path `app::profile`; network `spec/cloud-api.md` Profiles, ADR 0024). A full-screen overlay (`Overlay::Profile`) for a player's profile, the player's own or another's, on the frame of the trophy list: glass, 90 dp of side padding, the hint bar at the top right. It replaced the profile page of the side panel, whose banner is gone. `profile` answers a `ProfilePage`: the parts the player may see (`about`, `favourites`, `avatar`, `playtime`), the trophy counts (null when hidden) and the linked accounts. What is shown is decided in `screens::profile::view`, a function from what the launcher knows to plain data (`ProfileView`), and tested without a window; the painter decides nothing.
  - *Entry and Back.* The profile button of the top bar (own, once signed in), Your Profile in the Friends screen's rail, View Profile on a friend's page, and a card of the feed (the account of the card). Back leaves for where the player came from (`app::profile::Back`): Home for the button; for the others the Friends screen opens again on the page it was left on, with the focus on the row it was on. Signed out, the button keeps its old meaning and opens sign-in. On the player's own profile the buttons under the name are Edit Profile (opens the account web, as before; the one prominent button), Friends and, while the cloud answers, **Appear Offline** (Appear Online while it is on; the same button both ways, `Press::AppearOffline`) (the Friends screen on its first page). On a friend's: Friend Options (their page in the panel: Join Game, Message, Invite, Remove, Block, Report). On someone who asked: Answer Request. On a stranger: Add Friend. Entering through a profile never closes the way back to anything the old page offered.
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

**Trophy compare** (`screens::compare`, view `screens::compare::view`, host `app::panels::compare`, state `app::compare`; network `spec/cloud-api.md` Comparing, ADR 0029; control command `compare`). A full-screen overlay (`Overlay::Compare`) on the frame of the trophy list: the game's art and name (36 light) with "Compare Trophies" over it and "With Lumen" under it; on the right two heads over two 236 dp columns, each with avatar and name ("You" or the handle), "Level 9 · 915 points" with the track to the next level, the game's trophies by grade, and "130 points in this game"; then one row per trophy (picture 52 dp, name, "Gold · what it asks for") with, in each column, a check and the day it was earned or "Not yet". Everything numeric is the cloud's. **Names, texts and pictures come only from the player's own trophy file**; for a game that is not in the library the rows read "Trophy 4", the grade, and one quiet line says why. A hidden trophy the player has not earned is a secret in both columns ("Hidden Trophy", a lock, the Show pill on the focused row; select or Show reveals it and sends `reveal_trophies`, as on the trophy list); "Always Show Hidden Trophies" shows it. The other column is what that player's profile shows the player, so their secrets are not in it. States, one block of words under the header: loading, offline ("You are offline"), failed (Try Again, the one focus stop), hidden ("Their trophies are hidden"), empty ("Nothing to compare yet"). Up and down walk the rows; a row at the top or bottom edge of the list fades out as the edge reaches it (opaque while it rests inside the margin, gone when the cut reaches its text), so no line is cut half way under the heads. Back and Options return to the profile on the row it was left from (the cue is "back", 7.6). English and Turkish in `compare_strings.rs`.

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
  - *Read.* Looking at a conversation page marks its newest held number read, in the launcher at once and in the cloud with `mark_read`. Writing reads up to the message written. A message request is not marked. The unread mark counts `last - read` of each conversation and each request; the Messages row, the Messages page and the button show that one number.
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
  - *Ringing.* A call invitation does not toast. The window comes forward and a card hangs at the top centre, below the toast: the caller's avatar (the accent gradient and the initial, like the profile button), the handle at button size, "is calling you" in a note, and two buttons 12 dp apart, Decline and Accept, Accept the prominent one at the trailing edge. The card comes in and goes out with the overlay spring (fade and 24 dp of slide); nothing on it moves while it stays: no pulse, no countdown ring. A soft ring (Cue::Ring, two stacked fifths at F4 and C5, low and warm) plays at once and every 3.2 s while the card stays, through the sound settings like every cue. The card is for the pointer and for Tab (each button shows the focus ring); the pad and the keyboard answer from the Invitations row of the Online panel. A call that is not answered in 30 s leaves the card and becomes a missed call.
  - *In a call.* The same place holds a strip while the player is on a call: "In call" with the others' handles under it, a handle followed by "· speaking" and set in brighter ink while the member speaks, "You are muted" or "You are deafened" after the title; Mute, Deafen and Leave Call as icon buttons (the microphone and the headphones turn to their slashed forms when on, the sign-out glyph is destructive; each has its name for assistive technology). Deafen silences the microphone as well, so Mute is disabled while deafened. Per-person volume is on the member page of the party. There is no level meter and no waveform; who speaks is said in words.
  - *Mode in a call.* Push to Talk is the default, and a call that asks for a key makes the first words hard. A call (calling or connected) therefore talks with Voice Activity while it lasts, unless the player chose Push to Talk themselves (`Voice::mode_in_use`; the setting in the preferences is never touched). The mode goes to the net service when the call starts and the setting comes back when it ends (`Online::sync_voice_mode`, after every line and after Call, Accept and Hang Up); a key held at that moment is let go. Ordinary parties keep the setting. The strip says it honestly: the title reads "In call · Voice Activity" while the call overrides the setting (a mute or deafen takes that place first), and the Mode note on the party and voice pages reads "On a call, the microphone opens whenever you speak. Push to Talk is back when the call ends." The Mode row itself keeps showing the setting.\n  - *Missed calls.* A friend's row says "Missed call · 5 min ago" in place of their presence until the player has looked at the friend's page (`calls_seen`) or called them; a toast says "Missed call from Lumen" when one arrives.
  - *Blocks, mutes, new accounts.* The cloud applies the rules of messages: only friends ring, a block in either direction stops it, a muted account cannot call (the refusal's toast is the usual restriction text), calls are rate-limited, and a friend who is in a party is not rung.
  - *Voice on Discord.* Voice On (AnyPlay or Discord) is the first row of the voice rows on the party and voice pages and is kept in the preferences. On Discord, AnyPlay never opens the microphone for a party or a call: the toggle, the microphone, the mode and the key are replaced by "Voice on Discord", a note, and, when the party has a link, Open in Discord. The leader of the party sees a wide field for the Discord link (`discord.com/channels/…` or `discord.gg/…`, checked by the launcher before it is sent and by the cloud), Save Link and Remove Link; everyone in the party sees the link. A call still rings in Discord mode, the card saying "Your voice is set to Discord"; accepting joins the party and the strip says "Voice on Discord" with Answer on Discord when the party has a link; without one the card and strip say plainly that the voice is set to Discord. Only an address that passes `is_discord_link` is ever opened.
  - *Words.* Row titles in title case, details and notes in sentence case, no exclamation marks, no blame. "Add to Call" and "Call" are the only new verbs on a friend's page.

Gallery scenes: `incoming-call`, `incoming-call-discord`, `in-call`, `in-call-open` (the strip while a call overrides Push to Talk), `in-call-discord`, `calling`, `missed-call`, `party-discord`.

Gallery scenes: `friends-signin`, `friends-browser`, `friends`, `friends-friend`, `friends-settings`, `friends-picker`, `friends-party`, `friends-party-picker`, `friends-messages`, `friends-conversation`, `friends-message`, `friends-request`, `friends-party-voice`, `friends-party-member`, `friends-voice`, `friends-voice-key`, `friends-voice-devices`, `friends-voice-denied`, and `home-messages` (the profile button with unread messages), `home-friends`, `home-friends-quiet`, `home-friends-focus` and `home-signin-focus` (the top bar's pills).

**Share screen** (network `spec/control-protocol.md`, "Share screen"). Watch only, like a Go Live: a player shows the game they play to their party and members choose to watch; no input crosses. The share worker, a process the net service starts, captures, encodes, decodes and draws; the launcher neither decodes nor draws video. It chooses and says in words what the `screen` event reports. `anyps5_core::online::Screen` is the state (`can_share`, `can_watch`, the stream the player shares, the offers, the stream they watch), `ScreenCommand` (`Start`, `Stop`, `Watch(account)`, `Leave`) what the player asks for (`Online::screen_send`, the commands `screen_start`, `screen_stop`, `screen_watch`, `screen_leave`), `ScreenNotice` what the player hears of. Code: `anyps5-core/src/online/screen.rs`, `screens/online/screen.rs`, `screens/online_screen_strings.rs`, `app/online_screen.rs`.
  - *The worker and the switch.* A Windows release carries the worker inside the launcher (`ANYPLAY_SHARE_EMBED`, like the net service) and writes it once per version to `Networkin`; a build without it looks for `anyplay-share` beside itself or at `ANYPLAY_SHARE`. The launcher hands the path to the net service (`screen_worker`) right after `hello` only while share screen is on: `Preferences::online_share_screen` (default off, no control in the interface), or `ANYPLAY_SHARE_SCREEN=1` for one run. Off, the net service has no worker, `can_share` and `can_watch` stay false and nothing shows. The Share Screen row sends `screen_start` with the game's window and processes (`RunHandle::window`, `RunHandle::pids`, through `Effect::ScreenStart`), so the worker captures that window and no other.
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
  - *Settings, "Phone as Controller"* (the category after Behaviour, glyph `device-mobile`). Rows: the switch (detail: "Use your phone as a controller. Only phones on your home network can connect, and each one has to be paired first."); with it on: Status ("Waiting for a phone", "Connecting to Pixel 8", "Connected to Pixel 8"), Delay in ms while connected, the pairing question when a phone asks (the title "Pair Pixel 8?" with the code as its value, the note "Pair only if your phone shows the same code.", then Pair Phone and Deny as two commands), Address with the note for manual entry (and notes when the port was busy or the PC could not announce itself), the Paired phones caption with one destructive "Remove Pixel 8" command per phone ("Paired on 2026-10-24"), and the Virtual controller status with its notes. When the server cannot start, one note says so and the switch stays.
  - *The question comes to the player.* When a phone asks to be paired while nothing else is open and no game starts or runs, the settings open on this page (the link `anyps5://settings/phone` does the same). Otherwise a toast says who asks and where to answer; the question stays up for the minute the server waits.
  - *Top bar.* While a phone is welcomed, a pill 32 dp high (the buttons' surface, white 0.10) with the `device-mobile` glyph and the phone's name (14 medium, cut in the middle past 150 dp) sits 48 dp after the title, clear of the scanning spinner. It is a status, not a button: not focusable, named "Phone connected: Pixel 8" for screen readers. It is ink on the pill, not accent: the accent is for being online with the network.
  - *Words.* English and Turkish like the rest (`screens/phone_strings.rs`). The product text names no console brand.

**Tests.** `fake-phone` (`crates/fake-phone`, never shipped) is a phone and a game in one binary: `fake-phone phone --addr 127.0.0.1:47700 --key-file k.txt --script "select:ID,launch:ID,press:cross,..."`, `fake-phone phone --discover` finds the PC through mDNS, `fake-phone game` greets on the bridge. `crates/fake-phone/tests/e2e.rs` drives the real server with it: protocol round trip, pairing and key reuse, an unpaired phone rejected, a denied phone rejected, a request flood limited. The unit tests of `remote` cover the packets (golden hex), the device file, the layouts file, the rate limiter, the on-link rule, the relay's identity checks, the mDNS adapter choice and the pad mapping. The scenes render in the gallery; `docs/media/phone-off.jpg` is a real screenshot of the off state. Gallery scenes: `phone-off`, `phone-waiting`, `phone-pairing`, `phone-connected`, `phone-driver` (Windows without ViGEmBus), `phone-access` (Linux without access to uinput), `phone-failed`, and `home-phone` (the top bar). Not tested without hardware: a real phone on a real network and a real virtual pad in a real game (any game needs the user's go).
