import AppKit

var failures = 0
@MainActor func check(_ ok: Bool, _ what: String) {
    print(ok ? "ok    \(what)" : "FAIL  \(what)")
    if !ok { failures += 1 }
}

// Recorded from IOPMCopyAssertionsByProcess while `caffeinate -d` ran (pid 92255).
let recorded: AssertionsByProcess = [
    92255: [[
        "AssertType": "PreventUserIdleDisplaySleep",
        "AssertionTrueType": "PreventUserIdleDisplaySleep",
        "AssertName": "caffeinate command-line tool",
        "Process Name": "caffeinate",
        "AssertPID": 92255,
        "AssertLevel": 255,
    ]],
    342: [[
        "AssertType": "PreventUserIdleSystemSleep",
        "AssertName": "Powerd - Prevent sleep while display is on",
        "Process Name": "powerd",
        "AssertPID": 342,
    ]],
    401: [[
        "AssertType": "UserIsActive",
        "Process Name": "WindowServer",
        "AssertPID": 401,
    ]],
]
check(Presence.holdsDisplayAwake(pid: 92255, in: recorded), "caffeinate -d holds the display for itself")
check(!Presence.holdsDisplayAwake(pid: 777, in: recorded), "caffeinate -d does not hold it for the app in front (pid 777)")
check(!Presence.holdsDisplayAwake(pid: 342, in: recorded), "a system-sleep assertion does not count")
check(!Presence.holdsDisplayAwake(pid: 401, in: recorded), "a user-activity assertion does not count")

let onBehalf: AssertionsByProcess = [
    500: [["AssertType": "PreventUserIdleDisplaySleep", "AssertionOnBehalfOfPID": 777]],
]
check(Presence.holdsDisplayAwake(pid: 777, in: onBehalf), "an assertion declared on behalf of the app in front counts")
check(!Presence.holdsDisplayAwake(pid: 778, in: onBehalf), "an assertion on behalf of another app does not")

let minute: Int64 = 60_000
var seen = LastSeen()
check(seen.awaySince(nowMs: 100 * minute, lastInputMs: 98 * minute, frontHoldsDisplay: { false }) == nil,
      "2 min without input is present")
check(seen.awaySince(nowMs: 100 * minute, lastInputMs: 96 * minute, frontHoldsDisplay: { false }) == 96 * minute,
      "4 min without input is away since the last input")

seen = LastSeen()
check(seen.awaySince(nowMs: 100 * minute, lastInputMs: 50 * minute, frontHoldsDisplay: { true }) == nil,
      "a video in front keeps the user present")
check(seen.awaySince(nowMs: 102 * minute, lastInputMs: 50 * minute, frontHoldsDisplay: { false }) == nil,
      "2 min after the video stops is still present")
check(seen.awaySince(nowMs: 104 * minute, lastInputMs: 50 * minute, frontHoldsDisplay: { false }) == 100 * minute,
      "4 min after the video stops is away since the video was last seen playing")
check(seen.awaySince(nowMs: 105 * minute, lastInputMs: 104 * minute, frontHoldsDisplay: { false }) == nil,
      "new input makes the user present again")

// Input at 0, a short video plays until 175 s, the next check at 185 s.
seen = LastSeen()
var awayFrom: Int64?
for now in stride(from: Int64(5_000), through: 400_000, by: 5_000) where awayFrom == nil {
    awayFrom = seen.awaySince(nowMs: now, lastInputMs: 0, frontHoldsDisplay: { now <= 175_000 })
    if now == 185_000 { check(awayFrom == nil, "10 s after a short video ends the user is present") }
}
check(awayFrom == 175_000, "a short video keeps the time up to its end at 175 s (got \(awayFrom ?? -1) ms)")

seen = LastSeen()
var awayAt: Int64?
for now in stride(from: 1 * minute, through: 200 * minute, by: Int(minute / 12)) where awayAt == nil {
    awayAt = seen.awaySince(nowMs: now, lastInputMs: 0, frontHoldsDisplay: { true })
}
check(awayAt == 120 * minute, "a display held awake for hours counts 2 h after the last input, then away (got \(awayAt.map { $0 / minute } ?? -1) min)")

try idleChecks()
try browserChecks()

// A sync server's own text, its error body or a redirect target, never reaches the UI.
let hostile = "Visit evil.example to fix your account"
for status: UInt16 in [301, 302, 400, 401, 403, 404, 413, 500, 503, 600] {
    let text = SpentDayError.Server(status: status, message: hostile).userMessage
    check(!text.contains("evil") && !text.contains("\(status)"), "a \(status) answer shows a fixed message (got \(text))")
    check(!SpentDayError.Server(status: status, message: hostile).logMessage.contains("evil"), "a \(status) answer keeps the body out of the log")
}
check(SpentDayError.Server(status: 401, message: hostile).userMessage == "The sync server didn't accept the sync code.",
      "a 401 says the code was not accepted")
check(SpentDayError.Server(status: 302, message: hostile).userMessage.contains("doesn't follow redirects"),
      "a redirect says Spent Day doesn't follow it")
check(!SpentDayError.Network("bad response from server: invalid type: string \"evil\"").userMessage.contains("evil"),
      "a network error keeps the server's answer out of the UI")
check(!SpentDayError.Network("bad response from server: invalid type: string \"evil\"").logMessage.contains("evil"),
      "a network error keeps the server's answer out of the log")

// The app's data folder is private, both a new one and one an older build left at 0755.
let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("spentday-tests-\(UUID().uuidString)")
let dataFolder = scratch.appendingPathComponent("SpentDay", isDirectory: true)
func folderMode() -> Int { (try? FileManager.default.attributesOfItem(atPath: dataFolder.path)[.posixPermissions] as? Int) ?? -1 }
try Launch.makePrivateFolder(dataFolder)
check(folderMode() == 0o700, "a new data folder is 0700 (got \(String(folderMode(), radix: 8)))")
try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dataFolder.path)
try Launch.makePrivateFolder(dataFolder)
check(folderMode() == 0o700, "an existing 0755 data folder becomes 0700 (got \(String(folderMode(), radix: 8)))")
// A folder that can't be made, because its parent is read-only or a file sits in its place, stops the app.
func folderFailure(_ folder: URL) -> String? {
    do { try Launch.makePrivateFolder(folder); return nil } catch { return error.userMessage }
}
let locked = scratch.appendingPathComponent("locked", isDirectory: true)
try FileManager.default.createDirectory(at: locked, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o500])
let underLocked = folderFailure(locked.appendingPathComponent("SpentDay"))
check(underLocked?.contains("couldn't make its folder \(locked.path)/SpentDay private") == true,
      "a data folder in a read-only folder is an error (got \(underLocked ?? "no error"))")
try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: locked.path)
let file = scratch.appendingPathComponent("file")
try Data().write(to: file)
check(folderFailure(file) != nil, "a file where the data folder goes is an error")
try FileManager.default.removeItem(at: scratch)
check(Launch.folderName == "SpentDay", "a normal build keeps its data in SpentDay")

// The tests are a normal build, like the installed app, so they must ignore the dev overrides.
setenv("SPENTDAY_DEMO", "1", 1)
check(!Launch.isDemo, "a normal build ignores SPENTDAY_DEMO")
unsetenv("SPENTDAY_DEMO")

// Live: a real `caffeinate -d` next to Finder.
let caffeinate = Process()
caffeinate.executableURL = URL(fileURLWithPath: "/usr/bin/caffeinate")
caffeinate.arguments = ["-d"]
try caffeinate.run()
Thread.sleep(forTimeInterval: 1)
let live = Presence.currentAssertions()
let finder = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.finder").first
check(Presence.holdsDisplayAwake(pid: caffeinate.processIdentifier, in: live), "live: caffeinate -d is seen")
if let finder {
    check(!Presence.holdsDisplayAwake(pid: finder.processIdentifier, in: live), "live: Finder in front is not held awake by caffeinate")
}
caffeinate.terminate()
caffeinate.waitUntilExit()

exit(failures == 0 ? 0 : 1)
