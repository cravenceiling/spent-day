import Foundation

/// Fixed sample data for `snapshot.sh` and `SPENTDAY_DEMO=1` in dev builds, so the UI can be
/// built and checked without a working core or real history.
final class DemoStore: UsageStore, @unchecked Sendable {
    private let lock = NSLock()
    private var config: SyncConfig?

    private static let mac = DeviceInfo(id: "demo-mac", name: "MacBook Pro", platform: .macos, lastSeenMs: Date.now.millis)
    private static let phone = DeviceInfo(id: "demo-phone", name: "Pixel 9", platform: .android, lastSeenMs: Date.now.millis - 1_200_000)
    private static let laptop = DeviceInfo(id: "demo-linux", name: "ThinkPad", platform: .linux, lastSeenMs: Date.now.millis - 86_400_000)

    private static let items: [String: (String, ItemKind, String, [String])] = [
        "xcode": ("Xcode", .app, "com.apple.dt.Xcode", ["com.apple.dt.Xcode"]),
        "github": ("github.com", .site, "", []),
        "terminal": ("Terminal", .app, "com.apple.Terminal", ["com.apple.Terminal"]),
        "mail": ("Mail", .app, "com.apple.mail", ["com.apple.mail"]),
        "slack": ("Slack", .app, "slack", ["com.tinyspeck.slackmacgap"]),
        "messages": ("Messages", .app, "com.apple.MobileSMS", ["com.apple.MobileSMS"]),
        "instagram": ("Instagram", .app, "instagram", []),
        "youtube": ("YouTube", .app, "youtube", []),
        "youtubecom": ("youtube.com", .site, "", []),
        "chrome": ("Chrome", .app, "chrome", ["com.google.Chrome"]),
        "hn": ("news.ycombinator.com", .site, "", []),
        "vscode": ("Visual Studio Code", .app, "vscode", ["com.microsoft.VSCode"]),
    ]
    private static let blocks: [(String, String, Int, Int)] = [
        ("demo-phone", "messages", 432, 446),
        ("demo-phone", "instagram", 446, 468),
        ("demo-phone", "chrome", 500, 514),
        ("demo-mac", "mail", 529, 550),
        ("demo-mac", "xcode", 550, 655),
        ("demo-mac", "github", 655, 680),
        ("demo-mac", "xcode", 680, 755),
        ("demo-phone", "instagram", 760, 795),
        ("demo-phone", "messages", 795, 808),
        ("demo-mac", "slack", 810, 830),
        ("demo-mac", "xcode", 830, 940),
        ("demo-mac", "terminal", 940, 955),
        ("demo-mac", "github", 955, 990),
        ("demo-mac", "xcode", 990, 1065),
        ("demo-mac", "slack", 1065, 1085),
        ("demo-phone", "youtube", 1075, 1100),
        ("demo-linux", "hn", 1145, 1180),
        ("demo-linux", "vscode", 1180, 1225),
        ("demo-mac", "youtubecom", 1230, 1270),
        ("demo-phone", "instagram", 1250, 1265),
        ("demo-phone", "youtube", 1300, 1355),
        ("demo-phone", "instagram", 1355, 1378),
        ("demo-phone", "messages", 1400, 1418),
    ]

    static func colorSeed(for item: DayItem) -> TileLCH {
        // Frozen icon samples keep demo colours independent of the apps installed locally.
        let samples: [String: UInt32] = ["Xcode": 0x4090e7, "Mail": 0x235d99, "Visual Studio Code": 0x59a9e3,
            "Instagram": 0xd95a94, "Chrome": 0xd5ab34, "news.ycombinator.com": 0xee763f,
            "YouTube": 0xde5656, "youtube.com": 0xfda29c, "Messages": 0x1a5bb4, "Slack": 0x814d81,
            "github.com": 0x333333, "Terminal": 0x404040]
        let seed = samples[item.name].map { TileRGB(hex: $0).lch } ?? TileColors.hashed(item.key)
        return seed.c < 0.02 ? TileLCH(l: seed.l, c: 0, h: 0) : seed
    }

    func day(query: DayQuery) throws -> DayDetail {
        let calendar = Calendar.current
        let midnight = calendar.startOfDay(for: Date(millis: query.startMs))
        let devices = try devices().compactMap { device -> DeviceDay? in
            let own = Self.blocks.filter { $0.0 == device.id }
            let items = Dictionary(grouping: own, by: { $0.1 }).compactMap { id, blocks -> DayItem? in
                let (name, kind, appKey, appIds) = Self.items[id]!
                let sessions = blocks.compactMap { block -> Session? in
                    let from = calendar.date(bySettingHour: block.2 / 60, minute: block.2 % 60, second: 0, of: midnight)!.millis
                    let to = calendar.date(bySettingHour: block.3 / 60, minute: block.3 % 60, second: 0, of: midnight)!.millis
                    let start = max(query.startMs, from), end = min(query.endMs, to)
                    return end > start ? Session(startMs: start, endMs: end) : nil
                }.sorted { $0.startMs < $1.startMs }
                guard !sessions.isEmpty else { return nil }
                return DayItem(key: kind == .site ? "site:" + name : "app:" + appKey, name: name, kind: kind,
                               appKey: appKey, appIds: appIds, totalMs: sessions.reduce(0) { $0 + $1.endMs - $1.startMs }, sessions: sessions)
            }.sorted { $0.totalMs == $1.totalMs ? $0.key < $1.key : $0.totalMs > $1.totalMs }
            guard !items.isEmpty else { return nil }
            return DeviceDay(device: device, totalMs: items.reduce(0) { $0 + $1.totalMs }, items: items)
        }.sorted { $0.totalMs == $1.totalMs ? $0.device.id < $1.device.id : $0.totalMs > $1.totalMs }
        let sessions = unionSessions(devices.flatMap { $0.items.flatMap(\.sessions) })
        return DayDetail(startMs: query.startMs, endMs: query.endMs,
                         wallMs: sessions.reduce(0) { $0 + $1.endMs - $1.startMs },
                         deviceMs: devices.reduce(0) { $0 + $1.totalMs }, devices: devices)
    }

    func summary(query: SummaryQuery) throws -> Summary {
        let detail = try day(query: DayQuery(startMs: query.startMs, endMs: query.endMs))
        let devices = detail.devices.filter { query.deviceId == nil || query.deviceId == $0.device.id }
        let items = devices.flatMap(\.items)
        let apps = items.filter { $0.kind == .app }.map {
            AppUsage(appKey: $0.appKey, appName: $0.name, isBrowser: $0.appKey == "chrome", totalMs: $0.totalMs,
                     domains: [], appIds: $0.appIds)
        }.sorted { $0.totalMs > $1.totalMs }
        return Summary(startMs: query.startMs, endMs: query.endMs, totalMs: detail.deviceMs,
                       byDevice: devices.map { DeviceUsage(device: $0.device, totalMs: $0.totalMs) }, apps: apps,
                       sites: items.filter { $0.kind == .site }.map { DomainUsage(domain: $0.name, totalMs: $0.totalMs) }, buckets: [])
    }

    func devices() throws -> [DeviceInfo] { [Self.mac, Self.phone, Self.laptop] }

    func newSyncCode() -> String {
        let alphabet = Array("0123456789ABCDEFGHJKMNPQRSTVWXYZ")
        return (0..<14).map { _ in String((0..<4).map { _ in alphabet.randomElement()! }) }.joined(separator: "-")
    }

    func syncConfig() throws -> SyncConfig? { lock.withLock { config } }

    func setSyncConfig(config: SyncConfig?) throws { lock.withLock { self.config = config } }

    func syncNow() throws -> SyncReport {
        guard try syncConfig() != nil else { throw SpentDayError.SyncNotConfigured }
        Thread.sleep(forTimeInterval: 0.8)
        return SyncReport(pushedSpans: 42, pulledSpans: 317, finishedAtMs: Date.now.millis)
    }
}
