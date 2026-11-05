import Foundation

/// The slice of the core the UI reads and writes. `SpentDay` conforms as is;
/// `DemoStore` stands in for demo data and screenshots.
protocol UsageStore: AnyObject, Sendable {
    func day(query: DayQuery) throws -> DayDetail
    func summary(query: SummaryQuery) throws -> Summary
    func devices() throws -> [DeviceInfo]
    func newSyncCode() -> String
    func syncConfig() throws -> SyncConfig?
    func setSyncConfig(config: SyncConfig?) throws
    func syncNow() throws -> SyncReport
}

extension SpentDay: UsageStore {}

/// Every core method may block on SQLite or the network, so none run on the main thread.
func offMain<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
    try await withCheckedThrowingContinuation { continuation in
        DispatchQueue.global(qos: .userInitiated).async {
            continuation.resume(with: Result { try work() })
        }
    }
}

enum Launch {
    /// Read only by builds made with `build.sh --dev`, which scripts use to point the app at a
    /// scratch database or demo data. A normal build ignores the environment, so nothing can
    /// redirect an installed app's data.
    private static func override(_ name: String) -> String? {
        #if SPENTDAY_DEV
        if let value = ProcessInfo.processInfo.environment[name], !value.isEmpty { return value }
        #endif
        return nil
    }

    /// `SPENTDAY_DB` points dev builds at a scratch database.
    static var databasePath: String {
        if let path = override("SPENTDAY_DB") { return path }
        return dataFolder.appendingPathComponent("spentday.db").path
    }

    static var dataFolder: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent(folderName, isDirectory: true)
    }

    /// Runs before the database opens. The folder of a `SPENTDAY_DB` file is the user's, so it is
    /// left alone.
    static func prepareDataFolder() throws {
        if override("SPENTDAY_DB") == nil { try makePrivateFolder(dataFolder) }
    }

    /// The folder is the app's own, so only this user may open it, even one an older build made
    /// under umask 022. If either step fails, the app stops instead of putting the sync code there.
    static func makePrivateFolder(_ folder: URL) throws {
        let privateMode: [FileAttributeKey: Any] = [.posixPermissions: 0o700]
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true, attributes: privateMode)
            try FileManager.default.setAttributes(privateMode, ofItemAtPath: folder.path)
        } catch {
            throw DataFolderError(folder: folder.path, reason: error.localizedDescription)
        }
    }

    /// A dev build runs beside the installed app, so it keeps its data in a folder of its own.
    static var folderName: String {
        #if SPENTDAY_DEV
        return "SpentDay Dev"
        #else
        return "SpentDay"
        #endif
    }

    /// `SPENTDAY_DEMO=1` shows `DemoStore` in dev builds.
    static var isDemo: Bool { override("SPENTDAY_DEMO") == "1" }

    static var opensDashboard: Bool { CommandLine.arguments.contains("--dashboard") || opensSettings }

    static var opensSettings: Bool { CommandLine.arguments.contains("--settings") }

    static var initialDay: Date {
        let arguments = CommandLine.arguments
        guard let flag = arguments.firstIndex(of: "--day"), flag + 1 < arguments.count else { return .now }
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.date(from: arguments[flag + 1]) ?? .now
    }

    static var deviceName: String { Host.current().localizedName ?? "Mac" }
}

struct DataFolderError: LocalizedError {
    let folder: String
    let reason: String
    var errorDescription: String? { "Spent Day couldn't make its folder \(folder) private. \(reason)" }
}

extension Error {
    /// What the user reads when a core call fails. Every sync failure maps to a fixed message,
    /// so a server can't put its own text, such as its error body or a redirect target, in the UI.
    var userMessage: String {
        guard let error = self as? SpentDayError else { return localizedDescription }
        switch error {
        case .Storage(let detail):
            return "Spent Day couldn't read its database. \(detail)"
        case .SyncNotConfigured:
            return "Sync isn't set up on this Mac yet."
        case .InvalidSyncCode:
            return "That sync code isn't valid. Copy it again from a device that already syncs."
        case .Network:
            return "Couldn't reach the sync server or read its answer. Check the address and your connection."
        case .Server(let status, _):
            switch status {
            case 300..<400:
                return "The sync server sent Spent Day to another address. Spent Day doesn't follow redirects, so enter the address of the sync server itself."
            case 401, 403:
                return "The sync server didn't accept the sync code."
            case 400..<500:
                return "The sync server turned down the request."
            case 500..<600:
                return "The sync server had a problem. Try again later."
            default:
                return "The sync server sent an answer Spent Day doesn't understand."
            }
        case .Decrypt:
            return "Data on the server couldn't be decrypted. Make sure every device uses the same sync code."
        case .InvalidArgument(let detail):
            return "Spent Day couldn't use that value. \(detail)"
        case .OldDatabase:
            return "This data was made by an early test build of Spent Day and can't be opened. Quit Spent Day, delete the database with its -wal and -shm files, and open Spent Day again to start over."
        }
    }

    /// For the system log. A server's own text stays out of it too, so a network error, whose
    /// detail can quote the server's answer, is logged without it.
    var logMessage: String {
        switch self as? SpentDayError {
        case .Server(let status, _)?: return "the sync server answered \(status)"
        case .Network?: return "couldn't reach the sync server or read its answer"
        default: return String(describing: self)
        }
    }
}

extension Int64 {
    var accessibleDuration: String {
        let minutes = Swift.max(0, Int64((Double(self) / 60_000).rounded())), hours = minutes / 60, rest = minutes % 60
        if hours == 0 { return "\(rest) \(rest == 1 ? "minute" : "minutes")" }
        return "\(hours) \(hours == 1 ? "hour" : "hours") \(rest) \(rest == 1 ? "minute" : "minutes")"
    }

    /// "45m", "2h 5m", "<1m".
    var durationText: String {
        let minutes = Int((Double(self) / 60_000).rounded())
        if minutes < 1 { return self > 0 ? "<1m" : "0m" }
        let hours = minutes / 60, rest = minutes % 60
        if hours == 0 { return "\(rest)m" }
        return rest == 0 ? "\(hours)h" : "\(hours)h \(rest)m"
    }
}

extension Date {
    var millis: Int64 { Int64((timeIntervalSince1970 * 1000).rounded()) }
    init(millis: Int64) { self.init(timeIntervalSince1970: Double(millis) / 1000) }
}
