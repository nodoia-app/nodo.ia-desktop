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
let image = NSImage(size: size)
image.lockFocus()
NSGraphicsContext.current?.imageInterpolation = .high
NSColor.clear.setFill()
NSRect(origin: .zero, size: size).fill()
// Same visual as public/images/nodo-app-icon.png (squircle-ish rounded rect).
let radius = dim * 0.2237
let path = NSBezierPath(roundedRect: NSRect(origin: .zero, size: size), xRadius: radius, yRadius: radius)
path.addClip()
src.draw(
  in: NSRect(origin: .zero, size: size),
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
