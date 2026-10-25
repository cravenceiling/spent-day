// macOS helpers for scripts/e2e-cross-device.sh.
//
//   e2e-mac-helper window-id PID   prints the number of PID's first on-screen window
//   e2e-mac-helper hold-front      until killed, holds a display-sleep assertion on behalf of
//                                  whichever app is in front
//
// The Spent Day app counts the user as away after 3 minutes without input, unless the app in
// front holds the display awake, as a playing video does. hold-front lets the test run on a
// Mac nobody is touching. It works up to 2 hours after the last real input (heldAwakeCap).
import AppKit
import IOKit.pwr_mgt

let arguments = CommandLine.arguments
switch arguments.count > 1 ? arguments[1] : "" {
case "window-id" where arguments.count == 3:
    let pid = Int32(arguments[2])!
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    if let window = windows.first(where: {
        $0[kCGWindowOwnerPID as String] as? Int32 == pid && $0[kCGWindowLayer as String] as? Int == 0
    }) {
        print(window[kCGWindowNumber as String] as! Int)
    }
case "hold-front":
    signal(SIGTERM) { _ in exit(0) }
    var held: (pid: pid_t, id: IOPMAssertionID)?
    while true {
        let front = NSWorkspace.shared.frontmostApplication?.processIdentifier
        if front != held?.pid {
            if let old = held { IOPMAssertionRelease(old.id) }
            held = nil
            if let pid = front {
                let properties: [String: Any] = [
                    kIOPMAssertionTypeKey: kIOPMAssertionTypePreventUserIdleDisplaySleep,
                    kIOPMAssertionNameKey: "Spent Day e2e: keep the user present",
                    "AssertionOnBehalfOfPID": NSNumber(value: pid),
                ]
                var id: IOPMAssertionID = 0
                if IOPMAssertionCreateWithProperties(properties as CFDictionary, &id) == kIOReturnSuccess {
                    held = (pid, id)
                }
            }
        }
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.25))
    }
default:
    FileHandle.standardError.write("usage: e2e-mac-helper window-id PID | hold-front\n".data(using: .utf8)!)
    exit(2)
}
