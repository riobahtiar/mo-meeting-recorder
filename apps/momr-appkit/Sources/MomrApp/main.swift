import AppKit

/// The ready page: two live meters (microphone above, computer below), a
/// status line and Start. Recording, done and Settings arrive in later
/// slices; this slice proves the shell, the helper path and the meters.
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow!
    private let mic = SourceCapture("mic", label: "Microphone")
    private let computer = SourceCapture("system", label: "Computer audio")
    private let micMeter = MeterView()
    private let computerMeter = MeterView()
    private let statusLabel = NSTextField(labelWithString: "")
    private let clockLabel = NSTextField(labelWithString: "")
    private let startButton = NSButton(title: "Start Recording", target: nil, action: nil)
    private let pauseButton = NSButton(title: "Pause", target: nil, action: nil)
    private let stopButton = NSButton(title: "Stop", target: nil, action: nil)
    private let revealButton = NSButton(title: "Reveal in Finder", target: nil, action: nil)
    private var recorder: Recorder?
    private var clockTimer: Timer?
    private var meetingURL: URL?
    /// What the status line says while no recording runs: the last outcome
    /// (saved, failed, refused) or nil for the listening hint.
    private var message: String?

    func applicationDidFinishLaunching(_: Notification) {
        buildMenu()
        buildWindow()
        mic.onLevel = { [weak self] in self?.micMeter.level = CGFloat($0) }
        computer.onLevel = { [weak self] in self?.computerMeter.level = CGFloat($0) }
        mic.onNote = { [weak self] _ in self?.updateStatus() }
        computer.onNote = { [weak self] _ in self?.updateStatus() }
        mic.start()
        computer.start()
        updateStatus()
    }

    func applicationWillTerminate(_: Notification) {
        mic.stop()
        computer.stop()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_: NSApplication) -> Bool {
        true
    }

    // MARK: - Window

    private func buildWindow() {
        let content = NSView()
        content.translatesAutoresizingMaskIntoConstraints = false

        let title = NSTextField(labelWithString: "MOM Recorder")
        title.font = .systemFont(ofSize: 20, weight: .semibold)
        let micLabel = NSTextField(labelWithString: "Microphone")
        let computerLabel = NSTextField(labelWithString: "Computer audio")
        statusLabel.font = .systemFont(ofSize: 12)
        statusLabel.textColor = .secondaryLabelColor
        clockLabel.font = .monospacedDigitSystemFont(ofSize: 28, weight: .regular)
        clockLabel.isHidden = true
        startButton.bezelStyle = .rounded
        startButton.keyEquivalent = "r"
        startButton.keyEquivalentModifierMask = .command
        startButton.target = self
        startButton.action = #selector(startRecording)
        pauseButton.target = self
        pauseButton.action = #selector(togglePause)
        pauseButton.isHidden = true
        stopButton.target = self
        stopButton.action = #selector(stopRecording)
        stopButton.isHidden = true
        revealButton.target = self
        revealButton.action = #selector(revealMeeting)
        revealButton.isHidden = true
        let stack = NSStackView(views: [title, clockLabel, micLabel, micMeter, computerLabel, computerMeter, statusLabel, startButton, pauseButton, stopButton, revealButton])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 24),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -24),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 20),
            stack.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -20),
            micMeter.heightAnchor.constraint(equalToConstant: 24),
            computerMeter.heightAnchor.constraint(equalToConstant: 24),
            micMeter.widthAnchor.constraint(equalTo: stack.widthAnchor),
            computerMeter.widthAnchor.constraint(equalTo: stack.widthAnchor),
        ])

        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 320),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "MOM Recorder"
        window.contentView = content
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
    }

    @objc private func startRecording() {
        // A track whose capture is down records nothing; with both down the
        // meeting would be silence under a running clock.
        guard mic.isRunning || computer.isRunning else {
            show("Nothing is capturing, so there is nothing to record.")
            return
        }
        guard let recorder = Recorder(mic: mic, computer: computer) else {
            show("Could not open the staging folder.")
            return
        }
        self.recorder = recorder
        meetingURL = nil
        message = nil
        revealButton.isHidden = true
        startButton.isHidden = true
        pauseButton.isHidden = false
        stopButton.isHidden = false
        clockLabel.isHidden = false
        pauseButton.title = "Pause"
        clockTimer?.invalidate()
        clockTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            self?.tickClock()
        }
        tickClock()
    }

    private func tickClock() {
        guard let recorder else { return }
        let s = recorder.elapsed
        clockLabel.stringValue = String(format: "%02d:%02d", s / 60, s % 60)
        window.title = recorder.paused ? "Paused" : "● Recording"
        updateStatus()
    }

    /// Each source's reason for not capturing, if any.
    private var notes: [String] {
        [mic.note, computer.note].compactMap { $0 }
    }

    @objc private func togglePause() {
        guard let recorder else { return }
        if recorder.paused {
            recorder.resume()
            pauseButton.title = "Pause"
        } else {
            recorder.pause()
            pauseButton.title = "Resume"
        }
        tickClock()
    }

    @objc private func stopRecording() {
        guard let recorder else { return }
        self.recorder = nil
        clockTimer?.invalidate()
        pauseButton.isHidden = true
        stopButton.isHidden = true
        window.title = "MOM Recorder"
        show("Finishing: encoding and transcribing…")
        recorder.stop(title: "Meeting") { [weak self] url, error in
            guard let self else { return }
            self.startButton.isHidden = false
            self.clockLabel.isHidden = true
            if let url {
                self.meetingURL = url
                self.revealButton.isHidden = false
                self.show("Saved \(url.lastPathComponent).")
            } else {
                self.show(error ?? "Something went wrong.")
            }
        }
    }

    @objc private func revealMeeting() {
        if let url = meetingURL {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        }
    }

    /// The status line: the state or last outcome, then each source's note,
    /// so a capture that stopped mid-meeting is visible, not hidden behind
    /// "Recording".
    private func updateStatus() {
        var parts = notes
        if let recorder {
            parts.insert(recorder.paused ? "Paused." : "Recording.", at: 0)
        } else {
            parts.insert(message ?? "Listening: speak and play sound to move the meters.", at: 0)
        }
        statusLabel.stringValue = parts.joined(separator: " ")
    }

    private func show(_ text: String) {
        message = text
        updateStatus()
    }

    // MARK: - Menu

    private func buildMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        main.addItem(appItem)
        let app = NSMenu()
        app.addItem(NSMenuItem(title: "About MOM Recorder", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: ""))
        app.addItem(.separator())
        let settings = NSMenuItem(title: "Settings…", action: #selector(showSettings), keyEquivalent: ",")
        settings.target = self
        app.addItem(settings)
        app.addItem(.separator())
        app.addItem(NSMenuItem(title: "Quit MOM Recorder", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
        appItem.submenu = app
        NSApp.mainMenu = main
    }

    @objc private func showSettings() {
        let alert = NSAlert()
        alert.messageText = "Settings arrives with the Transcription slice"
        alert.runModal()
    }
}

// No nib names the delegate, so it is set by hand: `@main` on an AppKit
// delegate relies on MainMenu.xib to create it, and without one the app
// runs with no delegate, no window and no menu. `delegate` is weak on
// NSApplication, so this constant keeps it alive.
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
// An unbundled executable (`swift run`) starts as a background process;
// regular gives it a Dock icon, a menu bar and key windows.
app.setActivationPolicy(.regular)
app.run()
