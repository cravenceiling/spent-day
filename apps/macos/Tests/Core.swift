import Foundation

/// A real core on a scratch database, deleted when the process exits.
func scratchCore() throws -> SpentDay {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("spentday-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    atexit_b { try? FileManager.default.removeItem(at: dir) }
    return try SpentDay.open(dbPath: dir.appendingPathComponent("spentday.db").path, deviceName: "Test Mac", platform: .macos)
}

/// A whole hour, an hour ago, so the timestamps look like real ones to the core.
let t0: Int64 = (Int64(Date.now.timeIntervalSince1970) / 3600 - 1) * 3_600_000

func totalMs(_ core: SpentDay) throws -> Int64 {
    try core.summary(query: SummaryQuery(startMs: t0 - 3_600_000, endMs: t0 + 3_600_000, bucketMs: 3_600_000, deviceId: nil)).totalMs
}

func page(_ url: String) -> WebPage { WebPage(url: url) }

@MainActor func idleChecks() throws {
    // No input after 0 while a page left open changes on its own at 120 s and 175 s.
    let core = try scratchCore()
    for at in stride(from: Int64(0), through: 185_000, by: 5_000) {
        let web = at < 120_000 ? page("https://a.example/") : at < 175_000 ? page("https://b.example/") : page("https://c.example/")
        try core.heartbeat(atMs: t0 + at, activity: Activity(appId: "com.google.Chrome", appName: "Chrome", web: web))
    }
    try core.awaySince(lastInputMs: t0, nowMs: t0 + 185_000)
    let total = try totalMs(core)
    check(total == 0, "away_since(0, 185 s) keeps no idle time across page changes at 120 s and 175 s (got \(total) ms)")
}
