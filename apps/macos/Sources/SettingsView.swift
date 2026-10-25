import ServiceManagement
import SwiftUI

struct SettingsView: View {
    let model: AppModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            Form {
                SyncSection(model: model)
                GeneralSection()
            }
            .formStyle(.grouped)
            .scrollDisabled(true)
            .fixedSize(horizontal: false, vertical: true)

            Divider()
            HStack {
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(14)
        }
        .frame(width: 500)
    }
}

private struct SyncSection: View {
    let model: AppModel
    @State private var serverURL = ""
    @State private var joinCode = ""
    @State private var isJoining = false
    @State private var isWorking = false
    @State private var error: String?
    @State private var copied = false
    @State private var showsCode = false
    @State private var confirmingTurnOff = false

    var body: some View {
        Section {
            TextField("Server", text: $serverURL, prompt: Text("https://sync.example.com"))
                .textContentType(.URL)
                .onSubmit(saveServer)
            if let serverProblem {
                Text(serverProblem)
                    .font(.callout)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if let config = model.syncConfig {
                LabeledContent("Sync code") {
                    HStack(spacing: 8) {
                        if !showsCode {
                            Text("••••-••••-••••-…")
                                .font(.system(.callout, design: .monospaced))
                                .foregroundStyle(.secondary)
                                .accessibilityLabel("Hidden")
                        }
                        Button(showsCode ? "Hide" : "Show") { showsCode.toggle() }
                            .controlSize(.small)
                        Button(copied ? "Copied" : "Copy") { copy(config.syncCode) }
                            .controlSize(.small)
                    }
                }
                if showsCode {
                    Text(Self.twoLines(config.syncCode))
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                LabeledContent("Last sync") {
                    LastSyncText(model: model)
                }
                HStack {
                    Button("Turn Off Sync…", role: .destructive) { confirmingTurnOff = true }
                        .confirmationDialog("Turn off sync on this Mac?", isPresented: $confirmingTurnOff) {
                            Button("Turn Off Sync", role: .destructive) { run { try await model.turnOffSync() } }
                        } message: {
                            Text("History already on this Mac stays here. To sync again you'll need the sync code, so copy it first if no other device has it.")
                        }
                    Spacer()
                    if serverURL != config.serverUrl {
                        Button("Save Server") { saveServer() }
                            .disabled(!serverIsValid)
                    }
                    Button(model.isSyncing ? "Syncing…" : "Sync Now", action: model.syncNow)
                        .disabled(model.isSyncing)
                }
            } else if isJoining {
                TextField("Sync code", text: $joinCode, prompt: Text("Paste the code from another device"), axis: .vertical)
                    .font(.system(.callout, design: .monospaced))
                    .lineLimit(3, reservesSpace: true)
                    .onSubmit(join)
                HStack {
                    Spacer()
                    Button("Cancel") { isJoining = false }
                    Button("Join", action: join)
                        .disabled(!serverIsValid || joinCode.isEmpty || isWorking)
                        .keyboardShortcut(.defaultAction)
                }
            } else {
                HStack {
                    Spacer()
                    Button("Join with Code…") { isJoining = true }
                        .disabled(!serverIsValid)
                    Button("Create New Sync Code") { run { try await model.createSyncCode(serverURL: serverURL) } }
                        .disabled(!serverIsValid || isWorking)
                }
            }

            if let error {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.red)
                    .font(.callout)
            }
        } header: {
            Text("Sync")
        } footer: {
            Text(model.syncConfig == nil
                ? "Use one sync code on every device to see them all in one dashboard. Sync sends app names, domains, times and this Mac's name, encrypted with the sync code, so the server can't read them."
                : "Enter this code on your other devices. Anyone with it can read your history, so keep it private. Spent Day syncs every 5 minutes.")
                .font(.footnote)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .onAppear { serverURL = model.syncConfig?.serverUrl ?? serverURL }
        .onChange(of: model.syncConfig) { _, config in serverURL = config?.serverUrl ?? serverURL }
    }

    private var trimmedServerURL: String { serverURL.trimmingCharacters(in: .whitespacesAndNewlines) }

    private var serverIsValid: Bool { !trimmedServerURL.isEmpty && serverProblem == nil }

    /// The core accepts only an address that starts with http:// or https://.
    private var serverProblem: String? {
        let url = trimmedServerURL
        guard !url.isEmpty else { return nil }
        guard url.hasPrefix("https://") || url.hasPrefix("http://") else {
            return "Enter the full address, starting with https://. For example, https://sync.example.com."
        }
        guard let host = URL(string: url)?.host(), !host.isEmpty else {
            return "That doesn't look like a server address. For example, https://sync.example.com."
        }
        return nil
    }

    private func join() {
        guard serverIsValid, !joinCode.isEmpty else { return }
        run { try await model.configureSync(serverURL: serverURL, code: joinCode) }
    }

    private func saveServer() {
        guard let config = model.syncConfig, serverURL != config.serverUrl, serverIsValid else { return }
        run { try await model.configureSync(serverURL: serverURL, code: config.syncCode) }
    }

    private func run(_ action: @escaping @MainActor () async throws -> Void) {
        isWorking = true
        error = nil
        Task {
            do {
                try await action()
                isJoining = false
                joinCode = ""
            } catch {
                self.error = error.userMessage
            }
            isWorking = false
        }
    }

    /// The 14 groups of a sync code as two lines of 7, so it reads without wrapping mid-group.
    private static func twoLines(_ code: String) -> String {
        let groups = code.split(separator: "-")
        let half = (groups.count + 1) / 2
        return [groups.prefix(half), groups.dropFirst(half)].map { $0.joined(separator: "-") }.joined(separator: "\n")
    }

    private func copy(_ code: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(code, forType: .string)
        // Clipboard managers that follow nspasteboard.org don't keep items marked concealed.
        pasteboard.setData(Data(), forType: NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType"))
        copied = true
        Task {
            try? await Task.sleep(for: .seconds(1.5))
            copied = false
        }
    }
}

private struct LastSyncText: View {
    let model: AppModel

    var body: some View {
        switch (model.isSyncing, model.lastSync) {
        case (true, _):
            HStack(spacing: 6) {
                ProgressView().controlSize(.small)
                Text("Syncing…").foregroundStyle(.secondary)
            }
        case (false, .success(let report)?):
            VStack(alignment: .trailing, spacing: 2) {
                Text("\(Date(millis: report.finishedAtMs).formatted(.relative(presentation: .named)))  ·  \(report.pushedSpans) sent, \(report.pulledSpans) received")
                    .foregroundStyle(.secondary)
                if report.skippedBatches > 0 {
                    Text(report.skippedBatches == 1
                        ? "1 batch on the server couldn't be read and was skipped. A device may use a different sync code."
                        : "\(report.skippedBatches) batches on the server couldn't be read and were skipped. A device may use a different sync code.")
                        .foregroundStyle(.orange)
                        .multilineTextAlignment(.trailing)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        case (false, .failure(let message, let at)?):
            Text("Failed \(at.formatted(.relative(presentation: .named))). \(message)")
                .foregroundStyle(.red)
                .fixedSize(horizontal: false, vertical: true)
        case (false, nil):
            Text("Not yet").foregroundStyle(.secondary)
        }
    }
}

private struct GeneralSection: View {
    @State private var launchAtLogin = Self.isRegistered
    @State private var loginError: String?
    @AppStorage(SafariSites.defaultsKey) private var recordsSafariSites = false

    var body: some View {
        Section("General") {
            Toggle("Launch at login", isOn: $launchAtLogin)
                .onChange(of: launchAtLogin) { _, enabled in setLaunchAtLogin(enabled) }
            if let loginError {
                Text(loginError).font(.callout).foregroundStyle(.red)
            }
            Toggle(isOn: $recordsSafariSites) {
                Text("Record Safari sites")
                Text("Spent Day can't tell Safari Private windows apart from normal ones. With this on, private browsing is recorded.")
            }
        }
    }

    /// Registered, whether or not the user has approved it in Login Items yet.
    private static var isRegistered: Bool {
        [.enabled, .requiresApproval].contains(SMAppService.mainApp.status)
    }

    private func setLaunchAtLogin(_ enabled: Bool) {
        // Resetting the toggle after a failure calls this again. That call must not
        // undo anything or replace the error the user needs to see.
        guard enabled != Self.isRegistered else { return }
        do {
            if enabled { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            loginError = SMAppService.mainApp.status == .requiresApproval
                ? "Allow Spent Day in System Settings > General > Login Items to finish."
                : nil
        } catch {
            loginError = "macOS didn't accept the change: \(error.localizedDescription)"
            launchAtLogin = Self.isRegistered
        }
    }
}
