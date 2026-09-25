import AppKit

/// A horizontal level meter: 0..1 fill in the accent color with a peak-hold
/// tick that falls slowly. Redrawn from the main thread only.
final class MeterView: NSView {
    var level: CGFloat = 0 {
        didSet { needsDisplay = true }
    }

    private var peak: CGFloat = 0
    private var peakTimer: Timer?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        peakTimer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            guard let self else { return }
            if peak > level {
                peak = max(level, peak - 0.02)
                needsDisplay = true
            }
        }
    }

    @available(*, unavailable)
    required init?(coder _: NSCoder) {
        fatalError("no storyboards")
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        let bar = bounds.insetBy(dx: 0, dy: (bounds.height - 10) / 2)
        // Track.
        context.setFillColor(NSColor.controlBackgroundColor.cgColor)
        context.fill(bar)
        // Fill.
        let fill = NSRect(x: bar.minX, y: bar.minY, width: bar.width * min(max(level, 0), 1), height: bar.height)
        context.setFillColor(NSColor.controlAccentColor.cgColor)
        context.fill(fill)
        // Peak tick.
        let tick = NSRect(x: bar.minX + bar.width * min(max(peak, 0), 1) - 1, y: bar.minY - 2, width: 2, height: bar.height + 4)
        context.setFillColor(NSColor.labelColor.cgColor)
        context.fill(tick)
        if level > peak {
            peak = level
        }
    }
}
