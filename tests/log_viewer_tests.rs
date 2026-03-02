//! Tests to verify text binding approaches for a copyable log viewer.
//! We test multiple strategies to find one where:
//! 1. Text set from Rust propagates to the inner widget
//! 2. Text is selectable/copyable (TextEdit supports this, plain Text does not)

/// Approach A: TextEdit with one-way binding (text: root.log-text)
mod approach_a_oneway {
    slint::slint! {
        import { TextEdit } from "std-widgets.slint";

        export component TestViewer inherits Window {
            in property <string> log-text: "";
            out property <string> actual-text: te.text;

            te := TextEdit {
                text: root.log-text;
                read-only: true;
            }
        }
    }

    #[test]
    fn text_propagates_on_set() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        // Initially empty
        assert_eq!(ui.get_actual_text(), "", "should start empty");

        // Set text from Rust side
        ui.set_log_text("Hello from Rust".into());
        assert_eq!(
            ui.get_actual_text(),
            "Hello from Rust",
            "one-way binding should propagate to TextEdit"
        );
    }

    #[test]
    fn text_updates_on_subsequent_sets() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        ui.set_log_text("Line 1".into());
        assert_eq!(ui.get_actual_text(), "Line 1");

        ui.set_log_text("Line 1\nLine 2".into());
        assert_eq!(ui.get_actual_text(), "Line 1\nLine 2");

        ui.set_log_text("Line 1\nLine 2\nLine 3".into());
        assert_eq!(ui.get_actual_text(), "Line 1\nLine 2\nLine 3");
    }
}

/// Approach B: TextEdit with two-way binding (text <=> root.log-text)
mod approach_b_twoway {
    slint::slint! {
        import { TextEdit } from "std-widgets.slint";

        export component TestViewer inherits Window {
            in-out property <string> log-text: "";
            out property <string> actual-text: te.text;

            te := TextEdit {
                text <=> root.log-text;
                read-only: true;
            }
        }
    }

    #[test]
    fn text_propagates_on_set() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        assert_eq!(ui.get_actual_text(), "", "should start empty");

        ui.set_log_text("Hello from Rust".into());
        assert_eq!(
            ui.get_actual_text(),
            "Hello from Rust",
            "two-way binding should propagate to TextEdit"
        );
    }

    #[test]
    fn text_updates_on_subsequent_sets() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        ui.set_log_text("Line 1".into());
        assert_eq!(ui.get_actual_text(), "Line 1");

        ui.set_log_text("Line 1\nLine 2".into());
        assert_eq!(ui.get_actual_text(), "Line 1\nLine 2");
    }
}

/// Approach D: TextEdit inside VerticalLayout (ensures it stretches to fill parent)
mod approach_d_textedit_in_layout {
    slint::slint! {
        import { TextEdit } from "std-widgets.slint";

        export component TestViewer inherits Window {
            in property <string> log-text: "";
            out property <string> actual-text: te.text;
            out property <length> te-width: te.width;
            out property <length> te-height: te.height;

            width: 400px;
            height: 300px;

            VerticalLayout {
                te := TextEdit {
                    text: root.log-text;
                    read-only: true;
                    font-size: 12px;
                }
            }
        }
    }

    #[test]
    fn text_propagates() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        ui.set_log_text("Hello from Rust".into());
        assert_eq!(ui.get_actual_text(), "Hello from Rust");
    }

    #[test]
    fn textedit_has_nonzero_size() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        ui.set_log_text("Some log text\nLine 2".into());
        assert!(
            ui.get_te_width() > 0.,
            "TextEdit width should be > 0, got: {}",
            ui.get_te_width()
        );
        assert!(
            ui.get_te_height() > 0.,
            "TextEdit height should be > 0, got: {}",
            ui.get_te_height()
        );
    }
}

/// Approach E: TextEdit inside Rectangle with VerticalLayout (matches actual LogViewer)
mod approach_e_textedit_in_rect_with_layout {
    slint::slint! {
        import { TextEdit } from "std-widgets.slint";

        export component TestViewer inherits Window {
            in property <string> log-text: "";
            out property <string> actual-text: te.text;
            out property <length> te-width: te.width;
            out property <length> te-height: te.height;

            width: 400px;
            height: 300px;

            Rectangle {
                min-height: 150px;

                VerticalLayout {
                    te := TextEdit {
                        text: root.log-text;
                        read-only: true;
                        font-size: 12px;
                    }
                }
            }
        }
    }

    #[test]
    fn text_propagates() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        ui.set_log_text("Hello from Rust".into());
        assert_eq!(ui.get_actual_text(), "Hello from Rust");
    }

    #[test]
    fn textedit_has_nonzero_size() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        ui.set_log_text("Some log text".into());
        assert!(
            ui.get_te_width() > 0.,
            "TextEdit in VerticalLayout width should be > 0, got: {}",
            ui.get_te_width()
        );
        assert!(
            ui.get_te_height() > 0.,
            "TextEdit in VerticalLayout height should be > 0, got: {}",
            ui.get_te_height()
        );
    }
}

/// Approach C: Plain Text in ScrollView (baseline — known working for display, but not copyable)
mod approach_c_plain_text {
    slint::slint! {
        import { ScrollView } from "std-widgets.slint";

        export component TestViewer inherits Window {
            in property <string> log-text: "";
            out property <string> actual-text: txt.text;

            ScrollView {
                txt := Text {
                    text: root.log-text;
                }
            }
        }
    }

    #[test]
    fn text_propagates_on_set() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        assert_eq!(ui.get_actual_text(), "", "should start empty");

        ui.set_log_text("Hello from Rust".into());
        assert_eq!(
            ui.get_actual_text(),
            "Hello from Rust",
            "plain Text binding should propagate"
        );
    }

    #[test]
    fn text_updates_on_subsequent_sets() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = TestViewer::new().unwrap();

        ui.set_log_text("Line 1".into());
        assert_eq!(ui.get_actual_text(), "Line 1");

        ui.set_log_text("Line 1\nLine 2".into());
        assert_eq!(ui.get_actual_text(), "Line 1\nLine 2");
    }
}
