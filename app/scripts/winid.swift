// winid <pid|name>: print "id width height" for each window of that process,
// at the normal level or a floating one (a window the evidence harness keeps
// above others sits on the floating level), but not the menu bar's.
import CoreGraphics
import Foundation
let arg = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ""
let pid = Int32(arg)
let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list {
  let owner = w[kCGWindowOwnerName as String] as? String ?? ""
  let opid = w[kCGWindowOwnerPID as String] as? Int32 ?? -1
  let layer = w[kCGWindowLayer as String] as? Int ?? -1
  guard layer >= 0 && layer < 20 else { continue }
  if let p = pid { if opid != p { continue } } else if owner != arg { continue }
  let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
  let id = w[kCGWindowNumber as String] as? Int ?? 0
  print("\(id) \(b["Width"] ?? 0) \(b["Height"] ?? 0) \(owner)")
}
