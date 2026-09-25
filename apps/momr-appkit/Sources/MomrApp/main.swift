import AppKit

/// The ready page: two live meters (microphone above, computer below), a
/// status line and Start. Recording, done and Settings arrive in later
/// slices; this slice proves the shell, the helper path and the meters.
@main
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow!
    private let mic = SourceCapture("mic")
    private let computer = SourceCapture("system")
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

    func applicationDidFinishLaunching(_: Notification) {
        buildMenu()
        buildWindow()
        mic.onLevel = { [weak self] in self?.micMeter.level = CGFloat($0) }
        computer.onLevel = { [weak self] in self?.computerMeter.level = CGFloat($0) }
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
        guard let recorder = Recorder(mic: mic, computer: computer) else {
            statusLabel.stringValue = "Could not open the staging folder."
            return
        }
        self.recorder = recorder
        meetingURL = nil
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
        statusLabel.stringValue = recorder.paused ? "Paused." : "Recording both tracks."
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
        statusLabel.stringValue = "Finishing: encoding and transcribing…"
        recorder.stop(title: "Meeting") { [weak self] url, error in
            guard let self else { return }
            if let url {
                self.meetingURL = url
                self.revealButton.isHidden = false
                self.startButton.isHidden = false
                self.clockLabel.isHidden = true
                self.statusLabel.stringValue = "Saved \(url.lastPathComponent)."
            } else {
                self.startButton.isHidden = false
                self.clockLabel.isHidden = true
                self.statusLabel.stringValue = error ?? "Something went wrong."
            }
        }
    }

    @objc private func revealMeeting() {
        if let url = meetingURL {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        }
    }

    private func updateStatus() {
        if SourceCapture.helperURL() == nil {
            statusLabel.stringValue = "momr-audio helper not found — put it next to MomrApp or on PATH."
        } else {
            statusLabel.stringValue = "Listening: speak and play sound to move the meters."
        }
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
