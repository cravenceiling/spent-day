import AppKit
import SwiftUI

@MainActor enum DashboardWindow {
    static func make(model: AppModel) -> NSWindow {
        let host = NSHostingController(rootView: DashboardView(model: model))
        host.sizingOptions = []
        let window = NSWindow(contentViewController: host)
        window.title = "Spent Day"
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.setContentSize(NSSize(width: 1280, height: 800))
        window.contentMinSize = NSSize(width: 720, height: 560)
        window.isReleasedWhenClosed = false
        return window
    }
}

struct DashboardView: View {
    @Bindable var model: AppModel
    @State private var showingSettings = Launch.opensSettings
    @State private var showingOnboarding = !Onboarding.isDone

    var body: some View {
        VStack(spacing: 0) {
            if let error = model.loadError {
                HStack {
                    Text(error).fixedSize(horizontal: false, vertical: true)
                    Spacer()
                    Button("Try again", action: model.reload)
                }.padding(16).background(.yellow.opacity(0.12))
            }
            if let detail = model.detail {
                DayView(detail: detail, day: model.day, usualMs: model.usualMs, demoColors: model.isDemo,
                        previous: { model.stepDay(-1) }, next: { model.stepDay(1) }, settings: { showingSettings = true })
                    .id(model.day)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }.frame(minWidth: 720, minHeight: 560)
            .sheet(isPresented: $showingSettings) { SettingsView(model: model) }
            .sheet(isPresented: $showingOnboarding) { OnboardingView { showingOnboarding = false } }
    }
}
