import SwiftUI
import AppKit

// MARK: - App Entry Point

@main
struct OpenSlickyApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    @StateObject private var vm = ViewModel()

    var body: some Scene {
        MenuBarExtra {
            PopoverContent()
                .environmentObject(vm)
        } label: {
            Image(systemName: vm.isOn ? "lightbulb.fill" : "lightbulb")
        }
        .menuBarExtraStyle(.window)

        Window("OpenSlicky", id: "main") {
            FullWindowView()
                .environmentObject(vm)
        }
        .defaultSize(width: 400, height: 600)
        .windowResizability(.contentSize)
    }
}

// MARK: - App Delegate

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        let showInDock = UserDefaults.standard.bool(forKey: "showInDock")
        NSApp.setActivationPolicy(showInDock ? .regular : .accessory)
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag {
            // When user clicks dock icon with no visible window, open the main window
            for window in sender.windows {
                if window.identifier?.rawValue.contains("main") == true {
                    window.makeKeyAndOrderFront(nil)
                    sender.activate(ignoringOtherApps: true)
                    return true
                }
            }
        }
        return true
    }
}

// MARK: - ViewModel

/// Decoded custom preset from CLI JSON output.
struct CustomPresetInfo: Codable, Identifiable {
    let name: String
    let color: String
    let animation: String?
    let speed: Double

    var id: String { name }
}

/// Decoded update status from `slicky update status` JSON output.
struct UpdateStatusInfo: Codable {
    let current_version: String
    let latest_version: String?
    let update_available: Bool
    let last_check: String?
    let download_url: String?
}

/// Decoded install result from `slicky update install` JSON output.
struct InstallResultInfo: Codable {
    let status: String
    let version: String?
    let error: String?
}

final class ViewModel: ObservableObject {
    let cli = SlickyCLI()

    private enum SavedLightState {
        case preset(String)
        case customColor(Color)
        case animation(type: String, color: String, speed: Double)
    }

    private var savedState: SavedLightState?
    @Published var isCustomColorActive = false

    var isOn: Bool {
        currentPreset != nil || isAnimating || isCustomColorActive
    }

    var canTurnOn: Bool {
        savedState != nil
    }

    @Published var deviceConnected = false
    @Published var slackConnected = false
    @Published var currentPreset: String? = nil
    @Published var isInstalled = false
    @Published var isInstalling = false
    @Published var showInDock: Bool = false {
        didSet {
            UserDefaults.standard.set(showInDock, forKey: "showInDock")
            NSApp.setActivationPolicy(showInDock ? .regular : .accessory)
        }
    }
    @Published var autoSyncSlack: Bool = false {
        didSet {
            UserDefaults.standard.set(autoSyncSlack, forKey: "autoSyncSlack")
        }
    }

    // Animation state
    @Published var isAnimating = false
    @Published var selectedAnimationType = "breathing"
    @Published var animationSpeed: Double = 1.0

    // Intensity (brightness cap)
    @Published var intensity: Double = 1.0 {
        didSet {
            UserDefaults.standard.set(intensity, forKey: "lightIntensity")
        }
    }

    // Color picker state
    @Published var pickerColor = Color.white

    // Custom presets
    @Published var customPresets: [CustomPresetInfo] = []

    // Update state
    @Published var updateAvailable = false
    @Published var latestVersion: String?
    @Published var isUpdating = false
    @Published var updateError: String?
    @Published var updateInstalled = false

    private var refreshTimer: Timer?
    private var updateTimer: Timer?
    private var animationProcess: Process?
    private var refreshTask: Task<Void, Never>?

    init() {
        _showInDock = Published(initialValue: UserDefaults.standard.bool(forKey: "showInDock"))
        _autoSyncSlack = Published(initialValue: UserDefaults.standard.bool(forKey: "autoSyncSlack"))
        let savedIntensity = UserDefaults.standard.double(forKey: "lightIntensity")
        _intensity = Published(initialValue: savedIntensity > 0 ? savedIntensity : 1.0)
        isInstalled = cli.isInstalled
        startPolling()
        loadCustomPresets()
        startUpdateChecking()
    }

    deinit {
        refreshTimer?.invalidate()
        updateTimer?.invalidate()
    }

    func startPolling() {
        refresh()
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 5.0, repeats: true) { [weak self] _ in
            self?.refresh()
        }
    }

    func refresh() {
        refreshTask?.cancel()
        refreshTask = Task { @MainActor in
            let dev = await cli.isDeviceConnected()
            let slack = await cli.isSlackConnected()
            guard !Task.isCancelled else { return }
            self.deviceConnected = dev
            self.slackConnected = slack
        }
    }

    // MARK: - Light Control

    func setPreset(_ name: String) {
        stopAnimation()
        currentPreset = name
        isCustomColorActive = false
        // Sync picker color to match the selected preset
        let presetColor: Color
        if let customPreset = customPresets.first(where: { $0.name == name }) {
            presetColor = colorFromHex(customPreset.color)
        } else {
            presetColor = colorForPreset(name)
        }
        pickerColor = presetColor
        Task {
            let ok: Bool
            if intensity < 1.0 {
                let scaledHex = presetColor.scaledHex(intensity: intensity)
                ok = await cli.setHex(scaledHex)
            } else {
                ok = await cli.set(preset: name)
            }
            if !ok {
                await MainActor.run {
                    self.currentPreset = nil
                }
            }
            if ok && autoSyncSlack && slackConnected {
                await syncSlackStatus(for: name)
            }
        }
    }

    func turnOff() {
        // Save current state before turning off
        if isAnimating {
            let colorHex = pickerColor.toHex()
            savedState = .animation(type: selectedAnimationType, color: colorHex, speed: animationSpeed)
        } else if let preset = currentPreset {
            savedState = .preset(preset)
        } else if isCustomColorActive {
            savedState = .customColor(pickerColor)
        }

        stopAnimation()
        currentPreset = nil
        isCustomColorActive = false
        Task {
            let _ = await cli.off()
            if autoSyncSlack && slackConnected {
                let _ = await cli.slackClearStatus()
            }
        }
    }

    func turnOn() {
        guard let state = savedState else { return }
        switch state {
        case .preset(let name):
            setPreset(name)
        case .customColor(let color):
            pickerColor = color
            setPickerColor()
        case .animation(let type, let color, let speed):
            selectedAnimationType = type
            pickerColor = colorFromHex(color)
            animationSpeed = speed
            startAnimation(type: type, color: color, speed: speed)
        }
    }

    func setPickerColor() {
        stopAnimation()
        currentPreset = nil
        isCustomColorActive = true
        let hex = pickerColor.scaledHex(intensity: intensity)
        Task {
            let _ = await cli.setHex(hex)
        }
    }

    // MARK: - Animation

    func startAnimation(type animType: String, color: String = "white", speed: Double = 1.0) {
        stopAnimation()
        isAnimating = true
        currentPreset = nil
        animationProcess = cli.animate(type: animType, color: color, speed: speed, brightness: intensity)
    }

    func stopAnimation() {
        cli.stopAnimation(animationProcess)
        animationProcess = nil
        isAnimating = false
    }

    // MARK: - Custom Presets

    func loadCustomPresets() {
        Task { @MainActor in
            let json = await cli.listPresetsJSON()
            guard let data = json.data(using: .utf8) else { return }
            if let presets = try? JSONDecoder().decode([CustomPresetInfo].self, from: data) {
                self.customPresets = presets
            }
        }
    }

    // MARK: - Slack

    func disconnectSlack() {
        Task { @MainActor in
            let ok = await cli.slackDisconnect()
            if ok {
                slackConnected = false
            }
        }
    }

    private func syncSlackStatus(for preset: String) async {
        switch preset.lowercased() {
        case "available":
            let _ = await cli.slackSetStatus(text: "", emoji: "")
        case "busy":
            let _ = await cli.slackSetStatus(text: "Busy", emoji: ":no_entry:")
        case "away":
            let _ = await cli.slackSetStatus(text: "Away", emoji: ":away:")
        case "in-meeting":
            let _ = await cli.slackSetStatus(text: "In a meeting", emoji: ":calendar:")
        default:
            break
        }
    }

    // MARK: - Update

    func startUpdateChecking() {
        checkForUpdates()
        updateTimer = Timer.scheduledTimer(withTimeInterval: 1800, repeats: true) { [weak self] _ in
            self?.checkForUpdates()
        }
    }

    func checkForUpdates() {
        Task { @MainActor in
            let (output, ok) = await cli.updateStatus()
            guard ok else { return }
            let trimmed = output.trimmingCharacters(in: .whitespacesAndNewlines)
            guard let data = trimmed.data(using: .utf8),
                  let info = try? JSONDecoder().decode(UpdateStatusInfo.self, from: data) else {
                return
            }
            self.updateAvailable = info.update_available
            self.latestVersion = info.latest_version
        }
    }

    func installUpdate() {
        isUpdating = true
        updateError = nil
        Task { @MainActor in
            let (output, _) = await cli.installUpdate()
            let trimmed = output.trimmingCharacters(in: .whitespacesAndNewlines)

            if let data = trimmed.data(using: .utf8),
               let result = try? JSONDecoder().decode(InstallResultInfo.self, from: data) {
                if result.status == "installed" {
                    self.updateInstalled = true
                    self.updateAvailable = false
                } else if result.status == "up_to_date" {
                    self.updateAvailable = false
                } else if result.error == "permission_denied" {
                    // Fall back to admin-privileged install (off MainActor).
                    let cli = self.cli
                    Task.detached {
                        let adminOk = cli.installUpdateAdmin()
                        await MainActor.run {
                            if adminOk {
                                self.updateInstalled = true
                                self.updateAvailable = false
                            } else {
                                self.updateError = "Installation cancelled"
                            }
                            self.isUpdating = false
                        }
                    }
                    return
                } else if let error = result.error {
                    self.updateError = error
                }
            } else {
                self.updateError = "Installation failed"
            }
            self.isUpdating = false
        }
    }

    func restartApp() {
        guard let appURL = NSWorkspace.shared.urlForApplication(
            withBundleIdentifier: Bundle.main.bundleIdentifier ?? "com.openslicky.app"
        ) else {
            NSApp.terminate(nil)
            return
        }
        let config = NSWorkspace.OpenConfiguration()
        config.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: appURL, configuration: config) { _, _ in
            DispatchQueue.main.async {
                NSApp.terminate(nil)
            }
        }
    }

    // MARK: - Install / Uninstall

    func install() {
        isInstalling = true
        Task.detached { [cli] in
            let ok = cli.installSymlinks()
            if ok {
                let _ = await cli.startupEnable()
                cli.writeMarker()
            }
            await MainActor.run {
                if ok { self.isInstalled = true }
                self.isInstalling = false
            }
        }
    }

    func uninstall() {
        // Stop the animation subprocess tracked by this app
        stopAnimation()
        currentPreset = nil

        // Run the rest on a background thread to avoid blocking the main thread
        Task.detached { [cli] in
            // 1. Unload LaunchAgent and kill slickyd (wait for confirmed exit)
            cli.stopDaemon()

            // 2. Now that no other process holds the HID handle, turn off the light
            let _ = await cli.off()

            // 3. Remove install markers
            cli.removeMarkers()

            // 4. Remove symlinks + app bundle (single admin prompt)
            let ok = cli.removeSymlinksAndApp()

            await MainActor.run {
                if ok {
                    NSApp.terminate(nil)
                }
                // If admin was cancelled, app stays open — user can retry
            }
        }
    }
}

// MARK: - Popover Content (Router)

struct PopoverContent: View {
    @EnvironmentObject var vm: ViewModel

    var body: some View {
        Group {
            if vm.cli.isTranslocated {
                TranslocationWarningView()
            } else if !vm.isInstalled {
                InstallerView()
            } else {
                MenuBarView()
            }
        }
        .frame(width: 340)
    }
}

// MARK: - Translocation Warning

struct TranslocationWarningView: View {
    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 36))
                .foregroundColor(.orange)

            Text("Move to Applications")
                .font(.headline)

            Text("Please drag OpenSlicky to your Applications folder first, then open it from there.")
                .font(.callout)
                .multilineTextAlignment(.center)
                .foregroundColor(.secondary)
        }
        .padding(24)
    }
}

// MARK: - Installer View

struct InstallerView: View {
    @EnvironmentObject var vm: ViewModel

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "lightbulb.circle.fill")
                .font(.system(size: 48))
                .foregroundColor(.accentColor)

            Text("Install OpenSlicky")
                .font(.title3.bold())

            Text("v\(vm.cli.appVersion)")
                .font(.caption)
                .foregroundColor(.secondary)

            VStack(alignment: .leading, spacing: 6) {
                Label("Create CLI at /usr/local/bin/slicky", systemImage: "terminal")
                Label("Start daemon on login", systemImage: "arrow.clockwise")
                Label("Admin password required", systemImage: "lock.shield")
            }
            .font(.caption)
            .foregroundColor(.secondary)
            .padding(12)
            .background(RoundedRectangle(cornerRadius: 6).fill(Color.gray.opacity(0.1)))

            Button(action: { vm.install() }) {
                if vm.isInstalling {
                    ProgressView()
                        .controlSize(.small)
                        .padding(.horizontal, 40)
                } else {
                    Text("Install")
                        .frame(minWidth: 100)
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(vm.isInstalling)
        }
        .padding(24)
    }
}

// MARK: - Menu Bar View (compact)

struct MenuBarView: View {
    @EnvironmentObject var vm: ViewModel

    var body: some View {
        VStack(spacing: 16) {
            UpdateBannerView()
            StatusSection()
            if vm.deviceConnected {
                Divider()
                ColorGridSection()
                Divider()
                IntensitySection()
            }
            Divider()
            Text("OpenSlicky v\(vm.cli.appVersion)")
                .font(.caption2)
                .foregroundColor(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(16)
    }
}

// MARK: - Full Window View

struct FullWindowView: View {
    @EnvironmentObject var vm: ViewModel

    var body: some View {
        ScrollView {
            VStack(spacing: 16) {
                UpdateBannerView()
                StatusSection()
                Divider()
                ColorGridSection()
                if !vm.customPresets.isEmpty {
                    Divider()
                    CustomPresetsSection()
                }
                Divider()
                ColorPickerSection()
                Divider()
                IntensitySection()
                Divider()
                AnimationSection()
                Divider()
                SlackSection()
                Divider()
                SettingsSection()
                Divider()
                FooterSection()
            }
            .padding(16)
        }
    }
}

// MARK: - Update Banner

struct UpdateBannerView: View {
    @EnvironmentObject var vm: ViewModel

    var body: some View {
        if vm.updateInstalled {
            HStack(spacing: 8) {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundColor(.white)
                Text("Update installed!")
                    .font(.caption.bold())
                    .foregroundColor(.white)
                Spacer()
                Button("Restart") {
                    vm.restartApp()
                }
                .buttonStyle(.bordered)
                .controlSize(.mini)
                .tint(.white)
            }
            .padding(10)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.green))
        } else if let error = vm.updateError {
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundColor(.white)
                Text(error)
                    .font(.caption)
                    .foregroundColor(.white)
                    .lineLimit(2)
                Spacer()
                Button("Retry") {
                    vm.installUpdate()
                }
                .buttonStyle(.bordered)
                .controlSize(.mini)
                .tint(.white)
            }
            .padding(10)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.red))
        } else if vm.isUpdating {
            HStack(spacing: 8) {
                ProgressView()
                    .controlSize(.small)
                Text("Downloading and installing...")
                    .font(.caption)
                    .foregroundColor(.white)
                Spacer()
            }
            .padding(10)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.blue))
        } else if vm.updateAvailable, let version = vm.latestVersion {
            HStack(spacing: 8) {
                Image(systemName: "arrow.down.circle.fill")
                    .foregroundColor(.white)
                Text("Update Available: v\(version)")
                    .font(.caption.bold())
                    .foregroundColor(.white)
                Spacer()
                Button("Install Update") {
                    vm.installUpdate()
                }
                .buttonStyle(.bordered)
                .controlSize(.mini)
                .tint(.white)
            }
            .padding(10)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.blue))
        }
    }
}

// MARK: - Status Section

struct StatusSection: View {
    @EnvironmentObject var vm: ViewModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(vm.deviceConnected ? Color.green : Color.red)
                .frame(width: 8, height: 8)
            Text(vm.deviceConnected ? "Device connected" : "No device")
                .font(.caption)
                .foregroundColor(vm.deviceConnected ? .primary : .secondary)

            Spacer()

            if vm.isAnimating {
                Image(systemName: "waveform")
                    .font(.caption)
                Text("Animating")
                    .font(.caption)
            } else if let preset = vm.currentPreset {
                Circle()
                    .fill(colorForPreset(preset))
                    .frame(width: 8, height: 8)
                Text(displayName(for: preset))
                    .font(.caption)
            } else if vm.isCustomColorActive {
                Circle()
                    .fill(vm.pickerColor)
                    .frame(width: 8, height: 8)
                Text(vm.pickerColor.toHex())
                    .font(.caption)
            } else {
                Text("Off")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }

            Button(action: {
                if vm.isOn {
                    vm.turnOff()
                } else {
                    vm.turnOn()
                }
            }) {
                Image(systemName: "power")
            }
            .buttonStyle(.bordered)
            .controlSize(.mini)
            .disabled(!vm.isOn && !vm.canTurnOn)

            Button(action: {
                openWindow(id: "main")
                NSApp.activate(ignoringOtherApps: true)
            }) {
                Image(systemName: "gearshape")
            }
            .buttonStyle(.bordered)
            .controlSize(.mini)
        }
    }
}

// MARK: - Color Grid Section

struct ColorGridSection: View {
    @EnvironmentObject var vm: ViewModel

    private let statusPresets: [(name: String, label: String, color: Color)] = [
        ("available", "Available", Color(red: 0, green: 1, blue: 0)),
        ("busy", "Busy", Color(red: 1, green: 0, blue: 0)),
        ("away", "Away", Color(red: 1, green: 1, blue: 0)),
        ("in-meeting", "In Meeting", Color(red: 1, green: 0.27, blue: 0)),
    ]

    private let colorPresets: [(name: String, label: String, color: Color)] = [
        ("red", "Red", Color(red: 1, green: 0, blue: 0)),
        ("orange", "Orange", Color(red: 1, green: 0.65, blue: 0)),
        ("yellow", "Yellow", Color(red: 1, green: 1, blue: 0)),
        ("green", "Green", Color(red: 0, green: 1, blue: 0)),
        ("cyan", "Cyan", Color(red: 0, green: 1, blue: 1)),
        ("blue", "Blue", Color(red: 0, green: 0, blue: 1)),
        ("purple", "Purple", Color(red: 0.5, green: 0, blue: 0.5)),
        ("magenta", "Magenta", Color(red: 1, green: 0, blue: 1)),
        ("white", "White", Color(red: 1, green: 1, blue: 1)),
    ]

    private let gridColumns = Array(repeating: GridItem(.flexible(), spacing: 6), count: 4)

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("STATUS")
                .font(.caption2.bold())
                .foregroundColor(.secondary)

            LazyVGrid(columns: gridColumns, spacing: 6) {
                ForEach(statusPresets, id: \.name) { preset in
                    ColorButton(
                        label: preset.label,
                        color: preset.color,
                        isSelected: vm.currentPreset == preset.name
                    ) {
                        vm.setPreset(preset.name)
                    }
                }
            }

            Text("COLORS")
                .font(.caption2.bold())
                .foregroundColor(.secondary)
                .padding(.top, 4)

            LazyVGrid(columns: gridColumns, spacing: 6) {
                ForEach(colorPresets, id: \.name) { preset in
                    ColorButton(
                        label: preset.label,
                        color: preset.color,
                        isSelected: vm.currentPreset == preset.name
                    ) {
                        vm.setPreset(preset.name)
                    }
                }
            }
        }
    }
}

// MARK: - Color Button

struct ColorButton: View {
    let label: String
    let color: Color
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 2) {
                RoundedRectangle(cornerRadius: 6)
                    .fill(color)
                    .frame(height: 30)
                    .overlay(
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(isSelected ? Color.accentColor : Color.clear, lineWidth: 2)
                    )

                Text(label)
                    .font(.system(size: 9))
                    .foregroundColor(.primary)
                    .lineLimit(1)
            }
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Custom Presets Section

struct CustomPresetsSection: View {
    @EnvironmentObject var vm: ViewModel

    private let gridColumns = Array(repeating: GridItem(.flexible(), spacing: 6), count: 4)

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("CUSTOM PRESETS")
                .font(.caption2.bold())
                .foregroundColor(.secondary)

            LazyVGrid(columns: gridColumns, spacing: 6) {
                ForEach(vm.customPresets) { preset in
                    ColorButton(
                        label: preset.name.capitalized,
                        color: colorFromHex(preset.color),
                        isSelected: vm.currentPreset == preset.name
                    ) {
                        vm.setPreset(preset.name)
                    }
                }
            }
        }
    }
}

// MARK: - Intensity Section

struct IntensitySection: View {
    @EnvironmentObject var vm: ViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("INTENSITY")
                .font(.caption2.bold())
                .foregroundColor(.secondary)

            HStack {
                Image(systemName: "sun.min")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Slider(value: $vm.intensity, in: 0.05...1.0, step: 0.05)
                    .onChange(of: vm.intensity) { _ in
                        if vm.currentPreset != nil {
                            if let name = vm.currentPreset {
                                vm.setPreset(name)
                            }
                        } else if vm.isCustomColorActive {
                            vm.setPickerColor()
                        }
                    }
                Image(systemName: "sun.max")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Text(String(format: "%d%%", Int(vm.intensity * 100)))
                    .font(.system(.caption, design: .monospaced))
                    .foregroundColor(.secondary)
                    .frame(width: 40, alignment: .trailing)
            }
        }
    }
}

// MARK: - Color Picker Section

struct ColorPickerSection: View {
    @EnvironmentObject var vm: ViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("COLOR PICKER")
                .font(.caption2.bold())
                .foregroundColor(.secondary)

            HStack(spacing: 12) {
                ColorPicker("", selection: $vm.pickerColor, supportsOpacity: false)
                    .labelsHidden()

                Text(vm.pickerColor.toHex())
                    .font(.system(.caption, design: .monospaced))
                    .foregroundColor(.secondary)

                Spacer()

                Button("Set") {
                    vm.setPickerColor()
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
            }
        }
    }
}

// MARK: - Animation Section

struct AnimationSection: View {
    @EnvironmentObject var vm: ViewModel

    private let animationTypes = ["breathing", "flash", "sos", "pulse", "rainbow", "transition"]

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("ANIMATION")
                .font(.caption2.bold())
                .foregroundColor(.secondary)

            HStack(spacing: 8) {
                Picker("Type", selection: $vm.selectedAnimationType) {
                    ForEach(animationTypes, id: \.self) { type in
                        Text(type.capitalized).tag(type)
                    }
                }
                .labelsHidden()
                .frame(maxWidth: 120)

                Spacer()

                if vm.isAnimating {
                    Button(action: { vm.stopAnimation() }) {
                        HStack(spacing: 4) {
                            Image(systemName: "stop.fill")
                            Text("Stop")
                        }
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .tint(.red)
                } else {
                    Button(action: {
                        let colorHex = vm.pickerColor.toHex()
                        vm.startAnimation(
                            type: vm.selectedAnimationType,
                            color: colorHex,
                            speed: vm.animationSpeed
                        )
                    }) {
                        HStack(spacing: 4) {
                            Image(systemName: "play.fill")
                            Text("Play")
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                }
            }

            HStack {
                Text("Speed")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Slider(value: $vm.animationSpeed, in: 0.25...4.0, step: 0.25)
                Text(String(format: "%.1fx", vm.animationSpeed))
                    .font(.system(.caption, design: .monospaced))
                    .foregroundColor(.secondary)
                    .frame(width: 35, alignment: .trailing)
            }
        }
    }
}

// MARK: - Slack Section

struct SlackSection: View {
    @EnvironmentObject var vm: ViewModel
    @State private var showSetupWizard = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("SLACK")
                .font(.caption2.bold())
                .foregroundColor(.secondary)

            HStack {
                Circle()
                    .fill(vm.slackConnected ? Color.green : Color.gray)
                    .frame(width: 6, height: 6)
                Text(vm.slackConnected ? "Connected (Socket Mode)" : "Not connected")
                    .font(.caption)
                    .foregroundColor(.secondary)

                Spacer()

                if vm.slackConnected {
                    Button("Disconnect") {
                        vm.disconnectSlack()
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.mini)
                } else {
                    Button("Connect Slack") {
                        showSetupWizard = true
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.mini)
                }
            }

            if vm.slackConnected {
                HStack(spacing: 4) {
                    Image(systemName: "bolt.fill")
                        .font(.system(size: 8))
                        .foregroundColor(.green)
                    Text("Real-time events active")
                        .font(.caption2)
                        .foregroundColor(.secondary)
                }

                Toggle("Auto-sync status to Slack", isOn: $vm.autoSyncSlack)
                    .font(.caption)
                    .toggleStyle(.checkbox)
                    .help("When enabled, setting a status preset also updates your Slack status")
            }
        }
        .sheet(isPresented: $showSetupWizard) {
            SlackSetupWizard(vm: vm)
        }
    }
}

// MARK: - Slack Setup Wizard

struct SlackSetupWizard: View {
    @ObservedObject var vm: ViewModel
    @Environment(\.dismiss) private var dismiss

    @State private var step = 1
    @State private var appToken = ""
    @State private var botToken = ""
    @State private var userToken = ""
    @State private var isConnecting = false
    @State private var errorMessage: String?
    @State private var isConnected = false

    private let totalSteps = 4

    var body: some View {
        VStack(spacing: 0) {
            // Progress bar
            VStack(spacing: 4) {
                HStack {
                    Text("Step \(step) of \(totalSteps)")
                        .font(.caption.bold())
                        .foregroundColor(.secondary)
                    Spacer()
                }
                ProgressView(value: Double(step), total: Double(totalSteps))
                    .tint(.accentColor)
            }
            .padding(.horizontal, 24)
            .padding(.top, 20)
            .padding(.bottom, 12)

            Divider()

            // Step content
            Group {
                switch step {
                case 1: stepCreateApp
                case 2: stepAppToken
                case 3: stepInstallTokens
                case 4: stepVerify
                default: EmptyView()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(.horizontal, 24)
            .padding(.vertical, 16)

            Divider()

            // Navigation buttons
            HStack {
                if step > 1 && !isConnected {
                    Button("Back") {
                        errorMessage = nil
                        step -= 1
                    }
                    .controlSize(.regular)
                }

                Spacer()

                if isConnected {
                    Button("Done") {
                        dismiss()
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.regular)
                } else if step == totalSteps {
                    Button(action: connect) {
                        if isConnecting {
                            ProgressView()
                                .controlSize(.small)
                                .padding(.horizontal, 12)
                        } else {
                            Text("Connect")
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.regular)
                    .disabled(isConnecting)
                } else {
                    Button("Next") {
                        step += 1
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.regular)
                    .disabled(!canAdvance)
                }
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 12)
        }
        .frame(width: 480, height: 400)
    }

    // MARK: - Step Views

    private var stepCreateApp: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("Create Your Slack App", systemImage: "plus.app")
                .font(.headline)

            Text("Click the button below to copy the app manifest and open Slack.")
                .font(.callout)
                .foregroundColor(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            VStack(alignment: .leading, spacing: 4) {
                Text("1. Click **From a manifest**")
                Text("2. Pick your workspace")
                Text("3. Switch to the **JSON** tab, paste (**Cmd+V**)")
                Text("4. Click **Next**, then **Create**")
            }
            .font(.callout)
            .foregroundColor(.secondary)

            Spacer()

            HStack {
                Spacer()
                Button(action: {
                    Task {
                        let _ = await vm.cli.openSlackAppCreation()
                    }
                }) {
                    Label("Create Slack App", systemImage: "safari")
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                Spacer()
            }

            Spacer()
        }
    }

    private var stepAppToken: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("Generate App Token", systemImage: "key")
                .font(.headline)

            Text("In your app settings:")
                .font(.callout)
                .foregroundColor(.secondary)

            VStack(alignment: .leading, spacing: 4) {
                Text("1. Go to **Basic Information** > **App-Level Tokens**")
                Text("2. Click **Generate Token and Scopes**")
                Text("3. Name it anything, add scope: **connections:write**")
                Text("4. Click **Generate** and copy the token")
            }
            .font(.callout)
            .foregroundColor(.secondary)

            Spacer()

            VStack(alignment: .leading, spacing: 4) {
                Text("App-Level Token")
                    .font(.caption.bold())
                    .foregroundColor(.secondary)
                TextField("xapp-...", text: $appToken)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.body, design: .monospaced))
                if !appToken.isEmpty && !appToken.hasPrefix("xapp-") {
                    Text("Token must start with xapp-")
                        .font(.caption)
                        .foregroundColor(.red)
                }
            }

            Spacer()
        }
    }

    private var stepInstallTokens: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("Install & Copy Tokens", systemImage: "arrow.down.app")
                .font(.headline)

            VStack(alignment: .leading, spacing: 4) {
                Text("1. Go to **Install App** in your app settings")
                Text("2. Click **Install to Workspace** and authorize")
                Text("3. Copy both tokens below")
            }
            .font(.callout)
            .foregroundColor(.secondary)

            Spacer()

            VStack(alignment: .leading, spacing: 4) {
                Text("User OAuth Token")
                    .font(.caption.bold())
                    .foregroundColor(.secondary)
                TextField("xoxp-...", text: $userToken)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.body, design: .monospaced))
                if !userToken.isEmpty && !userToken.hasPrefix("xoxp-") {
                    Text("Token must start with xoxp-")
                        .font(.caption)
                        .foregroundColor(.red)
                }
            }

            VStack(alignment: .leading, spacing: 4) {
                Text("Bot User OAuth Token")
                    .font(.caption.bold())
                    .foregroundColor(.secondary)
                TextField("xoxb-...", text: $botToken)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.body, design: .monospaced))
                if !botToken.isEmpty && !botToken.hasPrefix("xoxb-") {
                    Text("Token must start with xoxb-")
                        .font(.caption)
                        .foregroundColor(.red)
                }
            }

            Spacer()
        }
    }

    private var stepVerify: some View {
        VStack(alignment: .leading, spacing: 12) {
            if isConnected {
                Spacer()
                HStack {
                    Spacer()
                    VStack(spacing: 12) {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 48))
                            .foregroundColor(.green)
                        Text("Slack Connected!")
                            .font(.title3.bold())
                        Text("Restart the daemon to enable Socket Mode events.")
                            .font(.callout)
                            .foregroundColor(.secondary)
                            .multilineTextAlignment(.center)
                    }
                    Spacer()
                }
                Spacer()
            } else {
                Label("Verify & Connect", systemImage: "checkmark.shield")
                    .font(.headline)

                Text("Ready to validate your tokens and connect to Slack.")
                    .font(.callout)
                    .foregroundColor(.secondary)

                VStack(alignment: .leading, spacing: 6) {
                    tokenSummaryRow("App Token", token: appToken, prefix: "xapp-")
                    tokenSummaryRow("User OAuth", token: userToken, prefix: "xoxp-")
                    tokenSummaryRow("Bot OAuth", token: botToken, prefix: "xoxb-")
                }
                .padding(12)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.gray.opacity(0.1)))

                if let error = errorMessage {
                    HStack(spacing: 6) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundColor(.red)
                        Text(error)
                            .font(.callout)
                            .foregroundColor(.red)
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.red.opacity(0.1)))
                }

                Spacer()
            }
        }
    }

    private func tokenSummaryRow(_ label: String, token: String, prefix: String) -> some View {
        HStack {
            Text(label)
                .font(.caption.bold())
                .frame(width: 80, alignment: .leading)
            if token.hasPrefix(prefix) {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundColor(.green)
                    .font(.caption)
                Text(String(token.prefix(12)) + "...")
                    .font(.system(.caption, design: .monospaced))
                    .foregroundColor(.secondary)
            } else {
                Image(systemName: "xmark.circle.fill")
                    .foregroundColor(.red)
                    .font(.caption)
                Text("Invalid prefix")
                    .font(.caption)
                    .foregroundColor(.red)
            }
        }
    }

    // MARK: - Logic

    private var canAdvance: Bool {
        switch step {
        case 1:
            return true
        case 2:
            return appToken.hasPrefix("xapp-")
        case 3:
            return botToken.hasPrefix("xoxb-") && userToken.hasPrefix("xoxp-")
        default:
            return true
        }
    }

    private func connect() {
        isConnecting = true
        errorMessage = nil
        Task {
            let (output, ok) = await vm.cli.configureSlack(
                appToken: appToken,
                botToken: botToken,
                userToken: userToken
            )
            await MainActor.run {
                isConnecting = false
                if ok {
                    isConnected = true
                    vm.slackConnected = true
                } else {
                    let msg = output.trimmingCharacters(in: .whitespacesAndNewlines)
                    errorMessage = msg.isEmpty ? "Connection failed" : msg
                }
            }
        }
    }
}

// MARK: - Settings Section

struct SettingsSection: View {
    @EnvironmentObject var vm: ViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("SETTINGS")
                .font(.caption2.bold())
                .foregroundColor(.secondary)

            Toggle("Show in Dock", isOn: $vm.showInDock)
                .font(.caption)
                .toggleStyle(.checkbox)
                .help("Show OpenSlicky in the Dock in addition to the menu bar")
        }
    }
}

// MARK: - Footer Section

struct FooterSection: View {
    @EnvironmentObject var vm: ViewModel
    @State private var showUninstallConfirm = false

    var body: some View {
        HStack {
            Text("OpenSlicky v\(vm.cli.appVersion)")
                .font(.caption2)
                .foregroundColor(.secondary)

            Spacer()

            Button("Uninstall") {
                showUninstallConfirm = true
            }
            .font(.caption2)
            .buttonStyle(.borderless)
            .foregroundColor(.red)
            .alert("Uninstall OpenSlicky?", isPresented: $showUninstallConfirm) {
                Button("Cancel", role: .cancel) {}
                Button("Uninstall", role: .destructive) {
                    vm.uninstall()
                }
            } message: {
                Text("This will remove CLI symlinks and disable startup. Your configuration will be preserved.")
            }
        }
    }
}

// MARK: - Helpers

func colorForPreset(_ name: String) -> Color {
    switch name.lowercased() {
    case "red", "busy": return Color(red: 1, green: 0, blue: 0)
    case "green", "available": return Color(red: 0, green: 1, blue: 0)
    case "blue": return Color(red: 0, green: 0, blue: 1)
    case "yellow", "away": return Color(red: 1, green: 1, blue: 0)
    case "cyan": return Color(red: 0, green: 1, blue: 1)
    case "magenta": return Color(red: 1, green: 0, blue: 1)
    case "white": return Color(red: 1, green: 1, blue: 1)
    case "orange": return Color(red: 1, green: 0.65, blue: 0)
    case "purple": return Color(red: 0.5, green: 0, blue: 0.5)
    case "in-meeting": return Color(red: 1, green: 0.27, blue: 0)
    default: return Color.gray
    }
}

func displayName(for preset: String) -> String {
    switch preset.lowercased() {
    case "in-meeting": return "In Meeting"
    default: return preset.capitalized
    }
}

/// Parse a hex color string (e.g. "#FF0000") into a SwiftUI Color.
func colorFromHex(_ hex: String) -> Color {
    let cleaned = hex.trimmingCharacters(in: .whitespacesAndNewlines)
        .replacingOccurrences(of: "#", with: "")
    guard cleaned.count == 6, let val = UInt64(cleaned, radix: 16) else {
        return Color.gray
    }
    let r = Double((val >> 16) & 0xFF) / 255.0
    let g = Double((val >> 8) & 0xFF) / 255.0
    let b = Double(val & 0xFF) / 255.0
    return Color(red: r, green: g, blue: b)
}

// MARK: - Color Extension

extension Color {
    /// Convert a SwiftUI Color to a hex string like "#RRGGBB".
    func toHex() -> String {
        guard let components = NSColor(self).usingColorSpace(.deviceRGB) else {
            return "#FFFFFF"
        }
        let r = Int(round(components.redComponent * 255))
        let g = Int(round(components.greenComponent * 255))
        let b = Int(round(components.blueComponent * 255))
        return String(format: "#%02X%02X%02X", r, g, b)
    }

    /// Convert to hex with brightness scaled by intensity (0.0–1.0).
    func scaledHex(intensity: Double) -> String {
        guard let components = NSColor(self).usingColorSpace(.deviceRGB) else {
            return "#000000"
        }
        let factor = max(0, min(1, intensity))
        let r = Int(round(components.redComponent * 255 * factor))
        let g = Int(round(components.greenComponent * 255 * factor))
        let b = Int(round(components.blueComponent * 255 * factor))
        return String(format: "#%02X%02X%02X", r, g, b)
    }
}
