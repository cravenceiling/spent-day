import SwiftUI

enum Onboarding {
    private static let key = "onboardingDone"

    static var isDone: Bool {
        get { UserDefaults.standard.bool(forKey: key) }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }
}

struct OnboardingView: View {
    let finish: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 8) {
                Image(systemName: "hourglass")
                    .font(.system(size: 34, weight: .medium))
                    .foregroundStyle(Color.accentColor.gradient)
                Text("Welcome to Spent Day")
                    .font(.title.weight(.semibold))
                Text("Spent Day records which app and website are in front of you, on this Mac and your other devices. It stores app names, website domains, times and this Mac's name. It never stores page addresses, page titles or window titles.")
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            VStack(alignment: .leading, spacing: 16) {
                Step(symbol: "safari", title: "Websites", detail: "macOS asks once per browser the first time you use Chrome or another Chromium browser. Safari sites are off until you turn them on in Settings. Firefox needs the Spent Day extension.") {
                    EmptyView()
                }
                Step(symbol: "lock.icloud", title: "Sync", detail: "End-to-end encrypted. Set it up later from Settings in the dashboard.") {
                    EmptyView()
                }
            }

            HStack {
                Spacer()
                Button("Get Started") {
                    Onboarding.isDone = true
                    finish()
                }
                .keyboardShortcut(.defaultAction)
                .controlSize(.large)
            }
        }
        .padding(28)
        .frame(width: 480)
    }
}

private struct Step<Accessory: View>: View {
    let symbol: String
    let title: String
    let detail: String
    @ViewBuilder var accessory: Accessory

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: symbol)
                .font(.title3)
                .foregroundStyle(Color.accentColor)
                .frame(width: 26)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.headline)
                Text(detail)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 12)
            accessory
        }
    }
}
