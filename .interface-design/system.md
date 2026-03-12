# Claude in K3s — Design System

## Direction: Forge

Warm industrial. A well-lit workshop where things get built.
The user is a developer operating AI coding agents in Kubernetes pods.
This is a power tool used during active development — fast decisions, status at a glance, act immediately.

## Palette

All surfaces share the same warm hue, shifting only in lightness.

| Token | Hex | Role |
|-------|-----|------|
| `bg` | `#121110` | Foundry floor — deepest surface |
| `surface` | `#1c1a18` | Workbench — card/panel surface |
| `surface-alt` | `#181614` | Anvil — sidebar, alternating rows |
| `border` | `#2e2a26` | Seam — structural separation |
| `accent` | `#d97706` | Copper — primary action, brand |
| `accent-hover` | `#e58a08` | Copper bright |
| `accent-press` | `#b96604` | Copper dark |
| `text` | `#d4d0cc` | Steel — primary text |
| `text-muted` | `#8a8480` | Patina — secondary text |
| `success` | `#4ade80` | Operational — healthy systems |
| `warning` | `#fbbf24` | Heat — attention needed |
| `error` | `#ef4444` | Quench — stop, fault, danger |
| `log-bg` | `#0e0d0c` | Crucible — darkest, log background |
| `log-text` | `#c4a882` | Ember glow — warm log readout |

## Depth Strategy

Subtle shadows + borders. Cards use `drop-shadow-blur: 8px` with warm shadow `#0a090830`.
Borders at `1px` with seam color. No dramatic elevation jumps.

## Spacing

4px base grid. Scale: 4, 8, 12, 16, 20, 24, 32.

## Typography

System default (Slint constraint). Hierarchy through weight and size:
- Page titles: 24px / 700
- Card titles: 15px / 600
- Body: 13px / 400
- Labels: 12px / 500
- Metadata: 11px / 400

## Component Patterns

### Card
- `surface` background, `border` 1px, 8px radius
- Subtle drop shadow (8px blur, 2px y-offset, warm)
- 16px padding, 12px internal spacing

### IconButton
- 6px radius, 150ms background transition
- States: default (`surface-alt`), hover (`accent`), pressed (`border`)
- Primary: copper accent with white text/icon
- Active: green-tinted background, success-colored icon
- Destructive: red-tinted background at rest, full red on hover

### StatusBadge
- Pill shape (10px radius), 22px height
- Tinted background matching semantic color (warm variants)
- Border 1px matching semantic but darker

### NavItem
- 44px height, 3px copper left indicator when active
- Active icon colorized to accent, text to primary
- Hover background shift, 120ms transition

### Sidebar
- Same hue family as main content (surface-alt), separated by 1px border
- NOT a different color world — warmth stays consistent

### Tooltip
- Surface background with border, subtle warm shadow
- Positioned below trigger, 11px text

## Semantic Status Colors (badge backgrounds)

| Status | Background | Border |
|--------|-----------|--------|
| Healthy/Running | `#1a2b1e` | `#2a5a3a` |
| Pending/Warning | `#2b2212` | `#5a4418` |
| Error/Failed | `#2b1612` | `#5a2820` |

## What NOT to do

- No cool-toned blues or purples — everything warm
- No dramatic shadows — whisper-quiet depth
- No different hue for sidebar vs content
- No hardcoded colors outside theme.slint
