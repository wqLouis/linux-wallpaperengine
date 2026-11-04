# The design language of the interface

The interface of \`lwe-gui\` is one thing, not a collection of screens.  Three
rules hold everywhere, and the module \`crates/lwe-gui/src/design.rs\` is their
implementation.

---

## 1. One scale

Every padding, margin, gap and size comes from five numbers.  A level says
what an element is, and the number of the level is its measurement.

| Level | Number | What it is |
|-------|--------|------------|
| Inline | 16 | a control: a button, a chip, the gap inside a row |
| Card | 24 | a card: a wallpaper of the list, the gap between two cards |
| Panel | 32 | a panel: the header, the status bar, the settings group |
| Window | 48 | the window: the padding around everything |
| Hero | 64 | the largest element of the interface |

Nothing else is measured: there is no \`gap_3\`, no \`px(13.)\`, no \`mt_2\`.
If a number is not in the table, it does not belong in the interface.

---

## 2. Depth, not borders

A surface has **the colour of the background**.  What makes it visible are
two shadows: the light one that falls from the upper left and the dark one
that falls from the lower right.  A surface that is pressed into the
background has the two shadows the other way round.

| Elevation | Where | Light shadow | Dark shadow |
|-----------|-------|--------------|-------------|
| Flat | the background | - | - |
| Raised | a control | -2, -2, blur 8 | 2, 2, blur 8 |
| Card | a wallpaper of the list | -4, -4, blur 12 | 4, 4, blur 12 |
| Floating | the wallpaper that runs | -8, -8, blur 24 | 8, 8, blur 24 |
| Inset | the wallpaper that is selected, a control that is on | 2, 2, blur 8 | -2, -2, blur 8 |

There are no borders and no separators: a line between two things is a
shadow or a gap, never a rule.

---

## 3. Concentric corners

The radius of an element is **the radius of the element around it plus the
gap that separates the two**.  The corners of the two follow each other
instead of crossing.

The gap is the padding of the container.  Worked out, from the outside in:

```text
the window                 radius  0     the window manager rounds the window
the content area           radius  0     it is the background itself
  a card in it             radius 48     0 + the padding of the content area (48)
    a control in the card  radius 72     48 + the padding of the card (24)
```

A radius bigger than half of the element would turn it into a circle, so
the window clamps it: the control above is 32 pixels high, so its radius
becomes 16 and it is a pill, not a disc.  The rule is never broken, only
clamped.

The radius is therefore **calculated, never chosen**:

```rust
let card_radius = nested_radius(content_radius, Level::Window.size());
let button_radius = nested_radius(card_radius, Level::Card.size());
```

---

## 4. The colours

| | Light | Dark |
|---|-------|------|
| background | hsl(214, 32%, 91%) | hsl(216, 16%, 19%) |
| text | hsl(218, 28%, 22%) | hsl(214, 26%, 90%) |
| muted text | hsl(216, 16%, 52%) | hsl(215, 12%, 62%) |
| accent | hsl(226, 84%, 62%) | hsl(226, 90%, 74%) |
| light shadow | white 85% | white 6% |
| dark shadow | hsl(213, 32%, 66%) 55% | black 45% |

The accent is used **once**: for the wallpaper that runs.  Everything else
is the background, the text and the shadows.

---

## 5. The pages

The application has **one window and two pages**.  There is no sidebar:
the window is a list, and the settings are a page of the same window.

```text
┌────────────────────────────────────────────────┐
│  Wallpapers                              [Settings] │   the header: a title, one button
│                                                │
│   ┌────────────────────────────────────────┐    │
│   │ [preview]  Aurora              Scene  │    │   a card: raised, the selected one inset
│   │ [preview]  Zen Garden          Scene  │    │
│   └────────────────────────────────────────┘    │
│                                                │
│  Aurora is running · 2 problems                │   the status line: text, no panel
└────────────────────────────────────────────────┘

The settings page holds the same list of rows, each row a label and a
control:

```text
┌────────────────────────────────────────────────┐
│  Settings                                [Back] │
│                                                │
│  Wallpapers                                   │
│  ~/steamapps/workshop/content/431960  [Choose…] │
│                                                │
│  Fit                                          │
│  [ Cover ][ Contain ][ Stretch ]               │
│                                                │
│  Frame rate            60 fps                 │
│  ──────────────●────────────────               │
│                                                │
│  Effects                              [ on ]    │
│  Puppets                              [ off ]   │
└────────────────────────────────────────────────┘
```

---

## 6. What the language does not do

Restraint is part of the design.  The interface has:

* a background, raised surfaces and one inset surface (the selection),
* text in two weights (the title and the muted text),
* three kinds of control: a button, a segmented choice, a toggle.

It has **no** icons, no badges, no coloured pills, no gradients, no
decorative frames, no drop shadows that are not part of an elevation and
no element that does not carry information.  A control exists because the
user can do something with it.

---

## See also

* [overview.md](overview.md) — the layout of the workspace and the data flow
* [engine.md](engine.md) — the façade of the library