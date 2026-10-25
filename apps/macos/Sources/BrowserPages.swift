import CoreGraphics
import Foundation

/// What the front window of a browser shows.
enum BrowserTab: Equatable {
    case page(WebPage)
    /// No web page: no window, a new tab, a non-http(s) URL, or a private window.
    case noPage
}

/// Reads the active tab of the frontmost browser through AppleScript, on a queue of
/// its own, so a slow browser or a pending Automation prompt never holds up a
/// heartbeat. A failure backs that browser off so a denied Automation permission
/// costs one attempt per window, not one every 5 s.
final class BrowserPages: @unchecked Sendable {
    private enum Dialect {
        case safari, chromium, arc

        /// Returns the tab's URL, or `{}` for a private window so its page never leaves the browser.
        /// The core keeps only the URL's domain.
        /// The front window is resolved once, by id, so the private check and the read
        /// are on the same window even if another comes to the front in between.
        /// Safari has no scripting property for private windows (see `SafariSites`).
        var source: String {
            let front = "set w to window id (id of front window)"
            return switch self {
            case .safari:
                """
                \(front)
                return {URL of current tab of w}
                """
            case .chromium:
                """
                \(front)
                if mode of w is "incognito" then return {}
                return {URL of active tab of w}
                """
            case .arc:
                """
                \(front)
                if incognito of w then return {}
                return {URL of active tab of w}
                """
            }
        }
    }

    private static let dialects: [String: Dialect] = [
        "com.apple.Safari": .safari,
        "com.apple.SafariTechnologyPreview": .safari,
        "com.google.Chrome": .chromium,
        "com.google.Chrome.beta": .chromium,
        "com.google.Chrome.canary": .chromium,
        "com.brave.Browser": .chromium,
        "net.imput.helium": .chromium,
        "com.microsoft.edgemac": .chromium,
        "company.thebrowser.Browser": .arc,
        "com.vivaldi.Vivaldi": .chromium,
        "com.operasoftware.Opera": .chromium,
    ]

    private static let notPermitted = -1743
    private static let noWindow = -1728
    /// Older answers are dropped rather than credited to whatever the browser shows now.
    private static let maxAge: TimeInterval = 10
    private static let refreshAfter: TimeInterval = 1

    /// Reads the active tab of a browser by its bundle id. `nil` when it could not be asked.
    typealias Ask = @Sendable (_ bundleID: String) -> BrowserTab?

    private let queue = DispatchQueue(label: "day.spent.SpentDay.browser-pages", qos: .utility)
    private let ask: Ask?
    private let frontWindow: @Sendable (pid_t) -> Int?
    private let answered: @Sendable () -> Void

    private let lock = NSLock()
    /// Which activation of the browser, and which of its windows, an answer is for.
    private struct Asked: Equatable {
        let pid: pid_t
        let activation: Int
        let window: Int?
    }

    /// Guarded by `lock`.
    private var latest: [String: (tab: BrowserTab?, at: Date, asked: Asked)] = [:]
    private var pending: Set<String> = []

    /// Confined to `queue`.
    private var scripts: [String: NSAppleScript] = [:]
    private var backedOffUntil: [String: Date] = [:]

    /// `answered` runs on the query queue after every query. Tests pass `ask`
    /// in place of AppleScript and `frontWindow` in place of the window server.
    init(
        ask: Ask? = nil,
        frontWindow: @escaping @Sendable (pid_t) -> Int? = FrontWindow.number,
        answered: @escaping @Sendable () -> Void
    ) {
        self.ask = ask
        self.frontWindow = frontWindow
        self.answered = answered
    }

    static func isSafari(_ bundleID: String) -> Bool { dialects[bundleID] == .safari }

    static func isScripted(_ bundleID: String) -> Bool { dialects[bundleID] != nil }

    /// Every browser Spent Day asks for its page.
    static var scriptedBundleIDs: [String] { dialects.keys.sorted() }

    /// The latest answer for `bundleID`'s front window in this activation, or `nil`
    /// while none has arrived or the browser could not be asked. Starts a fresh
    /// query unless one is running, and never waits for it. A new answer calls `answered`.
    func tab(bundleID: String, pid: pid_t, activation: Int) -> BrowserTab? {
        guard let dialect = Self.dialects[bundleID] else { return nil }
        let asked = Asked(pid: pid, activation: activation, window: frontWindow(pid))
        return lock.withLock {
            let now = Date.now
            let last = latest[bundleID].flatMap { $0.asked == asked && now.timeIntervalSince($0.at) < Self.maxAge ? $0 : nil }
            if !pending.contains(bundleID), last.map({ now.timeIntervalSince($0.at) >= Self.refreshAfter }) ?? true {
                pending.insert(bundleID)
                queue.async { self.query(bundleID: bundleID, dialect: dialect, asked: asked) }
            }
            return last?.tab
        }
    }

    private func query(bundleID: String, dialect: Dialect, asked: Asked) {
        var tab = if let ask { ask(bundleID) } else { run(bundleID: bundleID, dialect: dialect) }
        // Another window came to the front while the browser answered, so the answer may be from either.
        if frontWindow(asked.pid) != asked.window { tab = nil }
        lock.withLock {
            latest[bundleID] = (tab, .now, asked)
            pending.remove(bundleID)
        }
        answered()
    }

    private func run(bundleID: String, dialect: Dialect) -> BrowserTab? {
        if let until = backedOffUntil[bundleID], until > .now { return nil }

        var error: NSDictionary?
        let result = script(bundleID: bundleID, dialect: dialect).executeAndReturnError(&error)
        if let error {
            let code = error[NSAppleScript.errorNumber] as? Int ?? 0
            if code == Self.noWindow { return .noPage }
            backedOffUntil[bundleID] = .now.addingTimeInterval(code == Self.notPermitted ? 600 : 30)
            return nil
        }
        return Self.tab(from: result)
    }

    /// What a script's answer means. A private window answers `{}`, which is no page.
    static func tab(from result: NSAppleEventDescriptor) -> BrowserTab {
        guard let url = result.atIndex(1)?.stringValue, url.hasPrefix("http://") || url.hasPrefix("https://") else {
            return .noPage
        }
        return .page(WebPage(url: url))
    }

    /// The AppleScript that asks `bundleID` for its page, or `nil` when it isn't scripted.
    static func source(bundleID: String) -> String? {
        guard let dialect = dialects[bundleID] else { return nil }
        return """
        with timeout of 2 seconds
            tell application id "\(bundleID)"
                \(dialect.source)
            end tell
        end timeout
        """
    }

    private func script(bundleID: String, dialect: Dialect) -> NSAppleScript {
        if let cached = scripts[bundleID] { return cached }
        let script = NSAppleScript(source: Self.source(bundleID: bundleID)!)!
        script.compileAndReturnError(nil)
        scripts[bundleID] = script
        return script
    }
}

enum FrontWindow {
    /// The window number of `pid`'s frontmost normal window, or `nil` when none is on screen.
    /// Window numbers and owners need no permission.
    @Sendable static func number(pid: pid_t) -> Int? {
        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        guard let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else { return nil }
        let front = windows.first { window in
            window[kCGWindowOwnerPID as String] as? pid_t == pid && window[kCGWindowLayer as String] as? Int == 0
        }
        return front?[kCGWindowNumber as String] as? Int
    }
}
