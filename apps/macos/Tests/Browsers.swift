import Foundation

/// Stands in for a browser's AppleScript answer. While held, answers wait for `release()`.
final class FakeBrowser: @unchecked Sendable {
    private let lock = NSLock()
    private var tab: BrowserTab?
    private var held = false
    private var gate = DispatchSemaphore(value: 0)
    private(set) var asked = 0
    private var front: Int? = 1
    let answers = DispatchSemaphore(value: 0)

    /// The number of the browser's front window.
    var window: Int? {
        get { lock.withLock { front } }
        set { lock.withLock { front = newValue } }
    }

    func show(_ tab: BrowserTab?) { lock.withLock { self.tab = tab } }
    func hold() { lock.withLock { held = true; gate = DispatchSemaphore(value: 0) } }
    func release() {
        let gate = lock.withLock { held = false; return self.gate }
        gate.signal()
    }

    func ask(_ bundleID: String) -> BrowserTab? {
        let (held, gate) = lock.withLock { (self.held, self.gate) }
        if held { gate.wait() }
        return lock.withLock { asked += 1; return tab }
    }

    /// Gives a query that may have started time to answer.
    func settle() { _ = answers.wait(timeout: .now() + 0.5) }

    /// Waits for the browser's next answer to reach BrowserPages.
    @MainActor func waitForAnswer(_ what: String) {
        check(answers.wait(timeout: .now() + 3) == .success, "\(what): the browser answered")
    }
}

func browserHarness() throws -> (SpentDay, Recorder, FakeBrowser) {
    let core = try scratchCore()
    let browser = FakeBrowser()
    let pages = BrowserPages(ask: { browser.ask($0) }, frontWindow: { _ in browser.window }, answered: { browser.answers.signal() })
    return (core, Recorder(core: core, browsers: pages), browser)
}

let finderApp = FrontApp(id: "com.apple.finder", name: "Finder", pid: 100)
let chromeApp = FrontApp(id: "com.google.Chrome", name: "Google Chrome", pid: 200)
let safariApp = FrontApp(id: "com.apple.Safari", name: "Safari", pid: 300)

/// Time per site in `app`'s row, "" for no site.
func sites(_ core: SpentDay, _ appID: String) throws -> [String: Int64] {
    let summary = try core.summary(query: SummaryQuery(startMs: t0 - 3_600_000, endMs: t0 + 3_600_000, bucketMs: 3_600_000, deviceId: nil))
    guard let row = summary.apps.first(where: { $0.appIds.contains(appID) }) else { return [:] }
    if row.domains.isEmpty { return ["": row.totalMs] }
    return Dictionary(uniqueKeysWithValues: row.domains.map { ($0.domain, $0.totalMs) })
}

@MainActor func browserChecks() throws {
    do {
        // Finder, then Chrome comes to the front and answers a moment later.
        let (core, recorder, browser) = try browserHarness()
        browser.show(.page(page("https://youtube.com/watch")))
        try recorder.record(finderApp, atMs: t0)
        try recorder.record(chromeApp, atMs: t0 + 5_000)
        browser.waitForAnswer("first Chrome query")
        try recorder.record(chromeApp, atMs: t0 + 6_000)
        try core.flush(atMs: t0 + 8_000)
        check(try sites(core, "com.apple.finder") == ["": 5_000], "Finder stops getting time when Chrome comes to the front (got \(try sites(core, "com.apple.finder")))")
        check(try sites(core, "com.google.Chrome") == ["youtube.com": 2_000, "": 1_000],
              "Chrome has no site until it answers, then its page (got \(try sites(core, "com.google.Chrome")))")
    }

    do {
        // Chrome answers, the user switches to Finder and back, and Chrome is slow to answer again.
        let (core, recorder, browser) = try browserHarness()
        browser.show(.page(page("https://a.example/")))
        recorder.activated()
        try recorder.record(chromeApp, atMs: t0)
        browser.waitForAnswer("Chrome's first answer")
        try recorder.record(chromeApp, atMs: t0 + 1_000)
        recorder.activated()
        try recorder.record(finderApp, atMs: t0 + 2_000)
        browser.hold()
        browser.show(.page(page("https://b.example/")))
        recorder.activated()
        try recorder.record(chromeApp, atMs: t0 + 3_000)
        browser.release()
        browser.waitForAnswer("Chrome's answer after it came back")
        try recorder.record(chromeApp, atMs: t0 + 4_000)
        try core.flush(atMs: t0 + 5_000)
        check(try sites(core, "com.google.Chrome") == ["a.example": 1_000, "b.example": 1_000, "": 2_000],
              "a page read before Chrome was reactivated is never recorded after it (got \(try sites(core, "com.google.Chrome")))")
    }
    do {
        // Another Chrome window comes to the front without an app switch.
        let (core, recorder, browser) = try browserHarness()
        browser.show(.page(page("https://a.example/")))
        recorder.activated()
        try recorder.record(chromeApp, atMs: t0)
        browser.waitForAnswer("window 1")
        try recorder.record(chromeApp, atMs: t0 + 1_000)
        browser.hold()
        browser.window = 2
        browser.show(.page(page("https://b.example/")))
        try recorder.record(chromeApp, atMs: t0 + 2_000)
        browser.release()
        browser.waitForAnswer("window 2")
        try recorder.record(chromeApp, atMs: t0 + 3_000)
        try core.flush(atMs: t0 + 4_000)
        check(try sites(core, "com.google.Chrome") == ["a.example": 1_000, "b.example": 1_000, "": 2_000],
              "a page read from one window is never recorded for another (got \(try sites(core, "com.google.Chrome")))")
    }
    do {
        // The front window changes while Chrome is answering, so the answer could be from either.
        let (core, recorder, browser) = try browserHarness()
        browser.show(.page(page("https://a.example/")))
        browser.hold()
        recorder.activated()
        try recorder.record(chromeApp, atMs: t0)
        browser.window = 2
        browser.release()
        browser.waitForAnswer("the answer across a window switch")
        try recorder.record(chromeApp, atMs: t0 + 1_000)
        try core.flush(atMs: t0 + 2_000)
        check(try sites(core, "com.google.Chrome") == ["": 2_000],
              "an answer that spans a window switch is dropped (got \(try sites(core, "com.google.Chrome")))")
    }
    do {
        // Safari sites are off by default: Safari can't tell its Private windows apart.
        let (core, recorder, browser) = try browserHarness()
        browser.show(.page(page("https://apple.com/")))
        recorder.activated()
        try recorder.record(safariApp, atMs: t0)
        browser.settle()
        try recorder.record(safariApp, atMs: t0 + 1_000)
        try core.flush(atMs: t0 + 2_000)
        check(try sites(core, "com.apple.Safari") == ["": 2_000], "Safari time has no site by default (got \(try sites(core, "com.apple.Safari")))")
        check(browser.asked == 0, "Safari is never asked for its page by default (asked \(browser.asked) times)")
    }
    do {
        let (core, recorder, browser) = try browserHarness()
        UserDefaults.standard.register(defaults: [SafariSites.defaultsKey: true])
        defer { UserDefaults.standard.removeVolatileDomain(forName: UserDefaults.registrationDomain) }
        browser.show(.page(page("https://apple.com/")))
        recorder.activated()
        try recorder.record(safariApp, atMs: t0)
        browser.waitForAnswer("Safari with sites on")
        try recorder.record(safariApp, atMs: t0 + 1_000)
        try core.flush(atMs: t0 + 2_000)
        check(try sites(core, "com.apple.Safari") == ["": 1_000, "apple.com": 1_000],
              "with Record Safari sites on, Safari's page is recorded (got \(try sites(core, "com.apple.Safari")))")
    }
    do {
        // Chrome shows a page, then an incognito window comes to the front.
        let (core, recorder, browser) = try browserHarness()
        browser.show(BrowserPages.tab(from: urlAnswer("https://a.example/watch")))
        recorder.activated()
        try recorder.record(chromeApp, atMs: t0)
        browser.waitForAnswer("the normal window")
        try recorder.record(chromeApp, atMs: t0 + 1_000)
        browser.window = 2
        // What the script returns for an incognito window.
        browser.show(BrowserPages.tab(from: .list()))
        try recorder.record(chromeApp, atMs: t0 + 2_000)
        browser.waitForAnswer("the incognito window")
        try recorder.record(chromeApp, atMs: t0 + 3_000)
        try recorder.record(chromeApp, atMs: t0 + 4_000)
        try core.flush(atMs: t0 + 5_000)
        check(BrowserPages.tab(from: .list()) == .noPage, "a private window's empty answer is no page")
        check(try sites(core, "com.google.Chrome") == ["a.example": 1_000, "": 4_000],
              "a private window gets no site, and the page before it stops getting time (got \(try sites(core, "com.google.Chrome")))")
    }
    check(BrowserPages.tab(from: urlAnswer("file:///Users/me/notes.html")) == .noPage, "a file URL is no page")
    check(BrowserPages.tab(from: urlAnswer("https://a.example/")) == .page(page("https://a.example/")), "an https URL is a page")
    scriptChecks()
}

func urlAnswer(_ url: String) -> NSAppleEventDescriptor {
    let list = NSAppleEventDescriptor.list()
    list.insert(NSAppleEventDescriptor(string: url), at: 1)
    return list
}

/// The full script each browser gets. It resolves the front window once, returns `{}` for a
/// private window, and otherwise asks for the active tab's URL and nothing else.
@MainActor func scriptChecks() {
    let chromium = """
        set w to window id (id of front window)
        if mode of w is "incognito" then return {}
        return {URL of active tab of w}
        """
    let arc = """
        set w to window id (id of front window)
        if incognito of w then return {}
        return {URL of active tab of w}
        """
    // Safari has no private-window property, so it is only asked when Safari sites are on.
    let safari = """
        set w to window id (id of front window)
        return {URL of current tab of w}
        """
    let expected: [String: String] = [
        "com.apple.Safari": safari,
        "com.apple.SafariTechnologyPreview": safari,
        "com.google.Chrome": chromium,
        "com.google.Chrome.beta": chromium,
        "com.google.Chrome.canary": chromium,
        "com.brave.Browser": chromium,
        "net.imput.helium": chromium,
        "com.microsoft.edgemac": chromium,
        "com.vivaldi.Vivaldi": chromium,
        "com.operasoftware.Opera": chromium,
        "company.thebrowser.Browser": arc,
    ]
    check(BrowserPages.scriptedBundleIDs == expected.keys.sorted(),
          "every scripted browser has an expected script (scripted: \(BrowserPages.scriptedBundleIDs))")
    for bundleID in BrowserPages.scriptedBundleIDs {
        let want = "with timeout of 2 seconds\n    tell application id \"\(bundleID)\"\n        \(expected[bundleID]!)\n    end tell\nend timeout"
        check(BrowserPages.source(bundleID: bundleID) == want,
              "\(bundleID)'s script asks for the front window's URL\(expected[bundleID] == safari ? "" : " and private state"), nothing else")
    }
    check(BrowserPages.source(bundleID: "org.mozilla.firefox") == nil, "Firefox is not scripted")
}
