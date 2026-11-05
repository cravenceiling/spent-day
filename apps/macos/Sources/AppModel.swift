import Foundation
import Observation

enum SyncOutcome: Equatable {
    case success(SyncReport)
    case failure(message: String, at: Date)
}

@MainActor @Observable
final class AppModel {
    /// `nil` until the database is open.
    private var store: UsageStore?
    private(set) var isDemo = false

    var day: Date = Calendar.current.startOfDay(for: Launch.initialDay) {
        didSet { if day != oldValue { detail = nil; usualMs = nil; reload() } }
    }
    private(set) var detail: DayDetail?
    private(set) var usualMs: Int64?
    private(set) var today: Summary?

    private(set) var loadError: String?

    private(set) var syncConfig: SyncConfig?
    private(set) var isSyncing = false
    private(set) var lastSync: SyncOutcome?

    private var lastReloadToday = Calendar.current.startOfDay(for: .now)
    private var reloadGeneration = 0
    /// Bumped on every config change, so a sync that started under the old config
    /// doesn't report its result as the new one's.
    private var configGeneration = 0
    private var resyncWhenDone = false
    private var refreshTimer: Timer?
    private var syncTimer: Timer?

    func start(store: UsageStore) {
        self.store = store
        isDemo = store is DemoStore
        reload()
        Task { syncConfig = try? await offMain { try store.syncConfig() } }
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 30, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.reload() }
        }
        syncTimer = Timer.scheduledTimer(withTimeInterval: 300, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.syncIfConfigured() }
        }
        Task {
            try? await Task.sleep(for: .seconds(10))
            syncIfConfigured()
        }
    }

    func reload() {
        guard let store else { return }
        let now = Date.now
        let newToday = Calendar.current.startOfDay(for: now)
        let followedToday = day == lastReloadToday
        lastReloadToday = newToday
        if followedToday && day != newToday {
            day = newToday
            return
        }
        let selected = dayQuery(day)
        let todayQuery = dayQuery(now)
        let history = usualQueries(day: day, now: now)
        reloadGeneration += 1
        let generation = reloadGeneration
        Task {
            do {
                let (detail, today, usual) = try await offMain {
                    let detail = try store.day(query: selected)
                    let todayDetail: DayDetail
                    if selected == todayQuery { todayDetail = detail }
                    else { todayDetail = try store.day(query: todayQuery) }
                    var today = try store.summary(query: SummaryQuery(startMs: todayQuery.startMs, endMs: todayQuery.endMs,
                                                                       bucketMs: 3_600_000, deviceId: nil))
                    today.totalMs = todayDetail.wallMs
                    let usual = usualTotal(try history.map { try store.day(query: $0) })
                    return (detail, today, usual)
                }
                guard generation == self.reloadGeneration else { return }
                self.detail = detail
                self.today = today
                self.usualMs = usual
                self.loadError = nil
            } catch {
                guard generation == self.reloadGeneration else { return }
                self.loadError = error.userMessage
            }
        }
    }

    func stepDay(_ offset: Int) {
        guard let next = Calendar.current.date(byAdding: .day, value: offset, to: day),
              next <= Calendar.current.startOfDay(for: .now) else { return }
        day = next
    }

    // MARK: Sync

    func syncIfConfigured() {
        guard syncConfig != nil else { return }
        syncNow()
    }

    func syncNow() {
        guard !isSyncing, let store else { return }
        isSyncing = true
        let generation = configGeneration
        Task {
            let outcome: SyncOutcome
            do {
                outcome = .success(try await offMain { try store.syncNow() })
                reload()
            } catch {
                outcome = .failure(message: error.userMessage, at: .now)
            }
            isSyncing = false
            if generation == configGeneration { lastSync = outcome }
            if resyncWhenDone {
                resyncWhenDone = false
                syncIfConfigured()
            }
        }
    }

    private func configChanged() {
        configGeneration += 1
        lastSync = nil
    }

    /// Stores a new or changed config and syncs straight away.
    func configureSync(serverURL: String, code: String) async throws {
        let config = SyncConfig(
            serverUrl: serverURL.trimmingCharacters(in: .whitespacesAndNewlines),
            syncCode: code.trimmingCharacters(in: .whitespacesAndNewlines)
        )
        let store = try openStore()
        try await offMain { try store.setSyncConfig(config: config) }
        syncConfig = try await offMain { try store.syncConfig() }
        configChanged()
        if isSyncing { resyncWhenDone = true } else { syncNow() }
    }

    func createSyncCode(serverURL: String) async throws {
        let store = try openStore()
        let code = try await offMain { store.newSyncCode() }
        try await configureSync(serverURL: serverURL, code: code)
    }

    private func openStore() throws -> UsageStore {
        guard let store else { throw SpentDayError.Storage("The database is still opening. Try again in a moment.") }
        return store
    }

    func turnOffSync() async throws {
        let store = try openStore()
        try await offMain { try store.setSyncConfig(config: nil) }
        syncConfig = nil
        configChanged()
        resyncWhenDone = false
    }
}
