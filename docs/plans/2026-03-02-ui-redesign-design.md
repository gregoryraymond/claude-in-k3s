# UI Redesign: Dark Theme with Sidebar Navigation

## Context

The current UI uses Slint's `std-widgets` with minimal styling: platform-native TabWidget, GroupBox containers, and default buttons. This redesign transforms it into a polished dark-themed developer tool with a left sidebar, custom component library, and consistent visual language.

## Design Decisions

- **Dark theme**: Deep navy/charcoal palette with purple accent (`#6c63ff`)
- **Left sidebar**: Replaces TabWidget for a more modern, spacious feel
- **Fully custom components**: Replace std-widgets GroupBox/Button/TabWidget with themed alternatives
- **Slint-native**: No external assets, no SVGs, uses Unicode characters for icons

## Color System

File: `ui/theme.slint` — global singleton

| Token | Value | Usage |
|-------|-------|-------|
| bg | `#0f0f1a` | Window/page background |
| surface | `#1a1a2e` | Cards, panels |
| surface-alt | `#16162a` | Sidebar, alternating rows |
| border | `#2a2a3e` | Card borders, dividers |
| accent | `#6c63ff` | Primary actions, active states |
| accent-hover | `#7c73ff` | Hover on accent elements |
| text | `#e0e0e8` | Primary text |
| text-muted | `#8888a0` | Secondary/helper text |
| success | `#4ade80` | Running, Healthy, Found |
| warning | `#fbbf24` | Pending, Starting |
| error | `#f87171` | Failed, Missing, Error |
| log-bg | `#0a0a14` | Log viewer background |
| log-text | `#a0e8a0` | Log viewer text (green tint) |

Spacing: `4px`, `8px`, `12px`, `16px`, `24px`
Border-radius: `6px` (buttons), `8px` (cards), `10px` (badges)
Font sizes: `11px` (caption), `13px` (body), `15px` (subtitle), `20px` (title), `24px` (page header)

## File Structure

```
ui/
  theme.slint                    -- Global color/spacing/typography tokens
  components/
    card.slint                   -- Rounded container with border
    icon-button.slint            -- Styled button with hover/press/disabled
    status-badge.slint           -- Colored pill for status display
    form-field.slint             -- Label-above-input pattern
    nav-item.slint               -- Sidebar navigation link with active state
    log-viewer.slint             -- Terminal-style monospace log display
    data-row.slint               -- Table row with alternating backgrounds
    setup-panel.slint            -- Dependency check/install (redesigned)
    cluster-panel.slint          -- Terraform/Helm controls (redesigned)
    projects-panel.slint         -- Project management (redesigned)
    pods-panel.slint             -- Pod table + Claude prompt (redesigned)
  app-window.slint               -- Sidebar layout + content routing
```

## Layout

```
+----------+-------------------------------------------+
|          |  Page Title            ● Cluster Status    |  <- content header
|  Logo    +-------------------------------------------+
|----------|                                            |
| ▶ Cluster|                                            |
|   Project|         Content area                       |
|   Pods   |         (panels swapped by active-page)    |
|   ────── |                                            |
|   ⚙ Set  |                                            |
|          |                                            |
+----------+-------------------------------------------+
```

- **Sidebar**: 180px wide, `surface-alt` background
- **Active nav item**: 3px left accent bar + `#1f1f35` background
- **Content area**: `bg` background, scrollable
- **No separate status bar** — status info moves into the content header

Navigation uses `in-out property <int> active-page` with values:
- 0 = Cluster
- 1 = Projects
- 2 = Pods
- 3 = Settings

Setup panel shown conditionally when `!all-deps-met` (overrides normal navigation).

## Component Specifications

### Card (`card.slint`)

Replaces `GroupBox`. Rounded container with optional title.

```
component Card inherits Rectangle {
    in property <string> title: "";
    background: Theme.surface;
    border-width: 1px;
    border-color: Theme.border;
    border-radius: 8px;
    clip: true;
    // Title rendered as Text at top if non-empty
    // Content via @children
}
```

### IconButton (`icon-button.slint`)

Replaces `Button`. Custom styled with hover/press states.

```
component IconButton inherits Rectangle {
    in property <string> label: "";
    in property <bool> enabled: true;
    in property <bool> primary: false;     // accent colored
    in property <bool> destructive: false;  // error colored on hover
    callback clicked();

    // States:
    // Default:     bg surface-alt, text primary
    // Hover:       bg accent (or error if destructive)
    // Pressed:     slightly darker than hover
    // Disabled:    opacity 0.4
    // Primary:     bg accent always, lighter on hover

    // Animation: background 150ms ease-in-out
}
```

### StatusBadge (`status-badge.slint`)

Colored pill showing status text.

```
component StatusBadge inherits Rectangle {
    in property <string> status: "";
    // Auto-colors based on status text:
    // "Running"/"Healthy"/"Found" -> success bg
    // "Pending"/"Starting"        -> warning bg
    // "Failed"/"Error"/"Missing"  -> error bg
    // Other                       -> surface-alt bg
    border-radius: 10px;
    height: 22px;
    // White text, 11px, bold
}
```

### FormField (`form-field.slint`)

Label-above-input, replaces GroupBox + HorizontalBox pattern.

```
component FormField inherits VerticalLayout {
    in property <string> label: "";
    in property <string> value: "";
    in property <string> placeholder: "";
    in property <bool> is-password: false;
    in-out property <string> text: "";
    // Muted label text (13px) above
    // Dark-styled LineEdit below (surface bg, border, border-radius 6px)
}
```

### NavItem (`nav-item.slint`)

Sidebar navigation link.

```
component NavItem inherits Rectangle {
    in property <string> icon: "";     // Unicode character
    in property <string> label: "";
    in property <bool> active: false;
    callback clicked();

    height: 44px;
    // Active: 3px left accent bar, bg #1f1f35
    // Hover: bg #1a1a30
    // Icon + label in HorizontalLayout
    // Animation: background 120ms
}
```

### LogViewer (`log-viewer.slint`)

Terminal-style log display.

```
component LogViewer inherits Rectangle {
    in property <string> text: "";
    background: Theme.log-bg;
    border-radius: 6px;
    border-width: 1px;
    border-color: Theme.border;
    // Inner ScrollView with monospace Text
    // Green-tinted text (#a0e8a0), 12px
}
```

### DataRow (`data-row.slint`)

Table row with alternating background and hover.

```
component DataRow inherits Rectangle {
    in property <bool> alternate: false;
    // Default: transparent or surface-alt if alternate
    // Hover: #222240
    // Contains @children for flexible cell layout
    height: 40px;
}
```

## Panel Redesigns

### SetupPanel

- Page title "Setup" at top
- Subtitle "The following tools are required"
- 4 dependency items, each in its own row:
  - StatusBadge (Found/Missing) + tool name + version text
- "Install Missing" as primary IconButton, "Continue" as default IconButton
- LogViewer for install output

### ClusterPanel

- Page title "Cluster" + large StatusBadge for cluster status
- **Terraform card**: Title "Terraform Lifecycle", row of IconButtons (Init, Apply, Plan, Destroy)
  - Destroy button uses `destructive: true`
- **Helm card**: Title "Helm Release", Status IconButton
- **LogViewer**: Full-width below cards

### ProjectsPanel

- Page title "Projects"
- Folder path display with Browse + Refresh IconButtons
- Card containing project list:
  - DataRows with checkbox, project name, image ComboBox, dockerfile indicator
- Bottom: "Launch Selected" (primary) + "Stop Selected" (destructive) IconButtons

### PodsPanel

- Page title "Pods" + Refresh IconButton
- Table header row (muted text)
- DataRows for each pod:
  - Project name, StatusBadge for phase, ready indicator, restarts, age
  - Action IconButtons: Logs, Claude, Delete (destructive)
- Claude prompt section at bottom (when target pod selected):
  - Input field + Send (primary) IconButton
- Summary text at bottom

### Settings

- Page title "Settings"
- Cards grouping related settings:
  - **API Key card**: FormField with password input
  - **Claude Mode card**: ComboBox (keep std-widget, just restyle surroundings)
  - **Git Configuration card**: Two FormFields
  - **Resource Limits card**: Two FormFields
  - **Directories card**: Two FormFields
  - **Platform Info card**: Read-only display
- "Save Settings" as primary IconButton

## What We Keep from std-widgets

- `LineEdit` — restyled via surrounding elements (Slint doesn't expose LineEdit internals easily)
- `ComboBox` — same, keep native but place in themed containers
- `CheckBox` — keep for project selection toggles
- `ScrollView` — keep for scrollable content areas

## What We Replace

| Old (std-widgets) | New (custom) |
|---|---|
| `TabWidget` + `Tab` | Sidebar with `NavItem` + conditional panels |
| `GroupBox` | `Card` |
| `Button` | `IconButton` |
| `TextEdit` (read-only log) | `LogViewer` |
| Flat text status | `StatusBadge` |
| Label + LineEdit rows | `FormField` |
| Plain table rows | `DataRow` |

## Rust Code Changes

Minimal changes needed in `src/main.rs`:
- Replace `TabWidget` tab index management with `active-page` property
- Set `active-page` from Rust or let UI handle it internally
- All existing callbacks and properties remain the same
- No changes to any other Rust files

## Verification

1. `cargo build` — compiles without Slint errors
2. Run the app — visual inspection of:
   - Dark theme applied consistently
   - Sidebar navigation switches panels correctly
   - All buttons/actions still work (terraform, helm, pods, etc.)
   - Setup panel still shows when deps are missing
   - Hover states animate smoothly
3. `cargo test` — all existing tests still pass (UI is purely visual, no logic changes)
