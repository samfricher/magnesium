// Draw the app icon, a synapse: the end of one nerve cell sending, the next
// one's end receiving, and three dots crossing the gap between them. Writes
// every icon file the app ships:
//
//   swift app/scripts/make_icon.swift
//
// Under src-tauri/icons/: Icon.icon (the Icon Composer document: the colour
// behind and one SVG per shape), Assets.car (compiled from it by actool),
// icon.icns, icon.ico and the PNGs tauri.conf.json lists. Also
// src-tauri/app-icon.png, which the page harness attaches as a photo.
//
// It needs a Mac with Xcode 26 or later, for actool.
import AppKit

let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let tauri = app.appendingPathComponent("src-tauri")
let icons = tauri.appendingPathComponent("icons")
let fm = FileManager.default

// MARK: - The drawing, on a 1024 canvas whose tile is 824 square at 100, y down

let ground = CGColor(srgbRed: 14 / 255, green: 14 / 255, blue: 16 / 255, alpha: 1)
let white = CGColor(srgbRed: 1, green: 1, blue: 1, alpha: 1)

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

// MARK: - The flat drawing: icon.icns for macOS before 26, the PNGs, the ICO

/// Apple's tile: 824 of 1024, with continuous corners.
func tile() -> CGPath {
  let r = CGRect(x: 100, y: 100, width: 824, height: 824), radius: CGFloat = 185.4
  let corners: [(CGPoint, CGVector, CGVector)] = [
    (CGPoint(x: r.maxX, y: r.minY), CGVector(dx: 1, dy: 0), CGVector(dx: 0, dy: 1)),
    (CGPoint(x: r.maxX, y: r.maxY), CGVector(dx: 0, dy: 1), CGVector(dx: -1, dy: 0)),
    (CGPoint(x: r.minX, y: r.maxY), CGVector(dx: -1, dy: 0), CGVector(dx: 0, dy: -1)),
    (CGPoint(x: r.minX, y: r.minY), CGVector(dx: 0, dy: -1), CGVector(dx: 1, dy: 0)),
  ]
  let path = CGMutablePath()
  for (i, (c, d1, d2)) in corners.enumerated() {
    func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
      CGPoint(x: c.x - x * radius * d1.dx + y * radius * d2.dx, y: c.y - x * radius * d1.dy + y * radius * d2.dy)
    }
    if i == 0 { path.move(to: pt(1.52866483, 0)) } else { path.addLine(to: pt(1.52866483, 0)) }
    path.addCurve(to: pt(0.66993427, 0.06549600), control1: pt(1.08849323, 0), control2: pt(0.86840689, 0.02189799))
    path.addLine(to: pt(0.63149399, 0.07491100))
    path.addCurve(to: pt(0.07491100, 0.63149399), control1: pt(0.37282392, 0.16905899), control2: pt(0.16905899, 0.37282392))
    path.addLine(to: pt(0.06549600, 0.66993427))
    path.addCurve(to: pt(0, 1.52866483), control1: pt(0.02189799, 0.86840689), control2: pt(0, 1.08849323))
  }
  path.closeSubpath()
  return path
}

func png(px: Int) -> Data {
  let c = CGContext(data: nil, width: px, height: px, bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
  c.translateBy(x: 0, y: CGFloat(px))
  c.scaleBy(x: CGFloat(px) / 1024, y: -CGFloat(px) / 1024)
  c.addPath(tile()); c.setFillColor(ground); c.fillPath()
  c.addPath(tile()); c.clip()
  c.setFillColor(white)
  c.addPath(receiving()); c.fillPath()
  c.addPath(sending()); c.fillPath()
  for (p, r) in dots { c.fillEllipse(in: CGRect(x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r)) }
  let data = NSMutableData()
  let out = CGImageDestinationCreateWithData(data, "public.png" as CFString, 1, nil)!
  CGImageDestinationAddImage(out, c.makeImage()!, nil)
  guard CGImageDestinationFinalize(out) else { fatalError("could not encode a \(px) px drawing") }
  return data as Data
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
print("make_icon: wrote the icon files under \(tauri.path)")
