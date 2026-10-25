import AppKit
import SwiftUI

// Draws the dashboard and the menu bar panel from demo data into PNGs, offscreen.
// No window is ever ordered onto the screen, and the tool has no Dock icon or menu bar item.
// Usage: spentday-snapshot <output dir>

let output = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? ".", isDirectory: true)
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
app.appearance = NSAppearance(named: .darkAqua)
// Registered in memory only, so nothing is written to any defaults domain.
UserDefaults.standard.register(defaults: ["onboardingDone": true])

/// Runs the main run loop until `done` holds, so the model's reloads can land. Fails after 10 s.
@MainActor func wait(until done: () -> Bool) {
    let deadline = Date.now.addingTimeInterval(10)
    while !done() {
        guard Date.now < deadline else {
            FileHandle.standardError.write(Data("demo data didn't load in 10 s\n".utf8))
            exit(1)
        }
        RunLoop.main.run(until: .now.addingTimeInterval(0.05))
    }
}

/// Runs the main run loop briefly, so layout and animations finish.
@MainActor func settle() { RunLoop.main.run(until: .now.addingTimeInterval(0.5)) }

@MainActor func write(_ rep: NSBitmapImageRep?, _ name: String) {
    let url = output.appendingPathComponent(name)
    guard let data = rep?.representation(using: .png, properties: [:]) else {
        FileHandle.standardError.write(Data("couldn't draw \(name)\n".utf8))
        exit(1)
    }
    do { try data.write(to: url) } catch {
        FileHandle.standardError.write(Data("couldn't write \(url.path): \(error)\n".utf8))
        exit(1)
    }
    print(url.path)
}

let model = AppModel()
model.start(store: DemoStore())
model.range = .lastSevenDays
wait { model.summary != nil && model.today != nil }

let window = DashboardWindow.make(model: model)
// Taller than the app's default, so the app and site lists end inside the shot.
window.setContentSize(NSSize(width: 860, height: 850))
window.layoutIfNeeded()
settle()
// Toolbar controls are drawn by the window server and come out blank offscreen, so the
// shot leaves the toolbar out. The frame view still draws the title bar.
window.toolbar?.isVisible = false
settle()
let frame = window.contentView!.superview!
frame.layoutSubtreeIfNeeded()
let dashboard = frame.bitmapImageRepForCachingDisplay(in: frame.bounds)
if let dashboard { frame.cacheDisplay(in: frame.bounds, to: dashboard) }
write(dashboard, "dashboard-lastSevenDays-demo.png")

let renderer = ImageRenderer(content: MenuBarView(model: model, openDashboard: {}, quit: {})
    .background(Color(nsColor: .windowBackgroundColor))
    .environment(\.colorScheme, .dark))
renderer.scale = 2
write(renderer.cgImage.map(NSBitmapImageRep.init(cgImage:)), "menu-demo.png")
exit(0)
