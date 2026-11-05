import SwiftUI

struct DayDrill: Equatable {
    var deviceID: String?
    var itemKey: String?
}

struct DayView: View {
    let detail: DayDetail
    let day: Date
    let usualMs: Int64?
    var demoColors = false
    var previous: () -> Void = {}
    var next: () -> Void = {}
    var settings: () -> Void = {}
    @State private var drill: DayDrill
    @State private var hovered: String?
    @State private var showsDetails = false
    @FocusState private var rootFocused: Bool
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reducedMotion

    init(detail: DayDetail, day: Date, usualMs: Int64?, demoColors: Bool = false,
         initialDrill: DayDrill = DayDrill(), initialHover: String? = nil,
         previous: @escaping () -> Void = {}, next: @escaping () -> Void = {}, settings: @escaping () -> Void = {}) {
        self.detail = detail; self.day = day; self.usualMs = usualMs; self.demoColors = demoColors
        self.previous = previous; self.next = next; self.settings = settings
        _drill = State(initialValue: initialDrill)
        _hovered = State(initialValue: initialHover)
    }

    private var device: DeviceDay? { detail.devices.first { $0.device.id == drill.deviceID } }
    private var item: DayItem? { device?.items.first { $0.key == drill.itemKey } }
    private var sessions: [Session] { detail.devices.flatMap { $0.items.flatMap(\.sessions) } }
    private var totalMs: Int64 { item?.totalMs ?? device?.totalMs ?? detail.wallMs }
    private var dark: Bool { scheme == .dark }

    private var colors: [String: TileRGB] {
        let grouped = Dictionary(grouping: detail.devices.flatMap(\.items), by: \.key)
        var ranked: [(String, Int64, DayItem)] = grouped.map { key, items in
            let total = items.reduce(Int64(0)) { $0 + $1.totalMs }
            return (key, total, items[0])
        }
        ranked.sort { $0.1 == $1.1 ? $0.0 < $1.0 : $0.1 > $1.1 }
        let seeds = ranked.map { key, _, item -> (String, TileLCH) in
            let seed: TileLCH
            if demoColors { seed = DemoStore.colorSeed(for: item) }
            else if item.kind == .site { seed = TileColors.hashed(key) }
            else { seed = TileIconSampler.shared.seed(key: item.appKey, ids: item.appIds) }
            return (key, seed.c == 0 ? TileLCH(l: dark ? seed.l : 0.78, c: 0, h: 0) : seed.tuned(dark: dark))
        }
        return TileColors.separate(seeds, dark: dark)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 2) {
                Button(action: previous) { Image(systemName: "chevron.left").frame(width: 26, height: 26) }
                    .accessibilityLabel("Previous day").keyboardShortcut(.leftArrow, modifiers: .command)
                Text(day.formatted(.dateTime.weekday(.wide).day().month(.wide)))
                    .padding(.horizontal, 6)
                Button(action: next) { Image(systemName: "chevron.right").frame(width: 26, height: 26) }
                    .accessibilityLabel("Next day").keyboardShortcut(.rightArrow, modifiers: .command)
                    .disabled(Calendar.current.isDateInToday(day))
                Spacer()
                Button("Settings", action: settings).accessibilityLabel("Open settings")
            }
            .buttonStyle(.plain).font(.system(size: 13)).foregroundStyle(.secondary)
            .frame(height: 30)
            VStack(alignment: .leading, spacing: 6) {
                if let device {
                    HStack(spacing: 6) {
                        Button("All devices") { navigate(DayDrill()) }
                        Text("›")
                        Button(device.device.name) { navigate(DayDrill(deviceID: device.device.id)) }
                        if let item { Text("›"); Text(item.name).foregroundStyle(.primary) }
                    }.buttonStyle(.plain).font(.system(size: 13)).foregroundStyle(.secondary)
                }
                Text(totalMs.durationText)
                    .font(.system(size: 40, weight: .semibold)).tracking(-1).monospacedDigit()
                    .accessibilityLabel("\(totalMs.accessibleDuration) of screen time")
                if !sessions.isEmpty {
                    HStack(alignment: .firstTextBaseline, spacing: 10) {
                        Text(sentence).font(.system(size: 14, weight: .regular)).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                        if drill.deviceID == nil && !detail.devices.isEmpty {
                            Button("Details") { showsDetails.toggle() }.buttonStyle(.plain)
                                .font(.system(size: 13)).underline()
                                .accessibilityValue(showsDetails ? "Expanded" : "Collapsed")
                        }
                    }
                }
                if showsDetails && drill.deviceID == nil { details }
            }
            .padding(.top, 18).padding(.bottom, 18)
            if detail.wallMs == 0 {
                VStack(spacing: 8) {
                    Text("No screen time recorded").font(.title2.weight(.medium))
                    Text("Nothing was recorded for this day.").foregroundStyle(.secondary)
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                TreemapView(detail: detail, colors: colors, drill: $drill, hovered: $hovered)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                DayStrip(detail: detail, colors: colors,
                         hovered: hovered ?? item.map { (drill.deviceID ?? "") + "/" + $0.key })
                    .frame(height: 30).padding(.top, 16)
            }
        }
        .padding(.horizontal, 36).padding(.top, 22).padding(.bottom, 26)
        .background(dark ? Color(rgb: TileRGB(hex: 0x161618)) : Color(rgb: TileRGB(hex: 0xf7f7f9)))
        .focusable().focusEffectDisabled().focused($rootFocused)
        .onKeyPress(.escape) {
            guard drill.deviceID != nil else { return .ignored }
            navigate(drill.itemKey == nil ? DayDrill() : DayDrill(deviceID: drill.deviceID))
            return .handled
        }
        .onAppear { rootFocused = true }
        .onChange(of: drill) { _, _ in rootFocused = true }
    }

    private func navigate(_ path: DayDrill) {
        hovered = nil
        withAnimation(reducedMotion ? nil : .easeInOut(duration: 0.25)) { drill = path }
    }

    private var sentence: String {
        guard let first = sessions.map(\.startMs).min(), let last = sessions.map(\.endMs).max() else { return "" }
        if let device {
            let own = item?.sessions ?? device.items.flatMap(\.sessions)
            let from = own.map(\.startMs).min() ?? first, to = own.map(\.endMs).max() ?? last
            if let item { return "\(item.name) on \(device.device.name), \(item.sessions.count) \(item.sessions.count == 1 ? "session" : "sessions"), \(clock(from, dayEnd: detail.endMs)) to \(clock(to, dayEnd: detail.endMs))." }
            return "\(device.device.name), \(device.items.count) \(device.items.count == 1 ? "app or site" : "apps and sites"), \(clock(from, dayEnd: detail.endMs)) to \(clock(to, dayEnd: detail.endMs))."
        }
        var comparison = ""
        if let usualMs {
            let delta = detail.wallMs - usualMs
            let weekday = day.formatted(.dateTime.weekday(.wide))
            comparison = abs(delta) < 60_000 ? "About the same as a usual \(weekday). "
                : "\(abs(delta).durationText) \(delta > 0 ? "more" : "less") than a usual \(weekday). "
        }
        return comparison + "First screen \(clock(first, dayEnd: detail.endMs)), last \(clock(last, dayEnd: detail.endMs))."
    }

    private var details: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                Text("Time on two screens at once: \((detail.deviceMs - detail.wallMs).durationText). \(detail.deviceMs.durationText) of device time.")
                let stretches = detail.devices.flatMap { device in
                    unionSessions(device.items.flatMap(\.sessions)).map { (device.device.name, $0) }
                }
                if let longest = stretches.max(by: { $0.1.endMs - $0.1.startMs < $1.1.endMs - $1.1.startMs }) {
                    Text("Longest stretch: \((longest.1.endMs - longest.1.startMs).durationText) on \(longest.0), \(clock(longest.1.startMs, dayEnd: detail.endMs)) to \(clock(longest.1.endMs, dayEnd: detail.endMs)).")
                }
                let lateStart = Calendar.current.date(bySettingHour: 23, minute: 0, second: 0, of: day)!.millis
                let late = unionSessions(sessions).reduce(Int64(0)) { $0 + max(0, $1.endMs - max(lateStart, $1.startMs)) }
                Text("Time after 23:00: \(late.durationText).")
                ForEach(detail.devices, id: \.device.id) { device in
                    Text(device.device.name).fontWeight(.medium).padding(.top, 4)
                    ForEach(device.items, id: \.key) { item in
                        HStack { Text(item.name); Spacer(); Text(item.totalMs.durationText).monospacedDigit() }
                    }
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        }.font(.system(size: 13)).frame(maxHeight: 170).padding(.top, 6)
    }
}

struct DayStrip: View {
    let detail: DayDetail
    let colors: [String: TileRGB]
    var hovered: String?
    var only: MapItem?

    var body: some View {
        GeometryReader { proxy in
            Canvas { context, size in
                let track = CGRect(x: 0, y: 0, width: size.width, height: 10)
                context.fill(Path(roundedRect: track, cornerRadius: 5), with: .color(.primary.opacity(0.08)))
                let all = detail.devices.flatMap { device in device.items.flatMap { item in
                    item.sessions.map { (device.device.id, item, $0) }
                } }.sorted { $0.2.startMs < $1.2.startMs }
                for (device, item, session) in all {
                    if let only, !only.sessions.contains(session) { continue }
                    let denominator = Double(max(1, detail.endMs - detail.startMs))
                    let x = Double(session.startMs - detail.startMs) / denominator * size.width
                    let width = max(2, Double(session.endMs - session.startMs) / denominator * size.width)
                    let color = Color(rgb: colors[item.key] ?? TileColors.hashed(item.key).rgb)
                    let identity = device + "/" + item.key
                    let other = hovered == device + "/other" && Double(item.totalMs) < Double(detail.wallMs) * 0.003
                    let hot = hovered == nil || hovered == identity || hovered?.hasPrefix(identity + "/") == true || other
                    context.opacity = hot ? 1 : 0.18
                    context.fill(Path(roundedRect: CGRect(x: x,y: 0,width: width,height: only == nil ? 10 : 8), cornerRadius: 2), with: .color(color))
                }
            }
            ForEach([0, 6, 12, 18, 24], id: \.self) { hour in
                let instant = hour == 24 ? detail.endMs : Calendar.current.date(bySettingHour: hour, minute: 0, second: 0, of: Date(millis: detail.startMs))!.millis
                let fraction = Double(instant - detail.startMs) / Double(max(1, detail.endMs - detail.startMs))
                let x = hour == 0 ? 8 : hour == 24 ? proxy.size.width - 8 : fraction * proxy.size.width
                Text(String(format: "%02d", hour)).font(.system(size: 12)).foregroundStyle(.secondary).monospacedDigit()
                    .position(x: x, y: 21)
            }
        }
        .accessibilityElement(children: .ignore).accessibilityLabel("Screen time across the day, midnight to midnight")
    }
}

extension Color {
    init(rgb: TileRGB) { self.init(.sRGB, red: rgb.r, green: rgb.g, blue: rgb.b, opacity: 1) }
}
