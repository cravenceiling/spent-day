import AppKit

/// Sends a heartbeat every 5 s and on every app switch. All state is confined to `queue`.
final class Collector: @unchecked Sendable {
    private let core: SpentDay
    private let queue = DispatchQueue(label: "day.spent.SpentDay.collector", qos: .utility)
    private lazy var recorder = Recorder(core: core, browsers: BrowserPages { [weak self] in
        guard let self else { return }
        self.queue.async { self.tick() }
    })
    private var timer: DispatchSourceTimer?
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []
    /// Reasons the screen can't be seen right now. Heartbeats pause while any is set.
    private var hidden: Set<HiddenReason> = []
    private var lastSeen = LastSeen()
    private var stopped = false

    private enum HiddenReason { case asleep, displayAsleep, locked, screensaver, switchedOut }

    init(core: SpentDay) {
        self.core = core
    }

    func start() {
        let workspace = NSWorkspace.shared.notificationCenter
        observe(workspace, NSWorkspace.didActivateApplicationNotification) { collector in
            collector.recorder.activated()
            collector.tick()
        }
        observe(workspace, NSWorkspace.willSleepNotification) { $0.hide(.asleep) }
        observe(workspace, NSWorkspace.didWakeNotification) { $0.show(.asleep) }
        observe(workspace, NSWorkspace.screensDidSleepNotification) { $0.hide(.displayAsleep) }
        observe(workspace, NSWorkspace.screensDidWakeNotification) { $0.show(.displayAsleep) }
        observe(workspace, NSWorkspace.sessionDidResignActiveNotification) { $0.hide(.switchedOut) }
        observe(workspace, NSWorkspace.sessionDidBecomeActiveNotification) { $0.show(.switchedOut) }

        let distributed = DistributedNotificationCenter.default()
        observe(distributed, .init("com.apple.screenIsLocked")) { $0.hide(.locked) }
        observe(distributed, .init("com.apple.screenIsUnlocked")) { $0.show(.locked) }
        observe(distributed, .init("com.apple.screensaver.didstart")) { $0.hide(.screensaver) }
        observe(distributed, .init("com.apple.screensaver.didstop")) { $0.show(.screensaver) }

        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now(), repeating: .seconds(5), leeway: .milliseconds(250))
        timer.setEventHandler { [weak self] in self?.tick() }
        timer.resume()
        self.timer = timer
    }

    /// Closes the open span before the process exits.
    func stop() {
        timer?.cancel()
        for (center, token) in observers { center.removeObserver(token) }
        observers = []
        queue.sync {
            stopped = true
            flush()
        }
    }

    private func observe(_ center: NotificationCenter, _ name: Notification.Name, _ handle: @escaping @Sendable (Collector) -> Void) {
        let token = center.addObserver(forName: name, object: nil, queue: nil) { [weak self] _ in
            guard let self else { return }
            self.queue.async { handle(self) }
        }
        observers.append((center, token))
    }

    private func hide(_ reason: HiddenReason) {
        hidden.insert(reason)
        flush()
    }

    private func show(_ reason: HiddenReason) {
        hidden.remove(reason)
        tick()
    }

    /// A missed unlock or switch-back notification would stop recording for good,
    /// so every tick also reads the lock and console state of this login session.
    private func readSessionState() {
        guard let session = CGSessionCopyCurrentDictionary() as? [String: Any] else { return }
        set(.locked, session["CGSSessionScreenIsLocked"] as? Bool ?? false)
        set(.switchedOut, !(session[kCGSessionOnConsoleKey] as? Bool ?? true))
    }

    private func set(_ reason: HiddenReason, _ isSet: Bool) {
        if !isSet {
            hidden.remove(reason)
        } else if hidden.insert(reason).inserted {
            flush()
        }
    }

    private func flush() {
        do { try core.flush(atMs: Date.now.millis) } catch { log("flush", error) }
    }

    private func tick() {
        guard !stopped else { return }
        readSessionState()
        guard hidden.isEmpty else { return }
        let now = Date.now.millis
        let away = lastSeen.awaySince(
            nowMs: now,
            lastInputMs: Presence.lastInputMs(nowMs: now),
            frontHoldsDisplay: Presence.frontAppHoldsDisplayAwake
        )
        do {
            if let away {
                try core.awaySince(lastInputMs: away, nowMs: now)
            } else {
                try recorder.record(FrontApp.current(), atMs: now)
            }
        } catch {
            log("heartbeat", error)
        }
    }

    private func log(_ what: String, _ error: Error) {
        NSLog("Spent Day %@ failed: %@", what, error.logMessage)
    }
}
