// momr-menubar: the recording status in the menu bar (plan 09). A pulsing
// dot, the clock and a two-lane waveform while recording, "paused" while
// paused, the percentage while transcribing, hidden otherwise. Clicking shows
// a menu with Pause, Stop and Compact. It does not run `momr watch`: it
// connects to the app's socket itself through MomrWatch, reads the same
// NDJSON state lines `momr watch` prints, and sends commands back over the
// same socket.
//
// No Dock icon: the activation policy is set to accessory at launch, so no
// bundle with LSUIElement is needed. Started by the app, it gets the app's
// pid in `MOMR_PARENT_PID` and quits when the app ends, however it ends
// (see ParentWatch.swift).
//
// Strings in English and Indonesian. The app passes its own UI language in
// `MOMR_LANG` (`en` or `id`) when it spawns the item, so the menu matches the
// window even when the app's language differs from the system's. Started by
// hand, without it, the item falls back to the first of AppleLanguages. The
// choice is made once at launch, since neither source changes under a
// running item. (The Rust app holds the full tables in src/locales.rs; the
// status item needs its dozen.)

/// Whether to speak Indonesian, from `MOMR_LANG` and then the system.
private func menubarUsesIndonesian(
    environment: [String: String], appleLanguages: [String]?
) -> Bool {
    switch environment["MOMR_LANG"] {
    case "id": return true
    case "en": return false
    default: return appleLanguages?.first?.hasPrefix("id") ?? false
    }
}

private let indonesian = menubarUsesIndonesian(
    environment: ProcessInfo.processInfo.environment,
    appleLanguages: UserDefaults.standard.array(forKey: "AppleLanguages")
        as? [String])

private let menubarTable: [String: (en: String, id: String)] = [
    "show": ("Show MOM Recorder", "Tampilkan MOM Recorder"),
    "pause": ("Pause", "Jeda"),
    "resume": ("Resume", "Lanjutkan"),
    "stop": ("Stop", "Hentikan"),
    "compact": ("Compact Strip", "Strip Ringkas"),
    "quit": ("Quit Item", "Keluar"),
    "paused": ("paused", "dijeda"),
]

private func menubarText(_ key: String) -> String {
    guard let entry = menubarTable[key] else { return key }
    return indonesian ? entry.id : entry.en
}

import AppKit
import MomrWatch

final class StatusView: NSView {
    var history = LevelHistory()
    var recording = false

    override func draw(_ dirtyRect: NSRect) {
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        let mid = bounds.height / 2
        context.setFillColor(NSColor.controlAccentColor.cgColor)
        let n = history.mic.count
        guard n > 0 else { return }
        let step = bounds.width / CGFloat(max(n, 1))
        for (i, level) in history.mic.enumerated() {
            let h = CGFloat(level) * (mid - 1)
            context.fill(CGRect(x: CGFloat(i) * step, y: mid - h, width: max(step - 0.5, 0.5), height: h))
        }
        for (i, level) in history.computer.enumerated() {
            let h = CGFloat(level) * (mid - 1)
            context.fill(CGRect(x: CGFloat(i) * step, y: mid, width: max(step - 0.5, 0.5), height: h))
        }
        if recording, Int(Date().timeIntervalSince1970) % 2 == 0 {
            context.setFillColor(NSColor.systemRed.cgColor)
            context.fillEllipse(in: CGRect(x: 1, y: bounds.height - 5, width: 4, height: 4))
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    let view = StatusView(frame: NSRect(x: 0, y: 0, width: 60, height: 18))
    var pauseItem: NSMenuItem?
    let client = WatchClient()
    var state = WatchState.off
    var timer: Timer?
    var parentWatch: DispatchSourceProcess?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        if let parent = parentPid(environment: ProcessInfo.processInfo.environment) {
            parentWatch = watchProcessExit(parent) { NSApp.terminate(nil) }
        }
        item.isVisible = false
        let menu = NSMenu()
        menu.addItem(NSMenuItem(title: menubarText("show"), action: #selector(showApp), keyEquivalent: ""))
        let pause = NSMenuItem(title: menubarText("pause"), action: #selector(togglePause), keyEquivalent: "")
        pauseItem = pause
        menu.addItem(pause)
        menu.addItem(NSMenuItem(title: menubarText("stop"), action: #selector(stop), keyEquivalent: ""))
        menu.addItem(NSMenuItem(title: menubarText("compact"), action: #selector(compact), keyEquivalent: ""))
        menu.addItem(.separator())
        menu.addItem(NSMenuItem(title: menubarText("quit"), action: #selector(quit), keyEquivalent: ""))
        item.menu = menu
        Thread.detachNewThread { [client] in client.run() }
        client.onState = { [weak self] state in
            DispatchQueue.main.async { self?.update(state) }
        }
        timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            self?.view.needsDisplay = true
        }
    }

    func clock(_ elapsed: Int) -> String {
        String(format: "%02d:%02d", elapsed / 60, elapsed % 60)
    }

    func update(_ state: WatchState) {
        let wasVisible = item.isVisible
        self.state = state
        switch state.state {
        case "recording":
            pauseItem?.title = menubarText("pause")
            view.history.append(mic: state.mic, computer: state.computer)
            view.recording = true
            item.button?.title = "● " + clock(state.elapsed)
        case "paused":
            view.recording = false
            pauseItem?.title = menubarText("resume")
            item.button?.title = "❚❚ " + menubarText("paused") + " " + clock(state.elapsed)
        case "transcribing":
            view.recording = false
            item.button?.title = "⟳ \(Int(state.progress * 100))%"
        case "done":
            view.recording = false
            view.history.clear()
            item.button?.title = "✓"
        default:
            view.recording = false
            view.history.clear()
            item.button?.title = ""
        }
        item.button?.image = waveImage()
        item.isVisible = !(state.state == "off" || state.state == "idle")
        if item.isVisible != wasVisible {
            view.needsDisplay = true
        }
    }

    func waveImage() -> NSImage? {
        guard !view.history.mic.isEmpty else { return nil }
        let image = NSImage(size: NSSize(width: 44, height: 18), flipped: false) { _ in
            self.view.draw(NSRect(x: 0, y: 0, width: 44, height: 18))
            return true
        }
        image.isTemplate = true
        return image
    }

    @objc func showApp() {
        if let app = NSRunningApplication.runningApplications(
            withBundleIdentifier: "io.github.riobahtiar.MOMRecorder").first
        {
            app.activate()
        }
    }

    @objc func togglePause() { WatchClient.send("pause") }
    @objc func stop() { WatchClient.send("stop") }
    @objc func compact() { WatchClient.send("compact") }
    @objc func quit() { NSApp.terminate(nil) }
}

let delegate = AppDelegate()
let app = NSApplication.shared
app.delegate = delegate
app.run()
