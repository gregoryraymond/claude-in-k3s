# UI Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Transform the plain std-widgets UI into a polished dark-themed developer tool with left sidebar navigation and custom component library.

**Architecture:** Create a `theme.slint` global singleton for color tokens, build 7 reusable custom components (Card, IconButton, StatusBadge, FormField, NavItem, LogViewer, DataRow), replace the TabWidget with sidebar navigation using `active-page` integer routing, and rewrite all 4 panels to use the new components. The Rust API surface (properties, callbacks, structs) stays identical — only visuals change.

**Tech Stack:** Slint 1.x (`.slint` declarative UI), Rust (minimal `main.rs` update), existing `cargo test` suite.

**Design doc:** `docs/plans/2026-03-02-ui-redesign-design.md`

---

### Task 1: Create theme.slint with global color tokens

**Files:**
- Create: `ui/theme.slint`

**Step 1: Create the theme file**

```slint
// ui/theme.slint
export global Theme {
    // Backgrounds
    out property <color> bg:          #0f0f1a;
    out property <color> surface:     #1a1a2e;
    out property <color> surface-alt: #16162a;
    out property <color> border:      #2a2a3e;

    // Accent
    out property <color> accent:       #6c63ff;
    out property <color> accent-hover: #7c73ff;
    out property <color> accent-press: #5b52ee;

    // Text
    out property <color> text:       #e0e0e8;
    out property <color> text-muted: #8888a0;

    // Semantic
    out property <color> success: #4ade80;
    out property <color> warning: #fbbf24;
    out property <color> error:   #f87171;

    // Log viewer
    out property <color> log-bg:   #0a0a14;
    out property <color> log-text: #a0e8a0;

    // Sidebar active
    out property <color> nav-active-bg: #1f1f35;
    out property <color> nav-hover-bg:  #1a1a30;
}
```

**Step 2: Verify it compiles**

Temporarily add `import { Theme } from "theme.slint";` to the top of `ui/app-window.slint` (after existing imports) and run:

```bash
cargo build 2>&1 | head -5
```

Expected: compiles successfully (or only pre-existing warnings).

Remove the temporary import line after verifying.

**Step 3: Commit**

```bash
git add ui/theme.slint
git commit -m "feat(ui): add theme.slint with dark color tokens"
```

---

### Task 2: Create Card component

**Files:**
- Create: `ui/components/card.slint`

**Step 1: Create the component**

```slint
// ui/components/card.slint
import { Theme } from "../theme.slint";

export component Card inherits Rectangle {
    in property <string> title: "";

    background: Theme.surface;
    border-width: 1px;
    border-color: Theme.border;
    border-radius: 8px;
    clip: true;

    VerticalLayout {
        padding: 16px;
        spacing: 12px;

        if title != "" : Text {
            text: title;
            font-size: 15px;
            font-weight: 600;
            color: Theme.text;
        }

        @children
    }
}
```

**Step 2: Verify it compiles**

Add temporarily to `app-window.slint`:
```slint
import { Card } from "components/card.slint";
```

Run `cargo build`. Remove temporary import.

**Step 3: Commit**

```bash
git add ui/components/card.slint
git commit -m "feat(ui): add Card component"
```

---

### Task 3: Create IconButton component

**Files:**
- Create: `ui/components/icon-button.slint`

**Step 1: Create the component**

```slint
// ui/components/icon-button.slint
import { Theme } from "../theme.slint";

export component IconButton inherits Rectangle {
    in property <string> label: "";
    in property <bool> enabled: true;
    in property <bool> primary: false;
    in property <bool> destructive: false;
    callback clicked();

    private property <bool> hovered: false;
    private property <bool> pressed: false;

    height: 34px;
    min-width: 72px;
    horizontal-stretch: 0;
    border-radius: 6px;
    opacity: enabled ? 1.0 : 0.4;

    background: {
        if !enabled { return Theme.surface-alt; }
        if primary {
            if pressed { return Theme.accent-press; }
            if hovered { return Theme.accent-hover; }
            return Theme.accent;
        }
        if destructive && (hovered || pressed) {
            if pressed { return #d94040; }
            return Theme.error;
        }
        if pressed { return Theme.border; }
        if hovered { return Theme.accent; }
        return Theme.surface-alt;
    };

    animate background { duration: 150ms; easing: ease-in-out; }

    HorizontalLayout {
        padding-left: 14px;
        padding-right: 14px;
        alignment: center;

        Text {
            text: root.label;
            font-size: 13px;
            font-weight: 500;
            color: {
                if !root.enabled { return Theme.text-muted; }
                if root.primary || root.hovered || root.pressed { return #ffffff; }
                return Theme.text;
            };
            vertical-alignment: center;
            horizontal-alignment: center;
        }
    }

    TouchArea {
        mouse-cursor: root.enabled ? pointer : default;
        clicked => {
            if root.enabled { root.clicked(); }
        }
        hovered => {
            root.hovered = self.has-hover && root.enabled;
        }
        pointer-event(event) => {
            root.pressed = self.pressed && root.enabled;
        }
    }
}
```

**Step 2: Verify it compiles** (temporary import in app-window, then remove)

**Step 3: Commit**

```bash
git add ui/components/icon-button.slint
git commit -m "feat(ui): add IconButton component with hover/press states"
```

---

### Task 4: Create StatusBadge component

**Files:**
- Create: `ui/components/status-badge.slint`

**Step 1: Create the component**

```slint
// ui/components/status-badge.slint
import { Theme } from "../theme.slint";

export component StatusBadge inherits Rectangle {
    in property <string> status: "";

    height: 22px;
    min-width: 64px;
    horizontal-stretch: 0;
    border-radius: 10px;

    background: {
        if status == "Running" || status == "Healthy" || status == "Found" { return Theme.success; }
        if status == "Pending" || status == "Starting" { return Theme.warning; }
        if status == "Failed" || status == "Error" || status == "Missing" { return Theme.error; }
        return Theme.surface-alt;
    };

    HorizontalLayout {
        padding-left: 10px;
        padding-right: 10px;
        alignment: center;

        Text {
            text: root.status;
            font-size: 11px;
            font-weight: 700;
            color: {
                if root.status == "Pending" || root.status == "Starting" { return #1a1a2e; }
                return #ffffff;
            };
            vertical-alignment: center;
            horizontal-alignment: center;
        }
    }
}
```

**Step 2: Verify it compiles**

**Step 3: Commit**

```bash
git add ui/components/status-badge.slint
git commit -m "feat(ui): add StatusBadge component"
```

---

### Task 5: Create FormField component

**Files:**
- Create: `ui/components/form-field.slint`

**Step 1: Create the component**

```slint
// ui/components/form-field.slint
import { Theme } from "../theme.slint";
import { LineEdit } from "std-widgets.slint";

export component FormField inherits VerticalLayout {
    in property <string> label: "";
    in property <string> placeholder: "";
    in property <bool> is-password: false;
    in property <bool> read-only: false;
    in-out property <string> text: "";

    spacing: 4px;

    Text {
        text: root.label;
        font-size: 12px;
        font-weight: 500;
        color: Theme.text-muted;
    }

    LineEdit {
        text <=> root.text;
        placeholder-text: root.placeholder;
        input-type: root.is-password ? password : text;
        read-only: root.read-only;
        font-size: 13px;
    }
}
```

**Step 2: Verify it compiles**

**Step 3: Commit**

```bash
git add ui/components/form-field.slint
git commit -m "feat(ui): add FormField component"
```

---

### Task 6: Create NavItem component

**Files:**
- Create: `ui/components/nav-item.slint`

**Step 1: Create the component**

```slint
// ui/components/nav-item.slint
import { Theme } from "../theme.slint";

export component NavItem inherits Rectangle {
    in property <string> icon: "";
    in property <string> label: "";
    in property <bool> active: false;
    callback clicked();

    private property <bool> hovered: false;

    height: 44px;

    background: {
        if active { return Theme.nav-active-bg; }
        if hovered { return Theme.nav-hover-bg; }
        return transparent;
    };

    animate background { duration: 120ms; easing: ease-in-out; }

    // Active accent bar on the left
    Rectangle {
        x: 0px;
        y: 0px;
        width: active ? 3px : 0px;
        height: parent.height;
        background: Theme.accent;
        animate width { duration: 120ms; easing: ease-in-out; }
    }

    HorizontalLayout {
        padding-left: 16px;
        padding-right: 12px;
        spacing: 10px;
        alignment: start;

        Text {
            text: root.icon;
            font-size: 16px;
            color: root.active ? Theme.accent : Theme.text-muted;
            vertical-alignment: center;
            width: 24px;
            horizontal-alignment: center;
        }

        Text {
            text: root.label;
            font-size: 13px;
            font-weight: root.active ? 600 : 400;
            color: root.active ? Theme.text : Theme.text-muted;
            vertical-alignment: center;
        }
    }

    TouchArea {
        mouse-cursor: pointer;
        clicked => { root.clicked(); }
        moved => { root.hovered = self.has-hover; }
    }
}
```

**Step 2: Verify it compiles**

**Step 3: Commit**

```bash
git add ui/components/nav-item.slint
git commit -m "feat(ui): add NavItem sidebar component"
```

---

### Task 7: Create LogViewer component

**Files:**
- Create: `ui/components/log-viewer.slint`

**Step 1: Create the component**

```slint
// ui/components/log-viewer.slint
import { Theme } from "../theme.slint";
import { ScrollView } from "std-widgets.slint";

export component LogViewer inherits Rectangle {
    in property <string> text: "";

    background: Theme.log-bg;
    border-radius: 6px;
    border-width: 1px;
    border-color: Theme.border;
    min-height: 150px;

    ScrollView {
        VerticalLayout {
            padding: 12px;

            Text {
                text: root.text;
                font-size: 12px;
                color: Theme.log-text;
                wrap: word-wrap;
                // Slint doesn't have a monospace font property on Text,
                // but the default fixed-width rendering works reasonably.
            }
        }
    }
}
```

**Step 2: Verify it compiles**

**Step 3: Commit**

```bash
git add ui/components/log-viewer.slint
git commit -m "feat(ui): add LogViewer terminal-style component"
```

---

### Task 8: Create DataRow component

**Files:**
- Create: `ui/components/data-row.slint`

**Step 1: Create the component**

```slint
// ui/components/data-row.slint
import { Theme } from "../theme.slint";

export component DataRow inherits Rectangle {
    in property <bool> alternate: false;
    private property <bool> hovered: false;

    height: 40px;

    background: {
        if hovered { return #222240; }
        if alternate { return Theme.surface-alt; }
        return transparent;
    };

    animate background { duration: 100ms; }

    @children

    TouchArea {
        moved => { root.hovered = self.has-hover; }
    }
}
```

**Step 2: Verify it compiles**

**Step 3: Commit**

```bash
git add ui/components/data-row.slint
git commit -m "feat(ui): add DataRow component with alternating backgrounds"
```

---

### Task 9: Rewrite SetupPanel with dark theme components

**Files:**
- Modify: `ui/components/setup-panel.slint` (full rewrite)

**Step 1: Rewrite setup-panel.slint**

Replace the entire contents of `ui/components/setup-panel.slint` with:

```slint
import { Theme } from "../theme.slint";
import { IconButton } from "icon-button.slint";
import { StatusBadge } from "status-badge.slint";
import { LogViewer } from "log-viewer.slint";
import { ScrollView } from "std-widgets.slint";

export component SetupPanel inherits Rectangle {
    in property <bool> k3s-found: false;
    in property <bool> terraform-found: false;
    in property <bool> helm-found: false;
    in property <bool> docker-found: false;
    in property <string> k3s-label: "k3s";
    in property <string> k3s-version: "";
    in property <string> terraform-version: "";
    in property <string> helm-version: "";
    in property <string> docker-version: "";
    in property <string> install-log: "";
    in property <bool> is-installing: false;

    callback install-missing();
    callback continue-app();

    background: Theme.bg;

    VerticalLayout {
        padding: 32px;
        spacing: 20px;
        alignment: start;

        Text {
            text: "Setup";
            font-size: 24px;
            font-weight: 700;
            color: Theme.text;
        }

        Text {
            text: "The following tools are required to run Claude in K3s.";
            font-size: 14px;
            color: Theme.text-muted;
        }

        // Dependency rows
        Rectangle {
            background: Theme.surface;
            border-radius: 8px;
            border-width: 1px;
            border-color: Theme.border;

            VerticalLayout {
                padding: 16px;
                spacing: 12px;

                // k3s / k3d
                HorizontalLayout {
                    spacing: 12px;
                    height: 28px;
                    alignment: start;

                    StatusBadge { status: k3s-found ? "Found" : "Missing"; }

                    Text {
                        text: k3s-label;
                        font-size: 14px;
                        font-weight: 600;
                        color: Theme.text;
                        vertical-alignment: center;
                        min-width: 90px;
                    }

                    Text {
                        text: k3s-found ? k3s-version : "Not installed";
                        font-size: 13px;
                        color: Theme.text-muted;
                        vertical-alignment: center;
                    }
                }

                Rectangle { height: 1px; background: Theme.border; }

                // terraform
                HorizontalLayout {
                    spacing: 12px;
                    height: 28px;
                    alignment: start;

                    StatusBadge { status: terraform-found ? "Found" : "Missing"; }

                    Text {
                        text: "terraform";
                        font-size: 14px;
                        font-weight: 600;
                        color: Theme.text;
                        vertical-alignment: center;
                        min-width: 90px;
                    }

                    Text {
                        text: terraform-found ? terraform-version : "Not installed";
                        font-size: 13px;
                        color: Theme.text-muted;
                        vertical-alignment: center;
                    }
                }

                Rectangle { height: 1px; background: Theme.border; }

                // helm
                HorizontalLayout {
                    spacing: 12px;
                    height: 28px;
                    alignment: start;

                    StatusBadge { status: helm-found ? "Found" : "Missing"; }

                    Text {
                        text: "helm";
                        font-size: 14px;
                        font-weight: 600;
                        color: Theme.text;
                        vertical-alignment: center;
                        min-width: 90px;
                    }

                    Text {
                        text: helm-found ? helm-version : "Not installed";
                        font-size: 13px;
                        color: Theme.text-muted;
                        vertical-alignment: center;
                    }
                }

                Rectangle { height: 1px; background: Theme.border; }

                // docker
                HorizontalLayout {
                    spacing: 12px;
                    height: 28px;
                    alignment: start;

                    StatusBadge { status: docker-found ? "Found" : "Missing"; }

                    Text {
                        text: "docker";
                        font-size: 14px;
                        font-weight: 600;
                        color: Theme.text;
                        vertical-alignment: center;
                        min-width: 90px;
                    }

                    Text {
                        text: docker-found ? docker-version : "Not installed";
                        font-size: 13px;
                        color: Theme.text-muted;
                        vertical-alignment: center;
                    }
                }
            }
        }

        // Action buttons
        HorizontalLayout {
            spacing: 12px;
            alignment: start;

            IconButton {
                label: is-installing ? "Installing..." : "Install Missing";
                primary: true;
                enabled: !is-installing && !(k3s-found && terraform-found && helm-found && docker-found);
                clicked => { root.install-missing(); }
            }

            IconButton {
                label: "Continue";
                enabled: !is-installing && k3s-found && terraform-found && helm-found && docker-found;
                clicked => { root.continue-app(); }
            }
        }

        // Install log
        LogViewer {
            text: root.install-log;
            vertical-stretch: 1;
        }
    }
}
```

**Step 2: Verify it compiles**

```bash
cargo build 2>&1 | head -10
```

Note: This will fail until app-window.slint imports the theme. That's expected — we'll fix it in Task 13.

**Step 3: Commit**

```bash
git add ui/components/setup-panel.slint
git commit -m "feat(ui): redesign SetupPanel with dark theme"
```

---

### Task 10: Rewrite ClusterPanel with dark theme components

**Files:**
- Modify: `ui/components/cluster-panel.slint` (full rewrite)

**Step 1: Rewrite cluster-panel.slint**

Replace entire contents:

```slint
import { Theme } from "../theme.slint";
import { Card } from "card.slint";
import { IconButton } from "icon-button.slint";
import { StatusBadge } from "status-badge.slint";
import { LogViewer } from "log-viewer.slint";

export component ClusterPanel inherits Rectangle {
    in property <string> cluster-status: "Unknown";
    in property <string> log-output: "";
    in property <bool> is-busy: false;
    in property <bool> tf-initialized: false;

    callback terraform-init();
    callback terraform-apply();
    callback terraform-destroy();
    callback terraform-plan();
    callback helm-status();

    background: Theme.bg;

    VerticalLayout {
        padding: 24px;
        spacing: 16px;

        // Header
        HorizontalLayout {
            spacing: 12px;
            alignment: start;

            Text {
                text: "Cluster";
                font-size: 24px;
                font-weight: 700;
                color: Theme.text;
                vertical-alignment: center;
            }

            StatusBadge {
                status: cluster-status;
            }
        }

        // Terraform card
        Card {
            title: "Terraform Lifecycle";

            HorizontalLayout {
                spacing: 8px;
                alignment: start;

                IconButton {
                    label: tf-initialized ? "Re-Init" : "Init";
                    enabled: !is-busy;
                    clicked => { root.terraform-init(); }
                }

                IconButton {
                    label: "Apply";
                    primary: true;
                    enabled: !is-busy && tf-initialized;
                    clicked => { root.terraform-apply(); }
                }

                IconButton {
                    label: "Plan";
                    enabled: !is-busy && tf-initialized;
                    clicked => { root.terraform-plan(); }
                }

                IconButton {
                    label: "Destroy";
                    destructive: true;
                    enabled: !is-busy && tf-initialized;
                    clicked => { root.terraform-destroy(); }
                }
            }
        }

        // Helm card
        Card {
            title: "Helm Release";

            HorizontalLayout {
                spacing: 8px;
                alignment: start;

                IconButton {
                    label: "Status";
                    enabled: !is-busy;
                    clicked => { root.helm-status(); }
                }
            }
        }

        // Log output
        LogViewer {
            text: root.log-output;
            vertical-stretch: 1;
        }
    }
}
```

**Step 2: Commit**

```bash
git add ui/components/cluster-panel.slint
git commit -m "feat(ui): redesign ClusterPanel with dark theme"
```

---

### Task 11: Rewrite ProjectsPanel with dark theme components

**Files:**
- Modify: `ui/components/projects-panel.slint` (full rewrite)

**Step 1: Rewrite projects-panel.slint**

Replace entire contents:

```slint
import { Theme } from "../theme.slint";
import { Card } from "card.slint";
import { IconButton } from "icon-button.slint";
import { DataRow } from "data-row.slint";
import { CheckBox, ComboBox, LineEdit, ScrollView } from "std-widgets.slint";

export struct ProjectEntry {
    name: string,
    path: string,
    selected: bool,
    base-image-index: int,
    has-custom-dockerfile: bool,
}

export component ProjectsPanel inherits Rectangle {
    in property <string> projects-dir: "";
    in-out property <[ProjectEntry]> projects: [];
    in property <bool> is-busy: false;
    in property <[string]> base-image-options: ["Node.js", "Python", "Rust", "Go", ".NET", "Minimal", "Custom"];

    callback browse-folder();
    callback refresh-projects();
    callback launch-selected();
    callback stop-selected();
    callback project-toggled(int, bool);
    callback project-image-changed(int, int);

    background: Theme.bg;

    VerticalLayout {
        padding: 24px;
        spacing: 16px;

        Text {
            text: "Projects";
            font-size: 24px;
            font-weight: 700;
            color: Theme.text;
        }

        // Folder picker
        HorizontalLayout {
            spacing: 8px;

            Rectangle {
                horizontal-stretch: 1;
                height: 34px;
                background: Theme.surface;
                border-radius: 6px;
                border-width: 1px;
                border-color: Theme.border;

                HorizontalLayout {
                    padding-left: 12px;
                    padding-right: 12px;

                    Text {
                        text: projects-dir != "" ? projects-dir : "Select projects folder...";
                        font-size: 13px;
                        color: projects-dir != "" ? Theme.text : Theme.text-muted;
                        vertical-alignment: center;
                        overflow: elide;
                    }
                }
            }

            IconButton {
                label: "Browse";
                clicked => { root.browse-folder(); }
            }

            IconButton {
                label: "Refresh";
                enabled: projects-dir != "";
                clicked => { root.refresh-projects(); }
            }
        }

        // Project list card
        Card {
            title: "Project List (" + projects.length + " found)";
            vertical-stretch: 1;

            ScrollView {
                min-height: 200px;

                VerticalLayout {
                    spacing: 0px;

                    for project[idx] in projects: DataRow {
                        alternate: mod(idx, 2) == 1;

                        HorizontalLayout {
                            spacing: 8px;
                            padding-left: 12px;
                            padding-right: 12px;

                            CheckBox {
                                text: project.name;
                                checked: project.selected;
                                toggled => { root.project-toggled(idx, self.checked); }
                            }

                            ComboBox {
                                model: base-image-options;
                                current-index: project.base-image-index;
                                width: 120px;
                                selected(value) => { root.project-image-changed(idx, self.current-index); }
                            }

                            Text {
                                text: project.has-custom-dockerfile ? "(has Dockerfile)" : "";
                                font-size: 11px;
                                color: Theme.text-muted;
                                vertical-alignment: center;
                            }
                        }
                    }
                }
            }
        }

        // Action buttons
        HorizontalLayout {
            spacing: 8px;
            alignment: start;

            IconButton {
                label: "Launch Selected";
                primary: true;
                enabled: !is-busy;
                clicked => { root.launch-selected(); }
            }

            IconButton {
                label: "Stop Selected";
                destructive: true;
                enabled: !is-busy;
                clicked => { root.stop-selected(); }
            }
        }
    }
}
```

**Step 2: Commit**

```bash
git add ui/components/projects-panel.slint
git commit -m "feat(ui): redesign ProjectsPanel with dark theme"
```

---

### Task 12: Rewrite PodsPanel with dark theme components

**Files:**
- Modify: `ui/components/pods-panel.slint` (full rewrite)

**Step 1: Rewrite pods-panel.slint**

Replace entire contents:

```slint
import { Theme } from "../theme.slint";
import { Card } from "card.slint";
import { IconButton } from "icon-button.slint";
import { StatusBadge } from "status-badge.slint";
import { DataRow } from "data-row.slint";
import { ScrollView, LineEdit } from "std-widgets.slint";

export struct PodEntry {
    name: string,
    project: string,
    phase: string,
    ready: bool,
    restart-count: int,
    age: string,
}

export component PodsPanel inherits Rectangle {
    in property <[PodEntry]> pods: [];
    in property <bool> is-busy: false;

    callback refresh-pods();
    callback delete-pod(int);
    callback view-logs(int);
    callback exec-claude(int);
    callback send-prompt(string);
    in-out property <string> claude-prompt: "";
    in property <string> claude-target-pod: "";

    background: Theme.bg;

    VerticalLayout {
        padding: 24px;
        spacing: 16px;

        // Header
        HorizontalLayout {
            spacing: 12px;
            alignment: space-between;

            Text {
                text: "Pods";
                font-size: 24px;
                font-weight: 700;
                color: Theme.text;
                vertical-alignment: center;
            }

            IconButton {
                label: "Refresh";
                enabled: !is-busy;
                clicked => { root.refresh-pods(); }
            }
        }

        // Table header
        HorizontalLayout {
            spacing: 4px;
            height: 28px;
            padding-left: 12px;
            padding-right: 12px;

            Text { text: "Project"; font-weight: 600; width: 140px; font-size: 11px; color: Theme.text-muted; }
            Text { text: "Status"; font-weight: 600; width: 90px; font-size: 11px; color: Theme.text-muted; }
            Text { text: "Ready"; font-weight: 600; width: 50px; font-size: 11px; color: Theme.text-muted; }
            Text { text: "Restarts"; font-weight: 600; width: 60px; font-size: 11px; color: Theme.text-muted; }
            Text { text: "Age"; font-weight: 600; width: 140px; font-size: 11px; color: Theme.text-muted; }
            Text { text: "Actions"; font-weight: 600; horizontal-stretch: 1; font-size: 11px; color: Theme.text-muted; }
        }

        Rectangle { height: 1px; background: Theme.border; }

        // Pod rows
        ScrollView {
            vertical-stretch: 1;
            min-height: 200px;

            VerticalLayout {
                spacing: 0px;

                for pod[idx] in pods: DataRow {
                    alternate: mod(idx, 2) == 1;

                    HorizontalLayout {
                        spacing: 4px;
                        padding-left: 12px;
                        padding-right: 12px;

                        Text {
                            text: pod.project;
                            width: 140px;
                            font-size: 13px;
                            color: Theme.text;
                            vertical-alignment: center;
                            overflow: elide;
                        }

                        HorizontalLayout {
                            width: 90px;
                            StatusBadge { status: pod.phase; }
                        }

                        Text {
                            text: pod.ready ? "Yes" : "No";
                            width: 50px;
                            font-size: 13px;
                            color: pod.ready ? Theme.success : Theme.error;
                            vertical-alignment: center;
                        }

                        Text {
                            text: pod.restart-count;
                            width: 60px;
                            font-size: 13px;
                            color: pod.restart-count > 3 ? Theme.error : Theme.text;
                            vertical-alignment: center;
                        }

                        Text {
                            text: pod.age;
                            width: 140px;
                            font-size: 13px;
                            color: Theme.text-muted;
                            vertical-alignment: center;
                            overflow: elide;
                        }

                        HorizontalLayout {
                            horizontal-stretch: 1;
                            spacing: 4px;

                            IconButton {
                                label: "Logs";
                                clicked => { root.view-logs(idx); }
                            }

                            IconButton {
                                label: "Claude";
                                clicked => { root.exec-claude(idx); }
                            }

                            IconButton {
                                label: "Delete";
                                destructive: true;
                                clicked => { root.delete-pod(idx); }
                            }
                        }
                    }
                }
            }
        }

        // Claude prompt
        if claude-target-pod != "" : HorizontalLayout {
            spacing: 8px;
            height: 36px;

            Text {
                text: "Prompt for " + claude-target-pod + ":";
                font-size: 12px;
                color: Theme.text-muted;
                vertical-alignment: center;
            }

            LineEdit {
                text <=> root.claude-prompt;
                placeholder-text: "Enter a prompt for Claude...";
                horizontal-stretch: 1;
                font-size: 13px;
            }

            IconButton {
                label: "Send";
                primary: true;
                enabled: claude-prompt != "";
                clicked => { root.send-prompt(claude-prompt); }
            }
        }

        // Summary
        Text {
            text: "Total: " + pods.length;
            font-size: 12px;
            color: Theme.text-muted;
        }
    }
}
```

**Step 2: Commit**

```bash
git add ui/components/pods-panel.slint
git commit -m "feat(ui): redesign PodsPanel with dark theme"
```

---

### Task 13: Rewrite app-window.slint with sidebar layout and settings

This is the largest task — replaces TabWidget with sidebar navigation and inline settings page.

**Files:**
- Modify: `ui/app-window.slint` (full rewrite)

**Step 1: Rewrite app-window.slint**

Replace entire contents:

```slint
import { Theme } from "theme.slint";
import { NavItem } from "components/nav-item.slint";
import { Card } from "components/card.slint";
import { IconButton } from "components/icon-button.slint";
import { StatusBadge } from "components/status-badge.slint";
import { FormField } from "components/form-field.slint";
import { ClusterPanel } from "components/cluster-panel.slint";
import { ProjectsPanel, ProjectEntry } from "components/projects-panel.slint";
import { PodsPanel, PodEntry } from "components/pods-panel.slint";
import { SetupPanel } from "components/setup-panel.slint";
import { ScrollView, ComboBox } from "std-widgets.slint";

export { ProjectEntry, PodEntry }

export component AppWindow inherits Window {
    title: "Claude in K3s";
    min-width: 900px;
    min-height: 650px;
    preferred-width: 1024px;
    preferred-height: 768px;
    background: Theme.bg;

    // Navigation state
    in-out property <int> active-page: 0;

    // Cluster state
    in property <string> cluster-status: "Unknown";
    in property <string> cluster-log: "";
    in property <bool> is-busy: false;
    in property <bool> tf-initialized: false;

    // Projects state
    in property <string> projects-dir: "";
    in-out property <[ProjectEntry]> projects: [];

    // Pods state
    in property <[PodEntry]> pods: [];

    // Claude exec state
    in-out property <string> claude-prompt: "";
    in property <string> claude-target-pod: "";

    // Settings state
    in-out property <string> api-key: "";
    in property <string> platform-name: "Linux";
    in-out property <string> claude-mode: "daemon";
    in-out property <string> git-user-name: "Claude Code Bot";
    in-out property <string> git-user-email: "claude-bot@localhost";
    in-out property <string> cpu-limit: "2";
    in-out property <string> memory-limit: "4Gi";
    in-out property <string> terraform-dir: "terraform";
    in-out property <string> helm-chart-dir: "helm/claude-code";

    // Deps state
    in property <bool> all-deps-met: true;
    in property <bool> k3s-found: false;
    in property <bool> terraform-found: false;
    in property <bool> helm-found: false;
    in property <bool> docker-found: false;
    in property <string> k3s-label: "k3s";
    in property <string> k3s-version: "";
    in property <string> terraform-version: "";
    in property <string> helm-version: "";
    in property <string> docker-version: "";
    in property <string> install-log: "";
    in property <bool> is-installing: false;

    // Cluster callbacks
    callback terraform-init();
    callback terraform-apply();
    callback terraform-destroy();
    callback terraform-plan();
    callback helm-status();

    // Projects callbacks
    callback browse-folder();
    callback refresh-projects();
    callback launch-selected();
    callback stop-selected();
    callback project-toggled(int, bool);
    callback project-image-changed(int, int);

    // Pods callbacks
    callback refresh-pods();
    callback delete-pod(int);
    callback view-logs(int);
    callback exec-claude(int);
    callback send-prompt(string);

    // Settings callbacks
    callback save-settings();

    // Deps callbacks
    callback install-missing();
    callback continue-app();

    HorizontalLayout {
        spacing: 0px;

        // ─── Sidebar ───
        Rectangle {
            width: 180px;
            background: Theme.surface-alt;

            VerticalLayout {
                padding-top: 16px;
                spacing: 0px;

                // Logo / brand
                HorizontalLayout {
                    padding-left: 16px;
                    padding-right: 16px;
                    padding-bottom: 20px;
                    spacing: 8px;

                    Text {
                        text: "CK3";
                        font-size: 20px;
                        font-weight: 800;
                        color: Theme.accent;
                        vertical-alignment: center;
                    }

                    Text {
                        text: "Claude in K3s";
                        font-size: 11px;
                        color: Theme.text-muted;
                        vertical-alignment: center;
                    }
                }

                Rectangle { height: 1px; background: Theme.border; }

                // Nav items
                NavItem {
                    icon: "\u{25B6}";
                    label: "Cluster";
                    active: active-page == 0;
                    clicked => { active-page = 0; }
                }

                NavItem {
                    icon: "\u{25A3}";
                    label: "Projects";
                    active: active-page == 1;
                    clicked => { active-page = 1; }
                }

                NavItem {
                    icon: "\u{25CB}";
                    label: "Pods";
                    active: active-page == 2;
                    clicked => { active-page = 2; }
                }

                // Spacer
                Rectangle { vertical-stretch: 1; }

                Rectangle { height: 1px; background: Theme.border; }

                NavItem {
                    icon: "\u{2699}";
                    label: "Settings";
                    active: active-page == 3;
                    clicked => { active-page = 3; }
                }

                // Status at bottom of sidebar
                HorizontalLayout {
                    padding: 12px;
                    spacing: 8px;

                    Rectangle {
                        width: 8px;
                        height: 8px;
                        y: 4px;
                        border-radius: 4px;
                        background: cluster-status == "Healthy" ? Theme.success :
                                    cluster-status == "Starting" ? Theme.warning : Theme.error;
                    }

                    Text {
                        text: is-busy ? "Working..." : cluster-status;
                        font-size: 11px;
                        color: Theme.text-muted;
                    }
                }
            }
        }

        // ─── Sidebar divider ───
        Rectangle {
            width: 1px;
            background: Theme.border;
        }

        // ─── Content area ───
        Rectangle {
            horizontal-stretch: 1;
            background: Theme.bg;

            // Setup panel — shown when deps are missing
            if !all-deps-met : SetupPanel {
                k3s-found: root.k3s-found;
                terraform-found: root.terraform-found;
                helm-found: root.helm-found;
                docker-found: root.docker-found;
                k3s-label: root.k3s-label;
                k3s-version: root.k3s-version;
                terraform-version: root.terraform-version;
                helm-version: root.helm-version;
                docker-version: root.docker-version;
                install-log: root.install-log;
                is-installing: root.is-installing;

                install-missing => { root.install-missing(); }
                continue-app => { root.continue-app(); }
            }

            // Page content — shown when all deps are met
            if all-deps-met && active-page == 0 : ClusterPanel {
                cluster-status: root.cluster-status;
                log-output: root.cluster-log;
                is-busy: root.is-busy;
                tf-initialized: root.tf-initialized;

                terraform-init => { root.terraform-init(); }
                terraform-apply => { root.terraform-apply(); }
                terraform-destroy => { root.terraform-destroy(); }
                terraform-plan => { root.terraform-plan(); }
                helm-status => { root.helm-status(); }
            }

            if all-deps-met && active-page == 1 : ProjectsPanel {
                projects-dir: root.projects-dir;
                projects: root.projects;
                is-busy: root.is-busy;

                browse-folder => { root.browse-folder(); }
                refresh-projects => { root.refresh-projects(); }
                launch-selected => { root.launch-selected(); }
                stop-selected => { root.stop-selected(); }
                project-toggled(idx, checked) => { root.project-toggled(idx, checked); }
                project-image-changed(idx, img) => { root.project-image-changed(idx, img); }
            }

            if all-deps-met && active-page == 2 : PodsPanel {
                pods: root.pods;
                is-busy: root.is-busy;

                refresh-pods => { root.refresh-pods(); }
                delete-pod(idx) => { root.delete-pod(idx); }
                view-logs(idx) => { root.view-logs(idx); }
                exec-claude(idx) => { root.exec-claude(idx); }
                send-prompt(prompt) => { root.send-prompt(prompt); }
                claude-prompt <=> root.claude-prompt;
                claude-target-pod: root.claude-target-pod;
            }

            if all-deps-met && active-page == 3 : Rectangle {
                background: Theme.bg;

                ScrollView {
                    VerticalLayout {
                        padding: 24px;
                        spacing: 16px;

                        Text {
                            text: "Settings";
                            font-size: 24px;
                            font-weight: 700;
                            color: Theme.text;
                        }

                        Card {
                            title: "Anthropic API Key";

                            FormField {
                                label: "API Key";
                                text <=> root.api-key;
                                placeholder: "sk-ant-...";
                                is-password: true;
                            }
                        }

                        Card {
                            title: "Claude Mode";

                            VerticalLayout {
                                spacing: 4px;

                                Text {
                                    text: "Mode";
                                    font-size: 12px;
                                    font-weight: 500;
                                    color: Theme.text-muted;
                                }

                                ComboBox {
                                    model: ["daemon", "headless"];
                                    current-value <=> root.claude-mode;
                                }
                            }
                        }

                        Card {
                            title: "Git Configuration";

                            VerticalLayout {
                                spacing: 12px;

                                FormField {
                                    label: "User Name";
                                    text <=> root.git-user-name;
                                    placeholder: "Claude Code Bot";
                                }

                                FormField {
                                    label: "Email";
                                    text <=> root.git-user-email;
                                    placeholder: "claude-bot@localhost";
                                }
                            }
                        }

                        Card {
                            title: "Resource Limits";

                            VerticalLayout {
                                spacing: 12px;

                                FormField {
                                    label: "CPU Limit";
                                    text <=> root.cpu-limit;
                                    placeholder: "2";
                                }

                                FormField {
                                    label: "Memory";
                                    text <=> root.memory-limit;
                                    placeholder: "4Gi";
                                }
                            }
                        }

                        Card {
                            title: "Directories";

                            VerticalLayout {
                                spacing: 12px;

                                FormField {
                                    label: "Terraform Directory";
                                    text <=> root.terraform-dir;
                                    placeholder: "terraform";
                                }

                                FormField {
                                    label: "Helm Chart Directory";
                                    text <=> root.helm-chart-dir;
                                    placeholder: "helm/claude-code";
                                }
                            }
                        }

                        Card {
                            title: "Platform Info";

                            Text {
                                text: "Detected platform: " + root.platform-name;
                                font-size: 13px;
                                color: Theme.text;
                            }
                        }

                        IconButton {
                            label: "Save Settings";
                            primary: true;
                            clicked => { root.save-settings(); }
                        }

                        // Spacer
                        Rectangle { vertical-stretch: 1; }
                    }
                }
            }
        }
    }
}
```

**Step 2: Build and verify compilation**

```bash
cargo build 2>&1
```

This is the critical integration point. Fix any Slint compiler errors.

**Step 3: Run tests**

```bash
cargo test 2>&1
```

The existing `ui_property_defaults` test in `tests/ui_tests.rs` must still pass. All property names, types, callbacks, and struct exports are preserved identically.

**Step 4: Commit**

```bash
git add ui/app-window.slint
git commit -m "feat(ui): rewrite app-window with sidebar layout and dark theme"
```

---

### Task 14: Fix compilation errors and polish

This task handles any Slint compiler issues that arise from the integration.

**Step 1: Build and collect errors**

```bash
cargo build 2>&1
```

**Common issues to watch for:**

1. **`@children` in Card**: Slint requires components using `@children` to declare it. If Slint version doesn't support `@children` cleanly, replace Card usage with inline Rectangle + VerticalLayout wrappers.

2. **`animate` on computed properties**: If Slint complains about animating expression-based backgrounds, move the background logic into explicit states using `states` blocks instead.

3. **`hovered` in TouchArea**: The `moved =>` callback may not set `has-hover`. Use `pointer-event` or `has-hover` property directly:
```slint
TouchArea {
    ta := TouchArea {}
    // then reference ta.has-hover
}
```

4. **Import paths**: Component-to-component imports within `ui/components/` use relative paths like `"icon-button.slint"` (same directory). Theme imports use `"../theme.slint"`.

**Step 2: Fix each error, rebuild, iterate**

**Step 3: Run full test suite**

```bash
cargo test 2>&1
```

All 102+ tests must pass.

**Step 4: Commit**

```bash
git add -A
git commit -m "fix(ui): resolve Slint compilation issues from redesign"
```

---

### Task 15: Visual verification and final polish

**Step 1: Run the app**

```bash
cargo run
```

**Step 2: Visual checklist**

- [ ] Dark background applied everywhere (no white flashes)
- [ ] Sidebar renders with nav items, active state highlights correctly
- [ ] Clicking nav items switches between Cluster/Projects/Pods/Settings
- [ ] Cluster panel: StatusBadge shows, terraform buttons have hover/press states
- [ ] Projects panel: folder picker works, project list renders with alternating rows
- [ ] Pods panel: table renders, action buttons work, Claude prompt appears when target selected
- [ ] Settings panel: all form fields editable, Save button works
- [ ] Setup panel: shows when deps are missing, install flow works
- [ ] Buttons disabled state is visually distinct (lower opacity)
- [ ] Destroy/Delete buttons turn red on hover

**Step 3: Make any visual tweaks needed** (spacing, alignment, colors)

**Step 4: Final commit**

```bash
git add -A
git commit -m "feat(ui): complete dark theme UI redesign"
```
