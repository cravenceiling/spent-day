import AppKit
import IOKit.pwr_mgt

/// Power assertions by owning pid, as `IOPMCopyAssertionsByProcess` returns them.
typealias AssertionsByProcess = [NSNumber: [[String: Any]]]

enum Presence {
    static let idleThreshold: TimeInterval = 180
    /// A display assertion stops counting as presence after this long without input,
    /// so a video left looping or a stuck assertion can't record a whole night.
    static let heldAwakeCap: TimeInterval = 2 * 3600

    static func lastInputMs(nowMs: Int64) -> Int64 {
        let anyInput = CGEventType(rawValue: ~0)!
        let idle = CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: anyInput)
        return nowMs - Int64(idle * 1000)
    }

    static func frontAppHoldsDisplayAwake() -> Bool {
        guard let front = NSWorkspace.shared.frontmostApplication else { return false }
        return holdsDisplayAwake(pid: front.processIdentifier, in: currentAssertions())
    }

    static func currentAssertions() -> AssertionsByProcess {
        var raw: Unmanaged<CFDictionary>?
        guard IOPMCopyAssertionsByProcess(&raw) == kIOReturnSuccess,
              let byProcess = raw?.takeRetainedValue() as? AssertionsByProcess else { return [:] }
        return byProcess
    }

    /// Whether `pid` keeps the display awake, by its own assertion or one another
    /// process (WebKit's media process for Safari) declared on its behalf.
    static func holdsDisplayAwake(pid: pid_t, in assertions: AssertionsByProcess) -> Bool {
        let types: Set<String> = ["PreventUserIdleDisplaySleep", "NoDisplaySleepAssertion"]
        return assertions.contains { owner, list in
            list.contains { assertion in
                guard types.contains(assertion[kIOPMAssertionTypeKey] as? String ?? "") else { return false }
                let onBehalfOf = (assertion["AssertionOnBehalfOfPID"] as? NSNumber)?.int32Value
                return owner.int32Value == pid || onBehalfOf == pid
            }
        }
    }
}

/// When the user was last seen: their last input, or later, the last check at
/// which the app in front held the display awake, up to `heldAwakeCap` after that input.
struct LastSeen {
    private var heldAwakeAtMs: Int64?

    /// When the user left, or `nil` while they count as present. No input for
    /// `idleThreshold` counts as away, unless the app in front holds the display
    /// awake (a playing video), in which case the user is still watching.
    /// Call it on every tick: a video that ends before the threshold still counts.
    mutating func awaySince(nowMs: Int64, lastInputMs: Int64, frontHoldsDisplay: () -> Bool) -> Int64? {
        let threshold = Int64(Presence.idleThreshold * 1000)
        if nowMs - lastInputMs <= Int64(Presence.heldAwakeCap * 1000), frontHoldsDisplay() {
            heldAwakeAtMs = nowMs
        }
        let seen = max(lastInputMs, heldAwakeAtMs ?? lastInputMs)
        return nowMs - seen > threshold ? seen : nil
    }
}
