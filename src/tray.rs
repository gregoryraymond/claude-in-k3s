use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::Icon;
use tray_icon::TrayIconBuilder;

/// Status text shown in the tray menu.
pub struct TrayState {
    pub cluster_item: MenuItem,
    pub pods_item: MenuItem,
    pub show_item: MenuItem,
    pub exit_item: MenuItem,
    _tray: tray_icon::TrayIcon,
}

impl TrayState {
    pub fn new() -> anyhow::Result<Self> {
        let icon = create_icon()?;

        let cluster_item = MenuItem::new("Cluster: Unknown", false, None);
        let pods_item = MenuItem::new("Pods: 0", false, None);
        let show_item = MenuItem::new("Show Window", true, None);
        let exit_item = MenuItem::new("Exit", true, None);

        let menu = Menu::new();
        menu.append(&cluster_item)?;
        menu.append(&pods_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&show_item)?;
        menu.append(&exit_item)?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Claude in K3s")
            .with_icon(icon)
            .build()?;

        Ok(Self {
            cluster_item,
            pods_item,
            show_item,
            exit_item,
            _tray: tray,
        })
    }

    pub fn update_status(&self, cluster_status: &str, pod_count: usize) {
        self.cluster_item
            .set_text(format!("Cluster: {}", cluster_status));
        self.pods_item.set_text(format!("Pods: {}", pod_count));
    }
}

/// Generate a 32x32 RGBA icon programmatically.
/// Orange circle with a white "C" shape — represents Claude.
fn create_icon() -> anyhow::Result<Icon> {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let radius = 14.0f32;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;

            if dist <= radius {
                // Orange circle background (#E07B39 — Claude orange)
                rgba[idx] = 0xE0;
                rgba[idx + 1] = 0x7B;
                rgba[idx + 2] = 0x39;
                rgba[idx + 3] = 0xFF;

                // Draw a white "C" shape
                let inner_dist = dist;
                let angle = dy.atan2(dx);
                let ring_outer = 10.0;
                let ring_inner = 5.5;

                if inner_dist >= ring_inner
                    && inner_dist <= ring_outer
                    && !(angle > -0.7 && angle < 0.7)
                {
                    // White pixels for the C
                    rgba[idx] = 0xFF;
                    rgba[idx + 1] = 0xFF;
                    rgba[idx + 2] = 0xFF;
                    rgba[idx + 3] = 0xFF;
                }
            } else if dist <= radius + 1.0 {
                // Anti-alias edge
                let alpha = ((radius + 1.0 - dist) * 255.0) as u8;
                rgba[idx] = 0xE0;
                rgba[idx + 1] = 0x7B;
                rgba[idx + 2] = 0x39;
                rgba[idx + 3] = alpha;
            }
        }
    }

    Ok(Icon::from_rgba(rgba, size, size)?)
}
