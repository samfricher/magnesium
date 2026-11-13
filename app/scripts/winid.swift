// winid <pid|name>: print "id x y width height" for each on-screen window of
// that process, at the normal level or a floating one (a window the evidence
// harness keeps above others sits on the floating level), but not the menu
// bar's. Points, from the top left of the main display.
//
// winid crop <png> <x> <y> <width> <height>: cut a photograph down to that
// rectangle, in its own pixels, in place.
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let args = CommandLine.arguments
if args.count == 7 && args[1] == "crop" {
  let url = URL(fileURLWithPath: args[2]) as CFURL
  guard let source = CGImageSourceCreateWithURL(url, nil),
        let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
        let x = Int(args[3]), let y = Int(args[4]), let w = Int(args[5]), let h = Int(args[6]),
        let cut = image.cropping(to: CGRect(x: x, y: y, width: w, height: h)),
        let out = CGImageDestinationCreateWithURL(url, UTType.png.identifier as CFString, 1, nil)
  else { FileHandle.standardError.write("winid: could not crop \(args[2])\n".data(using: .utf8)!); exit(1) }
  CGImageDestinationAddImage(out, cut, nil)
  exit(CGImageDestinationFinalize(out) ? 0 : 1)
}

let arg = args.count > 1 ? args[1] : ""
let pid = Int32(arg)
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list {
  let owner = w[kCGWindowOwnerName as String] as? String ?? ""
  let opid = w[kCGWindowOwnerPID as String] as? Int32 ?? -1
  let layer = w[kCGWindowLayer as String] as? Int ?? -1
  guard layer >= 0 && layer < 20 else { continue }
  if let p = pid { if opid != p { continue } } else if owner != arg { continue }
  let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
  let id = w[kCGWindowNumber as String] as? Int ?? 0
  print("\(id) \(b["X"] ?? 0) \(b["Y"] ?? 0) \(b["Width"] ?? 0) \(b["Height"] ?? 0) \(owner)")
}
