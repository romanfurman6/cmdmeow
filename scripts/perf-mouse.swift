// Moves the mouse in a circle for scripts/perf.sh:
//   swift perf-mouse.swift <center x> <center y> <radius> <seconds>
// ~120 moves a second, like a trackpad.
import CoreGraphics
import Foundation

let a = CommandLine.arguments
let cx = Double(a[1])!, cy = Double(a[2])!, r = Double(a[3])!, secs = Double(a[4])!
let start = Date()
var angle = 0.0
while Date().timeIntervalSince(start) < secs {
    let p = CGPoint(x: cx + r * cos(angle), y: cy + r * 0.6 * sin(angle))
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
    angle += 0.04
    usleep(8_000)
}
