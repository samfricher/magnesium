// Draw the app icon, a synapse: the end of one nerve cell sending, the next
// one's end receiving, and three dots crossing the gap between them. Writes
// every icon file the app ships:
//
//   swift app/scripts/make_icon.swift
//
// Under src-tauri/icons/: Icon.icon (the Icon Composer document: the colour
// behind and one SVG per shape), Assets.car (compiled from it by actool), and
// icon.icns, icon.ico and the PNGs tauri.conf.json lists, which are macOS's own
// drawing of Assets.car. Also src-tauri/app-icon.png, which the page harness
// attaches as a photo.
//
// It needs macOS 26 or later, whose drawing of Assets.car has the glass, and
// Xcode 26 or later, for actool.
import AppKit

let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let tauri = app.appendingPathComponent("src-tauri")
let icons = tauri.appendingPathComponent("icons")
let fm = FileManager.default

// MARK: - The drawing, on a 1024 canvas whose tile is 824 square at 100, y down

/// A point given along the synapse: u runs from the bottom-left corner (the
/// receiving end) to the top-right corner (the sending end), v across.
func axis(_ u: CGFloat, _ v: CGFloat) -> CGPoint {
  let k: CGFloat = 0.70710678
  return CGPoint(x: 512 + (u + v) * k, y: 512 + (v - u) * k)
}

/// The sending end: a round bulb facing the gap, narrowing to a stalk that runs
/// off the top-right corner.
func sending() -> CGPath {
  let f: CGFloat = 126, h: CGFloat = 182, s: CGFloat = 111
  let p = CGMutablePath()
  p.move(to: axis(900, -s))
  p.addLine(to: axis(f + 2.0 * h, -s))
  p.addCurve(to: axis(f + 0.95 * h, -h), control1: axis(f + 1.5 * h, -s), control2: axis(f + 1.4 * h, -h))
  p.addCurve(to: axis(f, 0), control1: axis(f + 0.4 * h, -h), control2: axis(f, -0.62 * h))
  p.addCurve(to: axis(f + 0.95 * h, h), control1: axis(f, 0.62 * h), control2: axis(f + 0.4 * h, h))
  p.addCurve(to: axis(f + 2.0 * h, s), control1: axis(f + 1.4 * h, h), control2: axis(f + 1.5 * h, s))
  p.addLine(to: axis(900, s))
  p.closeSubpath()
  return p
}

/// The receiving end: a bell whose cupped face meets the gap, narrowing to a
/// stalk that runs off the bottom-left corner.
func receiving() -> CGPath {
  let r: CGFloat = -122, h: CGFloat = 214, s: CGFloat = 114, cup: CGFloat = 64
  let p = CGMutablePath()
  p.move(to: axis(-900, -s))
  p.addLine(to: axis(r - 2.0 * h, -s))
  p.addCurve(to: axis(r - 0.25 * h, -h), control1: axis(r - 1.4 * h, -s), control2: axis(r - 0.9 * h, -h))
  p.addCurve(to: axis(r, -0.8 * h), control1: axis(r - 0.05 * h, -h), control2: axis(r, -0.92 * h))
  p.addCurve(to: axis(r, 0.8 * h), control1: axis(r - cup, -0.4 * h), control2: axis(r - cup, 0.4 * h))
  p.addCurve(to: axis(r - 0.25 * h, h), control1: axis(r, 0.92 * h), control2: axis(r - 0.05 * h, h))
  p.addCurve(to: axis(r - 2.0 * h, s), control1: axis(r - 0.9 * h, h), control2: axis(r - 1.4 * h, s))
  p.addLine(to: axis(-900, s))
  p.closeSubpath()
  return p
}

/// The three dots in the gap: centre and radius.
let dots: [(CGPoint, CGFloat)] = [(axis(6, -118), 40), (axis(-6, 0), 40), (axis(6, 118), 40)]

// MARK: - The Icon Composer document and Assets.car

// macOS 26 and later draw an Icon Composer icon as its document says. Given
// only an .icns, macOS 27 makes its own glass from it: it shaded these white
// shapes and lit a bright edge where they meet the tile.

/// SVG path data for a path, moved from the 824 tile onto the document's
/// canvas, which is the tile itself at 1024.
func svgPath(_ p: CGPath) -> String {
  var d: [String] = []
  func at(_ q: CGPoint) -> String { String(format: "%.2f %.2f", (q.x - 100) * 1024 / 824, (q.y - 100) * 1024 / 824) }
  p.applyWithBlock { e in
    let pts = e.pointee.points
    switch e.pointee.type {
    case .moveToPoint: d.append("M " + at(pts[0]))
    case .addLineToPoint: d.append("L " + at(pts[0]))
    case .addQuadCurveToPoint: d.append("Q " + at(pts[0]) + " " + at(pts[1]))
    case .addCurveToPoint: d.append("C " + at(pts[0]) + " " + at(pts[1]) + " " + at(pts[2]))
    case .closeSubpath: d.append("Z")
    @unknown default: break
    }
  }
  return d.joined(separator: " ")
}

func svg(_ body: String) -> String {
  "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1024\" height=\"1024\" viewBox=\"0 0 1024 1024\">\(body)</svg>\n"
}

func write(_ text: String, _ url: URL) {
  do { try text.write(to: url, atomically: true, encoding: .utf8) } catch { fatalError("could not write \(url.path): \(error)") }
}

let document = icons.appendingPathComponent("Icon.icon")
try? fm.removeItem(at: document)
try! fm.createDirectory(at: document.appendingPathComponent("Assets"), withIntermediateDirectories: true)
write(svg("<path fill=\"#ffffff\" d=\"\(svgPath(receiving()))\"/>"), document.appendingPathComponent("Assets/receiving.svg"))
write(svg("<path fill=\"#ffffff\" d=\"\(svgPath(sending()))\"/>"), document.appendingPathComponent("Assets/sending.svg"))
write(svg(dots.map { c, r in
  String(format: "<circle fill=\"#ffffff\" cx=\"%.2f\" cy=\"%.2f\" r=\"%.2f\"/>", (c.x - 100) * 1024 / 824, (c.y - 100) * 1024 / 824, r * 1024 / 824)
}.joined()), document.appendingPathComponent("Assets/dots.svg"))
write("""
{
  "fill" : { "solid" : "srgb:0.05490,0.05490,0.06275,1.00000" },
  "groups" : [
    {
      "layers" : [
        { "glass" : true, "image-name" : "dots.svg", "name" : "dots" },
        { "glass" : true, "image-name" : "sending.svg", "name" : "sending" },
        { "glass" : true, "image-name" : "receiving.svg", "name" : "receiving" }
      ],
      "shadow" : { "kind" : "neutral", "opacity" : 0.5 },
      "specular" : true,
      "translucency" : { "enabled" : true, "value" : 0.4 }
    }
  ],
  "supported-platforms" : { "squares" : [ "macOS" ] }
}

""", document.appendingPathComponent("icon.json"))

// Compiled here and kept in the repository, so that a build on a Mac with an
// older Xcode still ships it: the bundler compiles a .icon only with actool 26
// or later and otherwise leaves it out without failing.
let work = fm.temporaryDirectory.appendingPathComponent("magnesium-icon-\(ProcessInfo.processInfo.processIdentifier)")
try? fm.removeItem(at: work)
try! fm.createDirectory(at: work, withIntermediateDirectories: true)
let actool = Process()
actool.executableURL = URL(fileURLWithPath: "/usr/bin/xcrun")
actool.arguments = ["actool", document.path, "--compile", work.path, "--platform", "macosx",
                    "--minimum-deployment-target", "12.0", "--app-icon", "Icon",
                    "--output-partial-info-plist", work.appendingPathComponent("partial.plist").path]
actool.standardOutput = FileHandle.nullDevice
try! actool.run()
actool.waitUntilExit()
guard actool.terminationStatus == 0, fm.fileExists(atPath: work.appendingPathComponent("Assets.car").path) else {
  FileHandle.standardError.write("make_icon: actool could not compile Icon.icon; it needs Xcode 26 or later, set up with xcodebuild -runFirstLaunch\n".data(using: .utf8)!)
  exit(1)
}
try? fm.removeItem(at: icons.appendingPathComponent("Assets.car"))
try! fm.copyItem(at: work.appendingPathComponent("Assets.car"), to: icons.appendingPathComponent("Assets.car"))
try? fm.removeItem(at: work)

// MARK: - The other icon files, as macOS draws Assets.car

// A throwaway app holding Assets.car, so that macOS draws the icon as the Dock
// does. Its path is new each run, since macOS keeps app icons by path.
let stub = fm.temporaryDirectory.appendingPathComponent("MagnesiumIcon-\(ProcessInfo.processInfo.processIdentifier).app")
try? fm.removeItem(at: stub)
try! fm.createDirectory(at: stub.appendingPathComponent("Contents/Resources"), withIntermediateDirectories: true)
try! fm.createDirectory(at: stub.appendingPathComponent("Contents/MacOS"), withIntermediateDirectories: true)
try! fm.copyItem(at: icons.appendingPathComponent("Assets.car"), to: stub.appendingPathComponent("Contents/Resources/Assets.car"))
let info: NSDictionary = [
  "CFBundleIdentifier": "dev.magnesium.icon-drawing", "CFBundleName": "Magnesium", "CFBundlePackageType": "APPL",
  "CFBundleIconName": "Icon", "CFBundleExecutable": "stub", "CFBundleInfoDictionaryVersion": "6.0",
]
info.write(toFile: stub.appendingPathComponent("Contents/Info.plist").path, atomically: true)
fm.createFile(atPath: stub.appendingPathComponent("Contents/MacOS/stub").path, contents: "#!/bin/sh\n".data(using: .utf8),
              attributes: [.posixPermissions: 0o755])
// macOS draws a new app's icon a moment after it is first asked for, and shows a
// dashed placeholder until then; so each size is drawn again until a point that
// is the black background in the icon comes out dark.
func png(px: Int) -> Data {
  for _ in 0..<80 {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: px, height: px)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current!.imageInterpolation = .high
    NSWorkspace.shared.icon(forFile: stub.path)
      .draw(in: NSRect(x: 0, y: 0, width: px, height: px), from: .zero, operation: .copy, fraction: 1)
    NSGraphicsContext.restoreGraphicsState()
    var sample = [Int](repeating: 0, count: 4)
    rep.getPixel(&sample, atX: px * 3 / 10, y: px * 3 / 10)
    if sample[3] > 250 && sample[0] < 40 {
      guard let data = rep.representation(using: .png, properties: [:]) else { fatalError("could not encode a \(px) px drawing") }
      return data
    }
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.25))
  }
  FileHandle.standardError.write("make_icon: macOS did not draw the icon at \(px) px within 20 s\n".data(using: .utf8)!)
  exit(1)
}

func write(_ data: Data, _ url: URL) {
  do { try data.write(to: url) } catch { fatalError("could not write \(url.path): \(error)") }
}

// The iconset has no 1x 16 or 32 px drawing (icon_16x16.png, icon_32x32.png):
// with either, macOS 27 draws the icon at that size shrunk onto a grey plate.
// Without them it makes those sizes from the @2x drawings.
let set = fm.temporaryDirectory.appendingPathComponent("magnesium-icon.iconset")
try? fm.removeItem(at: set)
try! fm.createDirectory(at: set, withIntermediateDirectories: true)
for (name, px) in [("16x16@2x", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256),
                   ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
  write(png(px: px), set.appendingPathComponent("icon_\(name).png"))
}
let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", "-o", icons.appendingPathComponent("icon.icns").path, set.path]
try! iconutil.run()
iconutil.waitUntilExit()
guard iconutil.terminationStatus == 0 else { fatalError("iconutil could not make icon.icns") }
try? fm.removeItem(at: set)

write(png(px: 32), icons.appendingPathComponent("32x32.png"))
write(png(px: 128), icons.appendingPathComponent("128x128.png"))
write(png(px: 256), icons.appendingPathComponent("128x128@2x.png"))
write(png(px: 1024), tauri.appendingPathComponent("app-icon.png"))

// The Windows icon: an ICO file holding PNG drawings.
let sizes = [16, 24, 32, 48, 64, 256]
let images = sizes.map { png(px: $0) }
var ico = Data()
func le16(_ v: Int) { ico.append(contentsOf: [UInt8(v & 255), UInt8(v >> 8 & 255)]) }
func le32(_ v: Int) { le16(v & 0xffff); le16(v >> 16) }
le16(0); le16(1); le16(sizes.count)
var offset = 6 + 16 * sizes.count
for (px, image) in zip(sizes, images) {
  ico.append(contentsOf: [UInt8(px % 256), UInt8(px % 256), 0, 0])  // 256 is written as 0
  le16(1); le16(32); le32(image.count); le32(offset)
  offset += image.count
}
images.forEach { ico.append($0) }
write(ico, icons.appendingPathComponent("icon.ico"))
try? fm.removeItem(at: stub)
print("make_icon: wrote the icon files under \(tauri.path)")
