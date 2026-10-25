import Foundation

enum DayRange: String, CaseIterable, Identifiable {
    case today, yesterday, lastSevenDays

    var id: Self { self }

    var title: String {
        switch self {
        case .today: "Today"
        case .yesterday: "Yesterday"
        case .lastSevenDays: "Last 7 Days"
        }
    }

    var isMultiDay: Bool { self == .lastSevenDays }

    /// Local-midnight boundaries. The core never sees time zones.
    func interval(now: Date = .now, calendar: Calendar = .current) -> DateInterval {
        let today = calendar.startOfDay(for: now)
        let day = { (offset: Int) in calendar.date(byAdding: .day, value: offset, to: today)! }
        switch self {
        case .today: return DateInterval(start: today, end: day(1))
        case .yesterday: return DateInterval(start: day(-1), end: today)
        case .lastSevenDays: return DateInterval(start: day(-6), end: day(1))
        }
    }

    /// Always hourly: the week view sums hours into local days itself, which stays
    /// exact across DST changes where a fixed 24 h bucket would drift.
    func query(deviceID: String?, now: Date = .now) -> SummaryQuery {
        let range = interval(now: now)
        return SummaryQuery(startMs: range.start.millis, endMs: range.end.millis, bucketMs: 3_600_000, deviceId: deviceID)
    }
}

struct ChartBar: Identifiable, Equatable {
    let start: Date
    let millis: Int64
    var id: Date { start }
}

extension Summary {
    func bars(for range: DayRange, calendar: Calendar = .current) -> [ChartBar] {
        let hourly = buckets.enumerated().map { index, millis in
            ChartBar(start: Date(millis: startMs + Int64(index) * 3_600_000), millis: millis)
        }
        guard range.isMultiDay else { return hourly }
        let byDay = Dictionary(grouping: hourly) { calendar.startOfDay(for: $0.start) }
        return byDay.map { day, hours in ChartBar(start: day, millis: hours.reduce(0) { $0 + $1.millis }) }
            .sorted { $0.start < $1.start }
    }
}
