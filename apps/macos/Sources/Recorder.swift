import AppKit

/// The app in front, as one tick reads it.
struct FrontApp {
    let id: String
    let name: String
    let pid: pid_t

    static func current() -> FrontApp? {
        // Command-line tools and some helper apps have no bundle id but are still in front.
        guard let app = NSWorkspace.shared.frontmostApplication,
              let id = app.bundleIdentifier ?? app.executableURL?.lastPathComponent ?? app.localizedName
        else { return nil }
        return FrontApp(id: id, name: app.localizedName ?? id, pid: app.processIdentifier)
    }
}

/// Turns the app in front into heartbeats, with the page of a scripted browser.
/// Confined to the collector's queue.
final class Recorder {
    private let core: SpentDay
    private let browsers: BrowserPages
    /// Bumped on every app activation, so a page read before one is never recorded after it.
    private var activation = 0
    /// The last tab each browser showed, so a switch to no page is reported once.
    private var lastTabs: [String: BrowserTab] = [:]

    init(core: SpentDay, browsers: BrowserPages) {
        self.core = core
        self.browsers = browsers
    }

    func activated() { activation += 1 }

    /// `nil` records that nothing is in front.
    func record(_ front: FrontApp?, atMs now: Int64) throws {
        guard let front else {
            try core.heartbeat(atMs: now, activity: nil)
            return
        }
        var web: WebPage?
        if BrowserPages.isScripted(front.id) {
            // A browser that hasn't answered yet is recorded with no page. Its answer ticks again.
            let tab = BrowserPages.isSafari(front.id) && !SafariSites.areRecorded
                ? .noPage
                : browsers.tab(bundleID: front.id, pid: front.pid, activation: activation)
            if tab == .noPage, lastTabs[front.id] != .noPage {
                try core.webCleared(atMs: now, browser: front.id)
            }
            lastTabs[front.id] = tab
            if case .page(let page) = tab { web = page }
        }
        let activity = Activity(appId: front.id, appName: front.name, web: web)
        try core.heartbeat(atMs: now, activity: activity)
    }
}

/// Safari can't tell its Private windows apart to AppleScript,
/// so its sites are recorded only after the user turns this on in Settings.
enum SafariSites {
    static let defaultsKey = "recordSafariSites"
    static var areRecorded: Bool { UserDefaults.standard.bool(forKey: defaultsKey) }
}
