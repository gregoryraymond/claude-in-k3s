/// Abstraction over progress/UI reporting for testability.
///
/// In production, [`SlintProgress`] wraps the Slint UI handle.
/// In tests, [`RecordingProgress`] captures events for assertions.
pub trait Progress: Send + Sync {
    /// Log a message to the cluster log buffer.
    fn log(&self, msg: &str);

    /// Add a new progress step (e.g. "Docker", "Building my-project").
    fn add_step(&self, label: &str, status: &str, message: &str);

    /// Update an existing progress step by index.
    fn update_step(&self, idx: usize, status: &str, message: &str);

    /// Append text to a launch tab's log.
    fn append_tab(&self, tab: i32, text: &str);

    /// Update a launch tab's status label.
    fn update_tab_status(&self, tab: i32, status: &str);

    /// Set the busy/loading state.
    fn set_busy(&self, busy: bool);

    /// Show a toast notification.
    fn show_toast(&self, message: &str, level: &str, target_page: i32);

    /// Set the recovery hint text.
    fn set_recovery_hint(&self, hint: &str);
}

/// Event recorded by [`RecordingProgress`] for test assertions.
#[derive(Debug, Clone, PartialEq)]
pub enum ProgressEvent {
    Log { msg: String },
    AddStep { label: String, status: String, message: String },
    UpdateStep { idx: usize, status: String, message: String },
    AppendTab { tab: i32, text: String },
    UpdateTabStatus { tab: i32, status: String },
    SetBusy(bool),
    ShowToast { message: String, level: String, target_page: i32 },
    SetRecoveryHint { hint: String },
}

/// Test-only implementation that records all progress events.
pub struct RecordingProgress {
    events: std::sync::Mutex<Vec<ProgressEvent>>,
}

impl RecordingProgress {
    /// Create a new recording progress tracker.
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Return all recorded events.
    pub fn events(&self) -> Vec<ProgressEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Check if any event matches a predicate.
    pub fn has_event<F: Fn(&ProgressEvent) -> bool>(&self, predicate: F) -> bool {
        self.events.lock().unwrap().iter().any(predicate)
    }

    /// Count events matching a predicate.
    pub fn count_events<F: Fn(&ProgressEvent) -> bool>(&self, predicate: F) -> usize {
        self.events.lock().unwrap().iter().filter(|e| predicate(e)).count()
    }
}

impl Progress for RecordingProgress {
    fn log(&self, msg: &str) {
        self.events.lock().unwrap().push(ProgressEvent::Log {
            msg: msg.to_string(),
        });
    }

    fn add_step(&self, label: &str, status: &str, message: &str) {
        self.events.lock().unwrap().push(ProgressEvent::AddStep {
            label: label.to_string(),
            status: status.to_string(),
            message: message.to_string(),
        });
    }

    fn update_step(&self, idx: usize, status: &str, message: &str) {
        self.events.lock().unwrap().push(ProgressEvent::UpdateStep {
            idx,
            status: status.to_string(),
            message: message.to_string(),
        });
    }

    fn append_tab(&self, tab: i32, text: &str) {
        self.events.lock().unwrap().push(ProgressEvent::AppendTab {
            tab,
            text: text.to_string(),
        });
    }

    fn update_tab_status(&self, tab: i32, status: &str) {
        self.events.lock().unwrap().push(ProgressEvent::UpdateTabStatus {
            tab,
            status: status.to_string(),
        });
    }

    fn set_busy(&self, busy: bool) {
        self.events.lock().unwrap().push(ProgressEvent::SetBusy(busy));
    }

    fn show_toast(&self, message: &str, level: &str, target_page: i32) {
        self.events.lock().unwrap().push(ProgressEvent::ShowToast {
            message: message.to_string(),
            level: level.to_string(),
            target_page,
        });
    }

    fn set_recovery_hint(&self, hint: &str) {
        self.events.lock().unwrap().push(ProgressEvent::SetRecoveryHint {
            hint: hint.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_progress_captures_events() {
        let progress = RecordingProgress::new();
        progress.log("hello");
        progress.add_step("Docker", "running", "Checking...");
        progress.update_step(0, "done", "Running");
        progress.append_tab(0, "some output");
        progress.update_tab_status(0, "Done");
        progress.set_busy(true);
        progress.show_toast("deployed", "info", 2);
        progress.set_recovery_hint("try again");

        let events = progress.events();
        assert_eq!(events.len(), 8);
        assert_eq!(events[0], ProgressEvent::Log { msg: "hello".into() });
        assert_eq!(events[1], ProgressEvent::AddStep {
            label: "Docker".into(),
            status: "running".into(),
            message: "Checking...".into(),
        });
        assert_eq!(events[5], ProgressEvent::SetBusy(true));
    }

    #[test]
    fn recording_progress_has_event() {
        let progress = RecordingProgress::new();
        progress.log("test message");
        progress.set_busy(false);

        assert!(progress.has_event(|e| matches!(e, ProgressEvent::Log { msg } if msg == "test message")));
        assert!(progress.has_event(|e| matches!(e, ProgressEvent::SetBusy(false))));
        assert!(!progress.has_event(|e| matches!(e, ProgressEvent::SetBusy(true))));
    }

    #[test]
    fn recording_progress_count_events() {
        let progress = RecordingProgress::new();
        progress.add_step("A", "running", "");
        progress.add_step("B", "running", "");
        progress.update_step(0, "done", "ok");

        assert_eq!(progress.count_events(|e| matches!(e, ProgressEvent::AddStep { .. })), 2);
        assert_eq!(progress.count_events(|e| matches!(e, ProgressEvent::UpdateStep { .. })), 1);
    }

    #[test]
    fn recording_progress_starts_empty() {
        let progress = RecordingProgress::new();
        assert!(progress.events().is_empty());
    }

    #[test]
    fn recording_progress_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RecordingProgress>();
    }

    #[test]
    fn progress_trait_is_object_safe() {
        // This should compile: Progress has no async methods or generic parameters
        fn takes_progress(_p: &dyn Progress) {}
        let rp = RecordingProgress::new();
        takes_progress(&rp);
    }
}
