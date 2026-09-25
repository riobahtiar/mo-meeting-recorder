// Renders data/icon/icon-1024.png, the MOM Recorder master icon: a deep-ink
// squircle with the two tracks (blue mic above, orange computer below) and a
// red recording dot. Geometric and flat, so it stays legible at 16 px.
// Run: swift data/icon/render.swift
import AppKit

let size = CGFloat(1024)
let image = NSImage(size: NSSize(width: size, height: size))
image.lockFocus()
guard let ctx = NSGraphicsContext.current?.cgContext else { fatalError("no context") }

// Squircle background with a top-lit gradient.
let corner = size * 0.225
let body = NSBezierPath(roundedRect: NSRect(x: 0, y: 0, width: size, height: size),
                        xRadius: corner, yRadius: corner)
ctx.saveGState()
body.addClip()
let bg = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(),
                    colors: [CGColor(red: 0.13, green: 0.13, blue: 0.17, alpha: 1),
                             CGColor(red: 0.07, green: 0.07, blue: 0.11, alpha: 1)] as CFArray,
                    locations: [0, 1])!
ctx.drawLinearGradient(bg, start: CGPoint(x: 0, y: size), end: CGPoint(x: 0, y: 0), options: [])
// Top highlight for depth.
ctx.setFillColor(CGColor(gray: 1, alpha: 0.07))
ctx.fill(CGRect(x: 0, y: size * 0.55, width: size, height: size * 0.45))
ctx.restoreGState()

// The two tracks: round bars, mic blue above, computer orange below.
func bar(y: CGFloat, width: CGFloat, color: CGColor) {
    let h = size * 0.105
    let rect = NSRect(x: (size - width) / 2, y: y, width: width, height: h)
    let path = NSBezierPath(roundedRect: rect, xRadius: h / 2, yRadius: h / 2)
    ctx.setFillColor(color)
    path.fill()
}
bar(y: size * 0.52, width: size * 0.62, color: CGColor(red: 0.04, green: 0.52, blue: 1, alpha: 1))
bar(y: size * 0.36, width: size * 0.48, color: CGColor(red: 1, green: 0.62, blue: 0.04, alpha: 1))

// Recording dot, top right.
let dotR = size * 0.052
let dot = NSRect(x: size * 0.72 - dotR, y: size * 0.68 - dotR, width: dotR * 2, height: dotR * 2)
ctx.setFillColor(CGColor(red: 1, green: 0.23, blue: 0.19, alpha: 1))
ctx.fillEllipse(in: dot)

image.unlockFocus()
let rep = NSBitmapImageRep(data: image.tiffRepresentation!)!
try rep.representation(using: .png, properties: [:])!.write(
    to: URL(fileURLWithPath: "data/icon/icon-1024.png"))
print("wrote data/icon/icon-1024.png")
