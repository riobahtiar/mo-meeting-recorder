import AppKit

/// The one window: two live meters (microphone above, computer below), a
/// status line, Start, and while recording the clock with Pause and Stop.
/// The done page and Settings arrive in later slices.
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var window: NSWindow!
    private let mic = SourceCapture(.mic)
    private let computer = SourceCapture(.system)
    private let micMeter = MeterView()
    private let computerMeter = MeterView()
    private let statusLabel = NSTextField(labelWithString: "")
    private let clockLabel = NSTextField(labelWithString: "")
    private let startButton = NSButton(title: "Start Recording", target: nil, action: nil)
    private let pauseButton = NSButton(title: "Pause", target: nil, action: nil)
    private let stopButton = NSButton(title: "Stop", target: nil, action: nil)
    private let revealButton = NSButton(title: "Reveal in Finder", target: nil, action: nil)
    /// Which sides the next recording keeps.
    private let sourcesPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    /// Voice enhancement for the saved audio; the transcript uses the original (D26).
    private let enhanceBox = NSButton(checkboxWithTitle: "Voice enhancement", target: nil, action: nil)
    private var micBlock: NSStackView!
    private var computerBlock: NSStackView!
    private var recorder: Recorder?
    private var clockTimer: Timer?
    private var meetingURL: URL?
    /// What the status line says while no recording runs: the last outcome
    /// (saved, failed, refused) or nil for the listening hint.
    private var message: String?
    /// Problems this recording hit that stay on screen until the next
    /// Start: a failed write or resume lost audio, which a later good chunk
    /// does not bring back.
    private var problems: [String] = []
    /// Stop is finishing the meeting; quitting now would cut `momr finish`.
    private var finishing = false
    /// Quit was chosen while recording or finishing: quit once saved.
    private var quitWhenDone = false

    func applicationDidFinishLaunching(_: Notification) {
        buildMenu()
        buildWindow()
        mic.onLevel = { [weak self] in self?.micMeter.level = CGFloat($0) }
        computer.onLevel = { [weak self] in self?.computerMeter.level = CGFloat($0) }
        mic.onNote = { [weak self] _ in self?.updateStatus() }
        computer.onNote = { [weak self] _ in self?.updateStatus() }
        mic.onWriteError = { [weak self] in self?.report($0) }
        computer.onWriteError = { [weak self] in self?.report($0) }
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

    /// ⌘Q or the last window closing mid-recording would leave the meeting
    /// in the cache for a recovery this shell does not have yet. Ask first,
    /// and offer to save it on the way out.
    func applicationShouldTerminate(_: NSApplication) -> NSApplication.TerminateReply {
        if finishing {
            quitWhenDone = true
            show("Saving the meeting; MOM Recorder quits when it is done.")
            return .terminateCancel
        }
        guard recorder != nil else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = "A recording is running"
        alert.informativeText = "Stop and save it before quitting?"
        alert.addButton(withTitle: "Stop and Quit")
        alert.addButton(withTitle: "Keep Recording")
        guard alert.runModal() == .alertFirstButtonReturn else { return .terminateCancel }
        quitWhenDone = true
        stopRecording()
        return .terminateCancel
    }

    /// Closing the window is quitting (the app has no other window), so it
    /// goes through the same question instead of vanishing mid-recording.
    func windowShouldClose(_: NSWindow) -> Bool {
        guard recorder != nil || finishing else { return true }
        NSApp.terminate(nil)
        return false
    }

    // MARK: - Window

    private func buildWindow() {
        let content = NSView()
        content.translatesAutoresizingMaskIntoConstraints = false

        let title = NSTextField(labelWithString: "MOM Recorder")
        title.font = .systemFont(ofSize: 20, weight: .semibold)
        let micLabel = NSTextField(labelWithString: "Microphone")
        let computerLabel = NSTextField(labelWithString: "Computer audio")
        sourcesPopup.addItems(withTitles: Sources.allCases.map(\.label))
        sourcesPopup.selectItem(at: Sources.allCases.firstIndex(of: Sources.saved()) ?? 0)
        sourcesPopup.target = self
        sourcesPopup.action = #selector(sourcesChanged)
        let sourcesRow = NSStackView(views: [NSTextField(labelWithString: "Record:"), sourcesPopup])
        sourcesRow.spacing = 8
        enhanceBox.state = (SavedSettings.value("enhance") as? Bool ?? false) ? .on : .off
        enhanceBox.toolTip = "Less noise and clearer voices in the saved audio; the transcript uses the original."
        enhanceBox.target = self
        enhanceBox.action = #selector(enhanceChanged)
        micBlock = NSStackView(views: [micLabel, micMeter])
        computerBlock = NSStackView(views: [computerLabel, computerMeter])
        for block in [micBlock!, computerBlock!] {
            block.orientation = .vertical
            block.alignment = .leading
            block.spacing = 4
        }
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
        let stack = NSStackView(views: [title, clockLabel, sourcesRow, enhanceBox, micBlock, computerBlock, statusLabel, startButton, pauseButton, stopButton, revealButton])
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
            micBlock.widthAnchor.constraint(equalTo: stack.widthAnchor),
            computerBlock.widthAnchor.constraint(equalTo: stack.widthAnchor),
            micMeter.widthAnchor.constraint(equalTo: micBlock.widthAnchor),
            computerMeter.widthAnchor.constraint(equalTo: computerBlock.widthAnchor),
        ])

        window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 320),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "MOM Recorder"
        window.delegate = self
        window.contentView = content
        window.center()
        sourcesChanged()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
    }

    @objc private func startRecording() {
        // A track whose capture is down records nothing; with both down the
        // meeting would be silence under a running clock.
        let sources = selectedSources
        // A kept side whose capture is down records nothing; with every
        // kept side down the meeting would be silence under a running clock.
        let kept = [(Source.mic, mic), (.system, computer)].filter { sources.records($0.0) }
        guard kept.contains(where: { $0.1.isRunning }) else {
            show("Nothing that would be recorded is capturing.")
            return
        }
        // Checked now, not at Stop: finding out after an hour's meeting that
        // the tool to save it is missing would be too late.
        guard Tools.url("momr") != nil else {
            show("momr not found — put it next to MomrApp, in /opt/homebrew/bin or on PATH.")
            return
        }
        let recorder: Recorder
        do {
            recorder = try Recorder(
                mic: mic, computer: computer, title: "Meeting", sources: sources,
                enhance: enhanceBox.state == .on)
        } catch {
            show("Could not start recording: \(error.localizedDescription)")
            return
        }
        self.recorder = recorder
        meetingURL = nil
        message = nil
        problems = []
        sourcesPopup.isEnabled = false
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
        clockLabel.stringValue = s >= 3600
            ? String(format: "%d:%02d:%02d", s / 3600, s / 60 % 60, s % 60)
            : String(format: "%02d:%02d", s / 60, s % 60)
        window.title = recorder.paused ? "Paused" : "● Recording"
        updateStatus()
    }

    private var selectedSources: Sources {
        Sources.allCases[max(sourcesPopup.indexOfSelectedItem, 0)]
    }

    @objc private func enhanceChanged() {
        recorder?.enhance = enhanceBox.state == .on
    }

    /// Dims the meter of a side the next recording will not keep; it still
    /// moves, so the user sees that side is live.
    @objc private func sourcesChanged() {
        let sources = selectedSources
        micBlock.alphaValue = sources.records(.mic) ? 1 : 0.4
        computerBlock.alphaValue = sources.records(.system) ? 1 : 0.4
    }

    /// Each source's reason for not capturing, if any.
    private var notes: [String] {
        [mic.note, computer.note].compactMap { $0 }
    }

    @objc private func togglePause() {
        guard let recorder else { return }
        if recorder.paused {
            if let problem = recorder.resume() {
                report(problem)
            }
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
        finishing = true
        show("Finishing: encoding and transcribing…")
        let lost = problems
        recorder.stop(title: "Meeting") { [weak self] outcome in
            guard let self else { return }
            self.finishing = false
            self.sourcesPopup.isEnabled = true
            self.startButton.isHidden = false
            self.clockLabel.isHidden = true
            // The folder is offered whenever it exists: it holds the audio
            // even when the transcript failed.
            self.meetingURL = outcome.folder
            self.revealButton.isHidden = outcome.folder == nil
            var parts: [String] = []
            if let folder = outcome.folder {
                parts.append(outcome.problem == nil ? "Saved \(folder.lastPathComponent)." : "Saved \(folder.lastPathComponent), but:")
            }
            parts += lost
            if let problem = outcome.problem {
                parts.append(problem)
            }
            self.problems = []
            self.show(parts.joined(separator: " "))
            if self.quitWhenDone {
                NSApp.terminate(nil)
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
        var parts = problems + notes
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

    /// A problem that cost this recording audio: kept on screen, and said
    /// again when the meeting is saved.
    private func report(_ problem: String) {
        if !problems.contains(problem) {
            problems.append(problem)
        }
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
