// Prints the CoreGraphics window id of the VS Code window whose title contains argv[1].
// Needs Screen Recording permission, or macOS hides window titles.
import CoreGraphics
import Foundation

let needle = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ""
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in windows {
    let owner = w[kCGWindowOwnerName as String] as? String ?? ""
    let title = w[kCGWindowName as String] as? String ?? ""
    let layer = w[kCGWindowLayer as String] as? Int ?? -1
    if layer == 0, owner.contains("Code") || owner.contains("Electron"), title.contains(needle),
       let id = w[kCGWindowNumber as String] as? Int {
        print(id)
        exit(0)
    }
}
exit(1)
