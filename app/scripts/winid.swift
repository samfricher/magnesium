// winid <pid|name>: print "id x y width height" for each on-screen window of
// that process, at the normal level or a floating one (a window the evidence
// harness keeps above others sits on the floating level), but not the menu
// bar's. Points, from the top left of the main display.
import CoreGraphics
import Foundation
let arg = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ""
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
