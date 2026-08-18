import AppKit

let root = URL(fileURLWithPath: CommandLine.arguments[1])
let srcURL = root.appendingPathComponent("app-icon.png")
let dstURL = root.appendingPathComponent("app-icon-rounded.png")

guard let src = NSImage(contentsOf: srcURL) else {
  fputs("failed to load \(srcURL.path)\n", stderr)
  exit(1)
}

let dim: CGFloat = 1024
let size = NSSize(width: dim, height: dim)
// Shrink the whole icon plate (Dock/Finder), not the mark inside it.
let plateScale: CGFloat = 0.82
let plateDim = dim * plateScale
let origin = (dim - plateDim) / 2
let plateRect = NSRect(x: origin, y: origin, width: plateDim, height: plateDim)
let radius = plateDim * 0.2237

let image = NSImage(size: size)
image.lockFocus()
NSGraphicsContext.current?.imageInterpolation = .high
NSColor.clear.setFill()
NSRect(origin: .zero, size: size).fill()
let path = NSBezierPath(roundedRect: plateRect, xRadius: radius, yRadius: radius)
path.addClip()
src.draw(
  in: plateRect,
  from: NSRect(origin: .zero, size: src.size),
  operation: .copy,
  fraction: 1
)
image.unlockFocus()

guard
  let tiff = image.tiffRepresentation,
  let rep = NSBitmapImageRep(data: tiff),
  let png = rep.representation(using: .png, properties: [:])
else {
  fputs("failed to encode png\n", stderr)
  exit(1)
}

try png.write(to: dstURL)
print("wrote \(dstURL.path)")
