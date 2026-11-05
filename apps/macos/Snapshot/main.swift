import AppKit
import SwiftUI

// Draws fixed day, hover, device, sessions, light, small, empty and menu demos offscreen.
// No window is ever ordered onto the screen, and the tool has no Dock icon or menu bar item.
// Usage: spentday-snapshot <output dir>. snapshot.sh accepts the same output directory.
// Dev app preview: SPENTDAY_DEMO=1 SpentDay --dashboard --day yyyy-MM-dd.

let output = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? ".", isDirectory: true)
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
// Registered in memory only, so nothing is written to any defaults domain.
UserDefaults.standard.register(defaults: ["onboardingDone": true])

@MainActor func write(_ image: CGImage?, _ name: String) {
    let url = output.appendingPathComponent(name)
    guard let image, let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
        FileHandle.standardError.write(Data("couldn't draw \(name)\n".utf8))
        exit(1)
    }
    do { try data.write(to: url) } catch {
        FileHandle.standardError.write(Data("couldn't write \(url.path): \(error)\n".utf8))
        exit(1)
    }
    print(url.path)
}

let calendar = Calendar.current
let date = calendar.date(from: DateComponents(year: 2026, month: 10, day: 28))!
let detail = try DemoStore().day(query: dayQuery(date))

@MainActor func snapshot(_ name: String, dark: Bool = true, width: CGFloat = 1280, height: CGFloat = 800,
                         drill: DayDrill = DayDrill(), hover: String? = nil, empty: Bool = false) {
    app.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
    let data = empty ? DayDetail(startMs: detail.startMs, endMs: detail.endMs, wallMs: 0, deviceMs: 0, devices: []) : detail
    let view = DayView(detail: data, day: date, usualMs: empty ? nil : 46_980_000, demoColors: true,
                       initialDrill: drill, initialHover: hover)
        .frame(width: width, height: height)
        .environment(\.colorScheme, dark ? .dark : .light)
        .transaction { $0.disablesAnimations = true }
    let host = NSHostingView(rootView: view)
    let bounds = NSRect(x: 0, y: 0, width: width, height: height)
    let window = NSWindow(contentRect: bounds, styleMask: .borderless, backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    window.contentView = host
    window.setContentSize(bounds.size)
    host.layoutSubtreeIfNeeded()
    RunLoop.main.run(until: .now.addingTimeInterval(0.3))
    host.layoutSubtreeIfNeeded()
    // ImageRenderer omits native scroll views; cache the real hosting view at one pixel per point.
    let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(width), pixelsHigh: Int(height),
                                  bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                  colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 32)!
    bitmap.size = bounds.size
    host.cacheDisplay(in: host.bounds, to: bitmap)
    write(bitmap.cgImage, name)
    window.close()
}

snapshot("dashboard-day-demo.png")
snapshot("dashboard-hover-demo.png", hover: "demo-mac/app:com.apple.dt.Xcode")
snapshot("dashboard-device-demo.png", drill: DayDrill(deviceID: "demo-mac"))
snapshot("dashboard-sessions-demo.png", drill: DayDrill(deviceID: "demo-mac", itemKey: "app:com.apple.dt.Xcode"))
snapshot("dashboard-light-demo.png", dark: false)
snapshot("dashboard-small-demo.png", width: 720, height: 560)
snapshot("dashboard-empty-demo.png", empty: true)

let model = AppModel()
model.start(store: DemoStore())
let deadline = Date.now.addingTimeInterval(10)
while model.today == nil && Date.now < deadline { RunLoop.main.run(until: .now.addingTimeInterval(0.05)) }
guard model.today != nil else { fatalError("Demo data did not load") }
app.appearance = NSAppearance(named: .darkAqua)
let menu = ImageRenderer(content: MenuBarView(model: model, openDashboard: {}, quit: {})
    .background(Color(nsColor: .windowBackgroundColor)).environment(\.colorScheme, .dark))
menu.scale = 2
write(menu.cgImage, "menu-demo.png")
exit(0)
