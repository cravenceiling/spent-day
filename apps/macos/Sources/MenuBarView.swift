import SwiftUI

struct MenuBarView: View {
    let model: AppModel
    let openDashboard: () -> Void
    let quit: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Today")
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.secondary)
                Text((model.today?.totalMs ?? 0).durationText)
                    .font(.system(size: 30, weight: .semibold, design: .rounded))
                    .monospacedDigit()
                    .contentTransition(.numericText())
            }
            .padding(.horizontal, 14)
            .padding(.top, 12)
            .padding(.bottom, 8)

            let apps = Array((model.today?.apps ?? []).prefix(3))
            if !apps.isEmpty {
                let top = apps[0].totalMs
                VStack(spacing: 8) {
                    ForEach(apps, id: \.appKey) { app in
                        HStack(spacing: 8) {
                            AppIcon(ids: app.appIds, name: app.appName, size: 18)
                            VStack(alignment: .leading, spacing: 4) {
                                Text(app.appName).lineLimit(1)
                                UsageBar(fraction: Double(app.totalMs) / Double(max(top, 1)))
                            }
                            Text(app.totalMs.durationText)
                                .monospacedDigit()
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .padding(.horizontal, 14)
                .padding(.bottom, 10)
            }

            Divider().padding(.horizontal, 8)

            VStack(spacing: 0) {
                Button {
                    dismiss()
                    openDashboard()
                } label: {
                    Text("Open Dashboard")
                }
                Button {
                    model.syncNow()
                } label: {
                    HStack {
                        Text(model.isSyncing ? "Syncing…" : "Sync Now")
                        Spacer()
                        if model.isSyncing { ProgressView().controlSize(.mini) }
                    }
                }
                .disabled(model.syncConfig == nil || model.isSyncing)
                .help(model.syncConfig == nil ? "Set up sync in the dashboard's settings" : "")
                Divider().padding(.vertical, 4).padding(.horizontal, 2)
                Button(action: quit) {
                    Text("Quit Spent Day")
                }
                .keyboardShortcut("q")
            }
            .buttonStyle(MenuRowButtonStyle())
            .padding(6)
        }
        .frame(width: 264)
        .onAppear(perform: model.reload)
    }
}
