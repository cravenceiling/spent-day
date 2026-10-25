import AppKit
import SwiftUI

@main
struct SpentDayApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        MenuBarExtra {
            MenuBarView(model: delegate.model, openDashboard: delegate.showDashboard, quit: delegate.quit)
        } label: {
            Image(systemName: "hourglass")
        }
        .menuBarExtraStyle(.window)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = AppModel()
    private var collector: Collector?
    private var dashboard: NSWindow?
    private var terminationSignal: DispatchSourceSignal?

    func applicationWillFinishLaunching(_ notification: Notification) {
        NSAppleEventManager.shared().setEventHandler(
            self, andSelector: #selector(handleQuitEvent(_:reply:)),
            forEventClass: AEEventClass(kCoreEventClass), andEventID: AEEventID(kAEQuitApplication)
        )
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // `kill` and launchd send SIGTERM; route it through terminate so the open span is flushed.
        signal(SIGTERM, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        source.setEventHandler { [weak self] in self?.quit() }
        source.resume()
        terminationSignal = source

        if Launch.isDemo {
            model.start(store: DemoStore())
        } else {
            openCore()
        }
        if Launch.opensDashboard || !Onboarding.isDone {
            showDashboard()
        }
    }

    /// Opening runs migrations and prunes old hints, and looking up the device name
    /// can wait on the network, so neither holds up the menu bar.
    private func openCore() {
        Task {
            do {
                let core = try await offMain {
                    try Launch.prepareDataFolder()
                    return try SpentDay.open(dbPath: Launch.databasePath, deviceName: Launch.deviceName, platform: .macos)
                }
                let collector = Collector(core: core)
                collector.start()
                self.collector = collector
                model.start(store: core)
                Task.detached {
                    do { try core.startWebListener(port: 7853) } catch {
                        // Binding a local port never involves a server, so its detail is kept.
                        NSLog("Spent Day web listener failed: %@", String(describing: error))
                    }
                }
            } catch {
                let alert = NSAlert()
                alert.messageText = "Spent Day can't open its database"
                alert.informativeText = "\(error.userMessage)\n\n\(Launch.databasePath)"
                alert.runModal()
                exit(1)
            }
        }
    }

    /// AppKit cancels termination, before asking the delegate, while a sheet is attached.
    /// Onboarding and settings are sheets, so every quit path ends them first:
    /// the Quit button, the quit Apple event (logout, `osascript`), and SIGTERM.
    func quit() {
        for window in NSApp.windows {
            if let sheet = window.attachedSheet { window.endSheet(sheet) }
        }
        NSApp.terminate(nil)
    }

    @objc private func handleQuitEvent(_ event: NSAppleEventDescriptor, reply: NSAppleEventDescriptor) {
        quit()
    }

    func applicationWillTerminate(_ notification: Notification) {
        collector?.stop()
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showDashboard()
        return false
    }

    func showDashboard() {
        if dashboard == nil {
            let window = DashboardWindow.make(model: model)
            window.setFrameAutosaveName("Dashboard")
            window.center()
            dashboard = window
        }
        model.reload()
        dashboard?.makeKeyAndOrderFront(nil)
        NSApp.activate()
    }
}
