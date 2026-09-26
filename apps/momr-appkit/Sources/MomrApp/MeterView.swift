import AppKit

/// A horizontal level meter: 0..1 fill in the accent color with a peak-hold
/// tick that falls slowly. Redrawn from the main thread only.
final class MeterView: NSView {
    /// The level to show, clamped to 0..1; a louder one lifts the peak tick.
    var level: CGFloat = 0 {
        didSet {
            level = min(max(level, 0), 1)
            peak = max(peak, level)
            needsDisplay = true
        }
    }

    private var peak: CGFloat = 0
    /// Lets the peak tick fall; runs only while the view is in a window.
    private var peakTimer: Timer?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
    }

    @available(*, unavailable)
    required init?(coder _: NSCoder) {
        fatalError("no storyboards")
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        peakTimer?.invalidate()
        peakTimer = nil
        guard window != nil else { return }
        peakTimer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            guard let self, peak > level else { return }
            peak = max(level, peak - 0.02)
            needsDisplay = true
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        let bar = bounds.insetBy(dx: 0, dy: (bounds.height - 10) / 2)
        // Track.
        context.setFillColor(NSColor.controlBackgroundColor.cgColor)
        context.fill(bar)
        // Fill.
        let fill = NSRect(x: bar.minX, y: bar.minY, width: bar.width * level, height: bar.height)
        context.setFillColor(NSColor.controlAccentColor.cgColor)
        context.fill(fill)
        // Peak tick.
        let tick = NSRect(x: bar.minX + bar.width * peak - 1, y: bar.minY - 2, width: 2, height: bar.height + 4)
        context.setFillColor(NSColor.labelColor.cgColor)
        context.fill(tick)
    }
}
