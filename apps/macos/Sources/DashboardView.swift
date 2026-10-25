import AppKit
import Charts
import SwiftUI

/// The dashboard's window. `snapshot.sh` draws the same window offscreen.
@MainActor enum DashboardWindow {
    static func make(model: AppModel) -> NSWindow {
        let host = NSHostingController(rootView: DashboardView(model: model))
        host.sceneBridgingOptions = [.toolbars, .title]
        host.sizingOptions = []
        let window = NSWindow(contentViewController: host)
        window.title = "Spent Day"
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView]
        window.toolbarStyle = .unified
        window.setContentSize(NSSize(width: 860, height: 760))
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
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                if let error = model.loadError {
                    ErrorBanner(message: error, retry: model.reload)
                }
                if let summary = model.summary {
                    Hero(summary: summary, range: model.range, showsDevices: model.deviceID == nil)
                    UsageChart(summary: summary, range: model.range)
                    if summary.totalMs == 0 {
                        EmptyState(range: model.range)
                    } else {
                        HStack(alignment: .top, spacing: 16) {
                            AppList(apps: summary.apps)
                            SiteList(sites: summary.sites)
                        }
                    }
                } else if model.loadError == nil {
                    ProgressView().frame(maxWidth: .infinity, minHeight: 300)
                }
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 20)
            .animation(.snappy(duration: 0.25), value: model.summary)
        }
        .frame(minWidth: 720, minHeight: 560)
        .navigationTitle("Spent Day")
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("Range", selection: $model.range) {
                    ForEach(DayRange.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
            }
            ToolbarItemGroup(placement: .primaryAction) {
                DevicePicker(model: model)
                Button {
                    showingSettings = true
                } label: {
                    Label("Settings", systemImage: "gearshape")
                }
                .help("Sync and general settings")
            }
        }
        .sheet(isPresented: $showingSettings) {
            SettingsView(model: model)
        }
        .sheet(isPresented: $showingOnboarding) {
            OnboardingView { showingOnboarding = false }
        }
    }
}

private struct DevicePicker: View {
    @Bindable var model: AppModel

    var body: some View {
        Menu {
            Picker("Device", selection: $model.deviceID) {
                Label("All Devices", systemImage: "rectangle.stack").tag(String?.none)
                Divider()
                ForEach(model.devices, id: \.id) { device in
                    Label(device.name, systemImage: device.platform.symbol).tag(Optional(device.id))
                }
            }
            .pickerStyle(.inline)
        } label: {
            Label(model.selectedDeviceName, systemImage: model.devices.first { $0.id == model.deviceID }?.platform.symbol ?? "rectangle.stack")
                .labelStyle(.titleAndIcon)
        }
        .menuIndicator(.visible)
        .fixedSize()
        .help("Show one device or all of them")
    }
}

extension Platform {
    var symbol: String {
        switch self {
        case .macos: "laptopcomputer"
        case .linux: "desktopcomputer"
        case .android: "iphone"
        case .ios: "iphone"
        case .windows: "pc"
        }
    }
}

private struct Hero: View {
    let summary: Summary
    let range: DayRange
    let showsDevices: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(caption)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(.secondary)
            Text(summary.totalMs.durationText)
                .font(.system(size: 46, weight: .semibold, design: .rounded))
                .monospacedDigit()
                .contentTransition(.numericText())
            if showsDevices, summary.byDevice.count > 1 {
                HStack(spacing: 14) {
                    ForEach(summary.byDevice.filter { $0.totalMs > 0 }, id: \.device.id) { usage in
                        Label {
                            Text(usage.device.name) + Text("  \(usage.totalMs.durationText)").foregroundStyle(.secondary)
                        } icon: {
                            Image(systemName: usage.device.platform.symbol).foregroundStyle(.secondary)
                        }
                        .font(.callout)
                    }
                }
                .padding(.top, 2)
            }
        }
    }

    private var caption: String {
        let start = Date(millis: summary.startMs)
        switch range {
        case .today, .yesterday:
            return "\(range.title), \(start.formatted(.dateTime.weekday(.wide).day().month(.wide)))"
        case .lastSevenDays:
            let last = Date(millis: summary.endMs - 1)
            let average = summary.totalMs / 7
            return "\(start.formatted(.dateTime.day().month())) – \(last.formatted(.dateTime.day().month()))  ·  \(average.durationText) daily average"
        }
    }
}

private struct UsageChart: View {
    let summary: Summary
    let range: DayRange
    @State private var selection: Date?

    private var bars: [ChartBar] { summary.bars(for: range) }
    private var unit: Calendar.Component { range.isMultiDay ? .day : .hour }
    /// Minutes per hour for a day, hours per day for the week.
    private var scale: Double { range.isMultiDay ? 3_600_000 : 60_000 }

    var body: some View {
        Card(title: range.isMultiDay ? "Daily" : "Hourly", trailing: selectedLabel) {
            Chart {
                ForEach(bars) { bar in
                    BarMark(
                        x: .value("Time", bar.start, unit: unit),
                        y: .value("Usage", Double(bar.millis) / scale)
                    )
                    .cornerRadius(3)
                    .foregroundStyle(Color.accentColor.gradient)
                    .opacity(selected == nil || selected == bar ? 1 : 0.4)
                }
                if range.isMultiDay, summary.totalMs > 0 {
                    RuleMark(y: .value("Average", Double(summary.totalMs) / 7 / scale))
                        .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
                        .foregroundStyle(.secondary)
                        .annotation(position: .top, alignment: .leading) {
                            Text("avg").font(.caption2).foregroundStyle(.secondary)
                        }
                }
            }
            .chartXSelection(value: $selection)
            .chartXScale(domain: Date(millis: summary.startMs)...Date(millis: summary.endMs))
            .chartXAxis {
                if range.isMultiDay {
                    AxisMarks(values: .stride(by: .day)) { _ in
                        AxisValueLabel(format: .dateTime.weekday(.abbreviated), centered: true)
                    }
                } else {
                    AxisMarks(values: .stride(by: .hour, count: 6)) { _ in
                        AxisGridLine()
                        AxisValueLabel(format: .dateTime.hour())
                    }
                }
            }
            .chartYAxis {
                AxisMarks(position: .trailing, values: .automatic(desiredCount: 3)) { value in
                    AxisGridLine()
                    AxisValueLabel {
                        if let amount = value.as(Double.self) {
                            Text(range.isMultiDay ? "\(Int(amount))h" : "\(Int(amount))m")
                        }
                    }
                }
            }
            .chartYScale(domain: 0...yMax)
            .frame(height: 190)
        }
    }

    private var yMax: Double {
        let peak = (bars.map(\.millis).max() ?? 0)
        return max(Double(peak) / scale, range.isMultiDay ? 1 : 15)
    }

    private var selected: ChartBar? {
        guard let selection else { return nil }
        let calendar = Calendar.current
        return bars.first { calendar.isDate($0.start, equalTo: selection, toGranularity: unit) }
    }

    private var selectedLabel: String? {
        guard let selected else { return nil }
        let when = range.isMultiDay
            ? selected.start.formatted(.dateTime.weekday(.wide).day().month())
            : "\(selected.start.formatted(.dateTime.hour())) – \(selected.start.addingTimeInterval(3600).formatted(.dateTime.hour()))"
        return "\(when)  \(selected.millis.durationText)"
    }
}

private struct AppList: View {
    let apps: [AppUsage]
    @State private var expanded: Set<String> = []
    @State private var showsAll = false

    private static let collapsedCount = 10

    var body: some View {
        Card(title: "Apps", trailing: "\(apps.count)") {
            let top = apps.first?.totalMs ?? 1
            VStack(spacing: 2) {
                ForEach(showsAll ? apps : Array(apps.prefix(Self.collapsedCount)), id: \.appKey) { app in
                    AppRow(app: app, fraction: Double(app.totalMs) / Double(top), isExpanded: expanded.contains(app.appKey)) {
                        withAnimation(.snappy(duration: 0.2)) {
                            if expanded.contains(app.appKey) { expanded.remove(app.appKey) } else { expanded.insert(app.appKey) }
                        }
                    }
                }
            }
            if apps.count > Self.collapsedCount {
                ShowAllButton(showsAll: $showsAll, hidden: apps.count - Self.collapsedCount)
            }
        }
    }
}

private struct AppRow: View {
    let app: AppUsage
    let fraction: Double
    let isExpanded: Bool
    let toggle: () -> Void

    /// Sites by time, then the browser's time with no site, which reads as "Other".
    private var sites: [DomainUsage] {
        app.domains.filter { !$0.domain.isEmpty } + app.domains.filter(\.domain.isEmpty)
    }

    private var expandable: Bool { app.isBrowser && app.domains.contains { !$0.domain.isEmpty } }

    private var summaryRow: some View {
        HStack(spacing: 10) {
            AppIcon(ids: app.appIds, name: app.appName)
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 4) {
                    Text(app.appName).lineLimit(1)
                    if expandable {
                        Image(systemName: "chevron.right")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.tertiary)
                            .rotationEffect(.degrees(isExpanded ? 90 : 0))
                    }
                }
                UsageBar(fraction: fraction)
            }
            Text(app.totalMs.durationText)
                .monospacedDigit()
                .foregroundStyle(.secondary)
                .frame(minWidth: 56, alignment: .trailing)
        }
        .padding(.vertical, 5)
        .contentShape(Rectangle())
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if expandable {
                Button(action: toggle) { summaryRow }
                    .buttonStyle(.plain)
                    .help(isExpanded ? "Hide sites" : "Show sites")
            } else {
                summaryRow
            }

            if expandable, isExpanded {
                VStack(spacing: 2) {
                    ForEach(sites, id: \.domain) { site in
                        DomainRow(site: site, fraction: Double(site.totalMs) / Double(max(app.totalMs, 1)), compact: true)
                    }
                }
                .padding(.leading, 32)
                .padding(.bottom, 4)
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
    }
}

private struct SiteList: View {
    let sites: [DomainUsage]
    @State private var showsAll = false

    private static let collapsedCount = 10

    var body: some View {
        Card(title: "Sites", trailing: sites.isEmpty ? nil : "\(sites.count)") {
            if sites.isEmpty {
                Text("Websites appear here when you browse in Chrome or another supported browser. For Safari, turn on Record Safari sites in Settings.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.vertical, 6)
            } else {
                let top = sites.first?.totalMs ?? 1
                VStack(spacing: 2) {
                    ForEach(showsAll ? sites : Array(sites.prefix(Self.collapsedCount)), id: \.domain) { site in
                        DomainRow(site: site, fraction: Double(site.totalMs) / Double(top), compact: false)
                    }
                }
                if sites.count > Self.collapsedCount {
                    ShowAllButton(showsAll: $showsAll, hidden: sites.count - Self.collapsedCount)
                }
            }
        }
    }
}

private struct DomainRow: View {
    let site: DomainUsage
    let fraction: Double
    let compact: Bool

    var body: some View {
        HStack(spacing: 10) {
            if !compact {
                RoundedRectangle(cornerRadius: 5, style: .continuous)
                    .fill(Monogram.color(for: site.domain).opacity(0.18))
                    .frame(width: 22, height: 22)
                    .overlay {
                        Text(site.domain.prefix(1).uppercased())
                            .font(.system(size: 11, weight: .semibold, design: .rounded))
                            .foregroundStyle(Monogram.color(for: site.domain))
                    }
            }
            VStack(alignment: .leading, spacing: compact ? 3 : 5) {
                Text(site.domain.isEmpty ? "Other" : site.domain)
                    .font(compact ? .callout : .body)
                    .foregroundStyle(compact ? .secondary : .primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                UsageBar(fraction: fraction, tint: compact ? .secondary : .teal)
            }
            Text(site.totalMs.durationText)
                .font(compact ? .callout : .body)
                .monospacedDigit()
                .foregroundStyle(.secondary)
                .frame(minWidth: 56, alignment: .trailing)
        }
        .padding(.vertical, compact ? 3 : 5)
    }
}

private struct ShowAllButton: View {
    @Binding var showsAll: Bool
    let hidden: Int

    var body: some View {
        Button(showsAll ? "Show Less" : "Show \(hidden) More") {
            withAnimation(.snappy(duration: 0.2)) { showsAll.toggle() }
        }
        .buttonStyle(.link)
        .font(.callout)
        .padding(.top, 2)
    }
}

private struct EmptyState: View {
    let range: DayRange

    var body: some View {
        ContentUnavailableView {
            Label("No Screen Time Yet", systemImage: "hourglass")
        } description: {
            Text(range == .today
                ? "Spent Day records the app in front of you while you use this Mac. Check back in a few minutes."
                : "Nothing was recorded for this period.")
        }
        .frame(maxWidth: .infinity, minHeight: 220)
    }
}

private struct ErrorBanner: View {
    let message: String
    let retry: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.yellow)
            Text(message).fixedSize(horizontal: false, vertical: true)
            Spacer()
            Button("Try Again", action: retry)
        }
        .padding(12)
        .background(.yellow.opacity(0.12), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
    }
}
