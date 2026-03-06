import Foundation

/// Async wrapper around the bundled `statuslight` CLI binary.
///
/// All communication with the StatusLight device and services goes through
/// the CLI binary located next to this executable in Contents/MacOS/.
final class StatusLightCLI {
    /// Path to the `statuslight` binary bundled inside the app.
    private let binaryPath: String

    init() {
        let execURL = Bundle.main.executableURL!
        let macosDir = execURL.deletingLastPathComponent()
        self.binaryPath = macosDir.appendingPathComponent("statuslight-cli").path
    }

    // MARK: - Light Control

    /// Set light to a named preset (e.g. "red", "available", "in-meeting").
    func set(preset: String, deviceSerial: String? = nil) async -> Bool {
        var args = deviceArgs(deviceSerial) + ["set", preset]
        let (_, ok) = await run(args)
        return ok
    }

    /// Set light to a hex color.
    func setHex(_ hex: String, deviceSerial: String? = nil) async -> Bool {
        var args = deviceArgs(deviceSerial) + ["hex", hex]
        let (_, ok) = await run(args)
        return ok
    }

    /// Turn the light off.
    func off(deviceSerial: String? = nil) async -> Bool {
        var args = deviceArgs(deviceSerial) + ["off"]
        let (_, ok) = await run(args)
        return ok
    }

    /// Build the `--device <serial>` prefix args.
    private func deviceArgs(_ serial: String?) -> [String] {
        if let s = serial { return ["--device", s] }
        return []
    }

    // MARK: - Animation

    /// Spawn a blocking animation process. Returns the Process handle.
    func animate(type animType: String, color: String? = nil, color2: String? = nil,
                 speed: Double = 1.0, brightness: Double = 1.0) -> Process {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)

        // --brightness is a global flag (0-100) before the subcommand
        var args: [String] = []
        let brightnessPercent = Int((brightness * 100).rounded())
        if brightnessPercent < 100 {
            args += ["--brightness", "\(brightnessPercent)"]
        }
        args += ["animate", animType]
        if let c = color {
            args += ["--color", c]
        }
        if let c2 = color2 {
            args += ["--color2", c2]
        }
        args += ["--speed", String(format: "%.2f", speed)]

        process.arguments = args
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try? process.run()
        return process
    }

    /// Stop a running animation process.
    func stopAnimation(_ process: Process?) {
        guard let p = process, p.isRunning else { return }
        p.terminate()
        DispatchQueue.global(qos: .utility).async {
            p.waitUntilExit()
        }
    }

    // MARK: - Custom Presets

    /// List custom presets as JSON array.
    func listPresetsJSON() async -> String {
        let (output, _) = await run(["preset", "list", "--json"])
        return output.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    // MARK: - Devices

    /// A connected device's info parsed from CLI output.
    struct DeviceInfo: Identifiable, Equatable {
        let id: String  // serial or "device-N" fallback
        let name: String
        let driver: String
        let serial: String?
    }

    /// Query connected devices with full info.
    func getDevices() async -> [DeviceInfo] {
        let (output, ok) = await run(["devices"])
        guard ok else { return [] }
        var devices: [DeviceInfo] = []
        var driver: String?
        var serial: String?
        var displayName: String?
        var index = 0
        for line in output.split(separator: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("Device ") {
                // Flush previous device.
                if let d = driver {
                    let name = displayName ?? d
                    let id = serial ?? "device-\(index)"
                    devices.append(DeviceInfo(id: id, name: name, driver: d, serial: serial))
                    index += 1
                }
                driver = nil; serial = nil; displayName = nil
            } else if trimmed.hasPrefix("Name:") {
                displayName = String(trimmed.dropFirst("Name:".count)).trimmingCharacters(in: .whitespaces)
            } else if trimmed.hasPrefix("Driver:") {
                driver = String(trimmed.dropFirst("Driver:".count)).trimmingCharacters(in: .whitespaces)
            } else if trimmed.hasPrefix("Serial:") {
                serial = String(trimmed.dropFirst("Serial:".count)).trimmingCharacters(in: .whitespaces)
            }
        }
        // Flush last device.
        if let d = driver {
            let name = displayName ?? d
            let id = serial ?? "device-\(index)"
            devices.append(DeviceInfo(id: id, name: name, driver: d, serial: serial))
        }
        return devices
    }

    /// Parsed light status from `statuslight status`.
    struct LightStatus {
        let colorHex: String?   // e.g. "#00FF00"
        let presetName: String? // e.g. "available"
    }

    /// Query current device color/status via `statuslight status`.
    func getStatus() async -> LightStatus {
        let (output, ok) = await run(["status"])
        guard ok else { return LightStatus(colorHex: nil, presetName: nil) }
        var colorHex: String?
        var presetName: String?
        for line in output.split(separator: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("Color:") {
                let value = String(trimmed.dropFirst("Color:".count)).trimmingCharacters(in: .whitespaces)
                // Extract hex from e.g. "#00FF00 (from device)"
                if let hex = value.split(separator: " ").first, hex.hasPrefix("#") {
                    colorHex = String(hex)
                }
            } else if trimmed.hasPrefix("Status:") {
                presetName = String(trimmed.dropFirst("Status:".count)).trimmingCharacters(in: .whitespaces)
            }
        }
        return LightStatus(colorHex: colorHex, presetName: presetName)
    }

    // MARK: - Slack

    /// Check Slack connection status. Returns true if connected.
    func isSlackConnected() async -> Bool {
        let (output, _) = await run(["slack", "status"])
        let lower = output.lowercased()
        return lower.contains("connected") && !lower.contains("not connected")
    }

    /// Open the browser with the pre-filled Slack app manifest.
    func openSlackAppCreation() async -> Bool {
        let (_, ok) = await run(["slack", "open-setup"])
        return ok
    }

    /// Non-interactive Slack token configuration via stdin. Returns (output, success).
    /// Tokens are piped via stdin to avoid exposing them in process arguments.
    func configureSlack(appToken: String, botToken: String, userToken: String) async -> (String, Bool) {
        let input = "\(appToken)\n\(botToken)\n\(userToken)\n"
        return await runWithStdin(["slack", "configure", "--stdin"], input: input)
    }

    /// Disconnect Slack (clears all tokens).
    func slackDisconnect() async -> Bool {
        let (_, ok) = await run(["slack", "disconnect"])
        return ok
    }

    /// Set Slack status text and emoji.
    func slackSetStatus(text: String, emoji: String) async -> Bool {
        let (_, ok) = await run(["slack", "set-status", "--text", text, "--emoji", emoji])
        return ok
    }

    /// Clear Slack status.
    func slackClearStatus() async -> Bool {
        let (_, ok) = await run(["slack", "clear-status"])
        return ok
    }

    /// Set Slack presence to "auto" (online) or "away".
    func slackSetPresence(_ presence: String) async -> Bool {
        let (_, ok) = await run(["slack", "set-presence", presence])
        return ok
    }

    // MARK: - Startup

    /// Enable launch-on-login.
    func startupEnable() async -> Bool {
        let (_, ok) = await run(["startup", "enable"])
        return ok
    }

    /// Disable launch-on-login.
    func startupDisable() async -> Bool {
        let (_, ok) = await run(["startup", "disable"])
        return ok
    }

    // MARK: - Update

    /// Query cached update status (local-only, no network). Returns (JSON output, success).
    func updateStatus() async -> (String, Bool) {
        return await run(["update", "status"])
    }

    /// Download and install the latest update. Returns (JSON output, success).
    func installUpdate() async -> (String, Bool) {
        return await run(["update", "install"])
    }

    /// Install update with admin privileges (fallback when normal install fails due to permissions).
    func installUpdateAdmin() -> Bool {
        let macosDir = shellEscape(URL(fileURLWithPath: binaryPath).deletingLastPathComponent().path)
        let script = "'\(macosDir)/statuslight-cli' update install"
        return runOsascriptAdmin(script)
    }

    // MARK: - Install / Uninstall (admin)

    /// Create symlinks in /usr/local/bin (requires admin).
    func installSymlinks() -> Bool {
        let macosDir = shellEscape(URL(fileURLWithPath: binaryPath).deletingLastPathComponent().path)
        let script = "mkdir -p /usr/local/bin && ln -sf '\(macosDir)/statuslight-cli' /usr/local/bin/statuslight && ln -sf '\(macosDir)/statuslightd' /usr/local/bin/statuslightd"
        return runOsascriptAdmin(script)
    }

    /// Remove symlinks and app bundle (requires admin).
    /// Single admin prompt handles symlinks + app removal.
    func removeSymlinksAndApp() -> Bool {
        let appPath = shellEscape(Bundle.main.bundlePath)
        let script = "rm -f /usr/local/bin/statuslight /usr/local/bin/statuslightd && rm -rf '\(appPath)'"
        return runOsascriptAdmin(script)
    }

    /// Restart the daemon by killing it and letting the LaunchAgent respawn it.
    /// If there's no LaunchAgent, launches the daemon directly from the bundle.
    func restartDaemon() {
        let killall = Process()
        killall.executableURL = URL(fileURLWithPath: "/usr/bin/killall")
        killall.arguments = ["statuslightd"]
        killall.standardOutput = FileHandle.nullDevice
        killall.standardError = FileHandle.nullDevice
        try? killall.run()
        killall.waitUntilExit()

        // Wait for it to exit, then check if LaunchAgent will respawn it.
        Thread.sleep(forTimeInterval: 1.0)

        let check = Process()
        check.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
        check.arguments = ["-x", "statuslightd"]
        check.standardOutput = FileHandle.nullDevice
        check.standardError = FileHandle.nullDevice
        try? check.run()
        check.waitUntilExit()
        if check.terminationStatus != 0 {
            // LaunchAgent didn't respawn it — start it manually.
            let macosDir = Bundle.main.executableURL!.deletingLastPathComponent()
            let daemon = Process()
            daemon.executableURL = macosDir.appendingPathComponent("statuslightd")
            daemon.standardOutput = FileHandle.nullDevice
            daemon.standardError = FileHandle.nullDevice
            try? daemon.run()
        }
    }

    /// Unload LaunchAgent and kill slickyd daemon. Waits for confirmed exit.
    func stopDaemon() {
        // Unload LaunchAgent
        let plistPath = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents/com.statuslight.daemon.plist").path
        if FileManager.default.fileExists(atPath: plistPath) {
            let unload = Process()
            unload.executableURL = URL(fileURLWithPath: "/bin/launchctl")
            unload.arguments = ["unload", "-w", plistPath]
            try? unload.run()
            unload.waitUntilExit()
            try? FileManager.default.removeItem(atPath: plistPath)
        }

        // Kill statuslightd only (not statuslight — that would kill CLI commands we need)
        let killall = Process()
        killall.executableURL = URL(fileURLWithPath: "/usr/bin/killall")
        killall.arguments = ["statuslightd"]
        try? killall.run()
        killall.waitUntilExit()

        // Wait for slickyd to fully exit (up to 5 seconds)
        for _ in 0..<10 {
            let check = Process()
            check.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
            check.arguments = ["-x", "statuslightd"]
            check.standardOutput = FileHandle.nullDevice
            check.standardError = FileHandle.nullDevice
            try? check.run()
            check.waitUntilExit()
            if check.terminationStatus != 0 { break } // no process found = done
            Thread.sleep(forTimeInterval: 0.5)
        }
    }

    // MARK: - Version

    /// Read app version from Info.plist.
    var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown"
    }

    // MARK: - Translocation Detection

    /// Returns true if the app is running from a translocated (DMG / temp) path.
    var isTranslocated: Bool {
        let path = Bundle.main.bundlePath
        return path.hasPrefix("/private/var/folders/")
            || path.hasPrefix("/var/folders/")
            || path.hasPrefix("/Volumes/")
    }

    // MARK: - Install Marker

    private var markerPath: String {
        let configDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".config/statuslight")
        return configDir.appendingPathComponent(".installed-\(appVersion)").path
    }

    /// Returns true if this version has been installed.
    var isInstalled: Bool {
        FileManager.default.fileExists(atPath: markerPath)
    }

    /// Write the install marker file.
    func writeMarker() {
        let dir = URL(fileURLWithPath: markerPath).deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        FileManager.default.createFile(atPath: markerPath, contents: nil)
    }

    /// Remove all install markers.
    func removeMarkers() {
        let configDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".config/statuslight")
        if let contents = try? FileManager.default.contentsOfDirectory(atPath: configDir.path) {
            for file in contents where file.hasPrefix(".installed-") {
                try? FileManager.default.removeItem(atPath: configDir.appendingPathComponent(file).path)
            }
        }
    }

    // MARK: - Private Helpers

    /// Run the CLI binary with the given arguments, piping input to stdin.
    ///
    /// Uses concurrent pipe reading and a 15-second watchdog timer.
    private func runWithStdin(_ arguments: [String], input: String) async -> (String, Bool) {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async { [binaryPath] in
                let process = Process()
                process.executableURL = URL(fileURLWithPath: binaryPath)
                process.arguments = arguments

                let outPipe = Pipe()
                process.standardOutput = outPipe
                process.standardError = outPipe

                let inPipe = Pipe()
                process.standardInput = inPipe

                // Collect output asynchronously to avoid pipe buffer deadlock.
                var outputData = Data()
                let outputLock = NSLock()
                outPipe.fileHandleForReading.readabilityHandler = { handle in
                    let chunk = handle.availableData
                    if !chunk.isEmpty {
                        outputLock.lock()
                        outputData.append(chunk)
                        outputLock.unlock()
                    }
                }

                do {
                    try process.run()
                    if let data = input.data(using: .utf8) {
                        inPipe.fileHandleForWriting.write(data)
                    }
                    inPipe.fileHandleForWriting.closeFile()
                } catch {
                    outPipe.fileHandleForReading.readabilityHandler = nil
                    continuation.resume(returning: ("", false))
                    return
                }

                // Watchdog: kill process after 15 seconds.
                let processRef = process
                let watchdog = DispatchWorkItem {
                    if processRef.isRunning {
                        processRef.terminate()
                    }
                }
                DispatchQueue.global(qos: .utility).asyncAfter(
                    deadline: .now() + 15.0,
                    execute: watchdog
                )

                process.waitUntilExit()
                watchdog.cancel()

                outPipe.fileHandleForReading.readabilityHandler = nil
                let finalChunk = outPipe.fileHandleForReading.readDataToEndOfFile()

                outputLock.lock()
                outputData.append(finalChunk)
                let output = String(data: outputData, encoding: .utf8) ?? ""
                outputLock.unlock()

                let ok = process.terminationStatus == 0
                continuation.resume(returning: (output, ok))
            }
        }
    }

    /// Run the CLI binary with the given arguments and return (stdout, success).
    ///
    /// Uses concurrent pipe reading (avoids pipe-buffer deadlock) and a 15-second
    /// watchdog timer that terminates the process if it hangs.
    private func run(_ arguments: [String]) async -> (String, Bool) {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async { [binaryPath] in
                let process = Process()
                process.executableURL = URL(fileURLWithPath: binaryPath)
                process.arguments = arguments

                let pipe = Pipe()
                process.standardOutput = pipe
                process.standardError = pipe

                // Collect output asynchronously to avoid pipe buffer deadlock.
                var outputData = Data()
                let outputLock = NSLock()
                pipe.fileHandleForReading.readabilityHandler = { handle in
                    let chunk = handle.availableData
                    if !chunk.isEmpty {
                        outputLock.lock()
                        outputData.append(chunk)
                        outputLock.unlock()
                    }
                }

                do {
                    try process.run()
                } catch {
                    pipe.fileHandleForReading.readabilityHandler = nil
                    continuation.resume(returning: ("", false))
                    return
                }

                // Watchdog: kill process after 15 seconds.
                let processRef = process
                let watchdog = DispatchWorkItem {
                    if processRef.isRunning {
                        processRef.terminate()
                    }
                }
                DispatchQueue.global(qos: .utility).asyncAfter(
                    deadline: .now() + 15.0,
                    execute: watchdog
                )

                process.waitUntilExit()
                watchdog.cancel()

                // Stop reading and collect final data.
                pipe.fileHandleForReading.readabilityHandler = nil
                let finalChunk = pipe.fileHandleForReading.readDataToEndOfFile()

                outputLock.lock()
                outputData.append(finalChunk)
                let output = String(data: outputData, encoding: .utf8) ?? ""
                outputLock.unlock()

                let ok = process.terminationStatus == 0
                continuation.resume(returning: (output, ok))
            }
        }
    }

    /// Escape a path for use inside single-quoted shell strings.
    private func shellEscape(_ path: String) -> String {
        path.replacingOccurrences(of: "'", with: "'\\''")
    }

    /// Run a shell command with administrator privileges via osascript.
    private func runOsascriptAdmin(_ script: String) -> Bool {
        let escaped = script.replacingOccurrences(of: "\"", with: "\\\"")
        let apple = "do shell script \"\(escaped)\" with administrator privileges"
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        process.arguments = ["-e", apple]
        do {
            try process.run()
            process.waitUntilExit()
            return process.terminationStatus == 0
        } catch {
            return false
        }
    }
}
