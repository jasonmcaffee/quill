# Task 1756 - Unluminous web support

- [x] Research IntelliJ's browser workflow and viable native web-view architectures for Unluminous.
- [x] Write and publish the technical design document with security, lifecycle, and performance decisions.
- [x] Add browser-tab state, local-file serving, navigation, and resource loading through a bounded web-view service.
- [x] Add the explorer `Open in Browser -> Tab` action and an agent-reachable command that share one implementation path.
- [x] Cover the human and agent paths, dependent assets, navigation, failures, lifecycle, and memory bounds with automated tests.
- [x] Measure startup/runtime overhead and verify the feature end-to-end in a real released Unluminous window.
- [x] Update documentation, commit the task changes, and publish a minor Unluminous release.

Found and fixed while verifying in a real window, none of it visible from the code alone:

- [x] A second browser tab hung the window for good; a window now has one native view, pointed at the
      tab that is showing, and each tab keeps its own history.
- [x] A tab the view was pointed back at kept showing the previous page, because `load_url` is handed
      straight to `Navigate` and the `unluminous://` scheme is refused in silence.
- [x] View creation moved out of the egui pass, where it blocks in a nested message pump.
- [x] Readable local addresses, scheme-less web addresses, popup occlusion, and editor commands that
      refuse a rendered tab instead of answering about the empty document behind it.
