import Foundation

/// Fixed sample data for `snapshot.sh` and `SPENTDAY_DEMO=1` in dev builds, so the UI can be
/// built and checked without a working core or real history.
final class DemoStore: UsageStore, @unchecked Sendable {
    private let lock = NSLock()
    private var config: SyncConfig?

    private static let mac = DeviceInfo(id: "demo-mac", name: "MacBook Pro", platform: .macos, lastSeenMs: Date.now.millis)
    private static let phone = DeviceInfo(id: "demo-phone", name: "Pixel 9", platform: .android, lastSeenMs: Date.now.millis - 1_200_000)
    private static let laptop = DeviceInfo(id: "demo-linux", name: "ThinkPad", platform: .linux, lastSeenMs: Date.now.millis - 86_400_000)

    /// (key, name, browser, share of the day, per-device weights, domains with shares)
    private static let apps: [(String, String, Bool, Double, [String: Double], [(String, Double)])] = [
        ("com.apple.dt.Xcode", "Xcode", false, 0.27, ["demo-mac": 1], []),
        ("com.apple.Safari", "Safari", true, 0.21, ["demo-mac": 1], [
            ("github.com", 0.38), ("developer.apple.com", 0.24), ("news.ycombinator.com", 0.16),
            ("youtube.com", 0.14), ("en.wikipedia.org", 0.05), ("", 0.03),
        ]),
        ("chrome", "Chrome", true, 0.13, ["demo-phone": 0.6, "demo-linux": 0.4], [
            ("youtube.com", 0.45), ("reddit.com", 0.25), ("maps.google.com", 0.18), ("nytimes.com", 0.12),
        ]),
        ("com.tinyspeck.slackmacgap", "Slack", false, 0.11, ["demo-mac": 0.8, "demo-phone": 0.2], []),
        ("com.apple.Terminal", "Terminal", false, 0.09, ["demo-mac": 0.7, "demo-linux": 0.3], []),
        ("com.apple.MobileSMS", "Messages", false, 0.07, ["demo-mac": 1], []),
        ("com.spotify.client", "Spotify", false, 0.05, ["demo-phone": 1], []),
        ("com.apple.mail", "Mail", false, 0.04, ["demo-mac": 1], []),
        ("org.telegram.messenger", "Telegram", false, 0.03, ["demo-phone": 1], []),
    ]

    /// Minutes of use per local hour of a typical day.
    private static let dailyShape: [Double] = [
        0, 0, 0, 0, 0, 0, 0, 8, 22, 46, 52, 55, 30, 41, 54, 57, 49, 38, 20, 25, 34, 28, 12, 3,
    ]

    func summary(query: SummaryQuery) throws -> Summary {
        let calendar = Calendar.current
        let now = Date.now.millis
        let devices = query.deviceId.map { [$0] } ?? ["demo-mac", "demo-phone", "demo-linux"]
        let deviceShare = devices.reduce(0.0) { total, id in total + Self.deviceShare(id) }

        var buckets: [Int64] = []
        var slot = query.startMs
        while slot < query.endMs {
            let date = Date(millis: slot)
            let hour = calendar.component(.hour, from: date)
            let day = calendar.ordinality(of: .day, in: .era, for: date) ?? 0
            let wobble = 0.75 + 0.25 * sin(Double(day * 7 + hour))
            let elapsed = min(1, max(0, Double(now - slot) / 3_600_000))
            let minutes = Self.dailyShape[hour] * wobble * deviceShare * elapsed
            buckets.append(Int64(minutes * 60_000))
            slot += query.bucketMs
        }
        let total = buckets.reduce(0, +)

        var apps: [AppUsage] = []
        for (key, name, isBrowser, share, weights, domains) in Self.apps {
            let weight = devices.reduce(0.0) { $0 + (weights[$1] ?? 0) * Self.deviceShare($1) } / max(deviceShare, 0.001)
            let millis = Int64(Double(total) * share * weight)
            guard millis > 0 else { continue }
            let usage = domains.map { DomainUsage(domain: $0.0, totalMs: Int64(Double(millis) * $0.1)) }
            let ids = key == "chrome" ? ["com.android.chrome", "google-chrome"] : [key]
            apps.append(AppUsage(appKey: key, appName: name, isBrowser: isBrowser, totalMs: millis, domains: usage, appIds: ids))
        }
        apps.sort { $0.totalMs > $1.totalMs }

        let sites = Dictionary(grouping: apps.flatMap(\.domains).filter { !$0.domain.isEmpty }, by: \.domain)
            .map { DomainUsage(domain: $0.key, totalMs: $0.value.reduce(0) { $0 + $1.totalMs }) }
            .sorted { $0.totalMs > $1.totalMs }
        let byDevice = [Self.mac, Self.phone, Self.laptop]
            .filter { devices.contains($0.id) }
            .map { DeviceUsage(device: $0, totalMs: Int64(Double(total) * Self.deviceShare($0.id) / max(deviceShare, 0.001))) }

        return Summary(startMs: query.startMs, endMs: query.endMs, totalMs: total, byDevice: byDevice, apps: apps, sites: sites, buckets: buckets)
    }

    private static func deviceShare(_ id: String) -> Double {
        ["demo-mac": 0.62, "demo-phone": 0.28, "demo-linux": 0.1][id] ?? 0
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
