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

    var range: DayRange = Launch.initialRange { didSet { if range != oldValue { reload() } } }
    /// `nil` means all devices.
    var deviceID: String? { didSet { if deviceID != oldValue { reload() } } }

    private(set) var summary: Summary?
    private(set) var today: Summary?
    private(set) var devices: [DeviceInfo] = []
    private(set) var loadError: String?

    private(set) var syncConfig: SyncConfig?
    private(set) var isSyncing = false
    private(set) var lastSync: SyncOutcome?

    private var reloadGeneration = 0
    /// Bumped on every config change, so a sync that started under the old config
    /// doesn't report its result as the new one's.
    private var configGeneration = 0
    private var resyncWhenDone = false
    private var refreshTimer: Timer?
    private var syncTimer: Timer?

    func start(store: UsageStore) {
        self.store = store
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
        let selected = range.query(deviceID: deviceID)
        let todayQuery = DayRange.today.query(deviceID: nil)
        reloadGeneration += 1
        let generation = reloadGeneration
        Task {
            do {
                let (summary, today, devices) = try await offMain {
                    (try store.summary(query: selected), try store.summary(query: todayQuery), try store.devices())
                }
                // A slower, older reload must not overwrite a newer selection.
                guard generation == self.reloadGeneration else { return }
                self.summary = summary
                self.today = today
                self.devices = devices
                self.loadError = nil
            } catch {
                self.loadError = error.userMessage
            }
        }
    }

    var selectedDeviceName: String {
        devices.first { $0.id == deviceID }?.name ?? "All Devices"
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
