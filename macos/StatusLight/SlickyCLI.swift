import Foundation

/// Async wrapper around the bundled `slicky` CLI binary.
///
/// All communication with the Slicky device and services goes through
/// the CLI binary located next to this executable in Contents/MacOS/.
final class SlickyCLI {
    /// Path to the `slicky` binary bundled inside the app.
    private let binaryPath: String

    init() {
        let execURL = Bundle.main.executableURL!
        let macosDir = execURL.deletingLastPathComponent()
        self.binaryPath = macosDir.appendingPathComponent("slicky").path
    }

    // MARK: - Light Control

    /// Set light to a named preset (e.g. "red", "available", "in-meeting").
    func set(preset: String) async -> Bool {
        let (_, ok) = await run(["set", preset])
        return ok
    }

    /// Set light to a hex color.
    func setHex(_ hex: String) async -> Bool {
        let (_, ok) = await run(["hex", hex])
        return ok
    }

    /// Turn the light off.
    func off() async -> Bool {
        let (_, ok) = await run(["off"])
        return ok
    }

    // MARK: - Animation

    /// Spawn a blocking animation process. Returns the Process handle.
    func animate(type animType: String, color: String? = nil, color2: String? = nil,
                 speed: Double = 1.0, brightness: Double = 1.0) -> Process {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: binaryPath)

        var args = ["animate", animType]
        if let c = color {
            args += ["--color", c]
        }
        if let c2 = color2 {
            args += ["--color2", c2]
        }
        args += ["--speed", String(format: "%.2f", speed)]
        if brightness < 1.0 {
            args += ["--brightness", String(format: "%.2f", brightness)]
        }

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

    /// Check if any Slicky device is connected.
    func isDeviceConnected() async -> Bool {
        let (output, ok) = await run(["devices"])
        guard ok else { return false }
        // The `devices` command lists connected devices; if there's meaningful
        // output beyond headers, a device is connected.
        let lines = output.split(separator: "\n").filter { !$0.isEmpty }
        return lines.count > 1
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
        let script = "'\(macosDir)/slicky' update install"
        return runOsascriptAdmin(script)
    }

    // MARK: - Install / Uninstall (admin)

    /// Create symlinks in /usr/local/bin (requires admin).
    func installSymlinks() -> Bool {
        let macosDir = shellEscape(URL(fileURLWithPath: binaryPath).deletingLastPathComponent().path)
        let script = "mkdir -p /usr/local/bin && ln -sf '\(macosDir)/slicky' /usr/local/bin/slicky && ln -sf '\(macosDir)/slickyd' /usr/local/bin/slickyd"
        return runOsascriptAdmin(script)
    }

    /// Remove symlinks and app bundle (requires admin).
    /// Single admin prompt handles symlinks + app removal.
    func removeSymlinksAndApp() -> Bool {
        let appPath = shellEscape(Bundle.main.bundlePath)
        let script = "rm -f /usr/local/bin/slicky /usr/local/bin/slickyd && rm -rf '\(appPath)'"
        return runOsascriptAdmin(script)
    }

    /// Unload LaunchAgent and kill slickyd daemon. Waits for confirmed exit.
    func stopDaemon() {
        // Unload LaunchAgent
        let plistPath = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents/com.openslicky.daemon.plist").path
        if FileManager.default.fileExists(atPath: plistPath) {
            let unload = Process()
            unload.executableURL = URL(fileURLWithPath: "/bin/launchctl")
            unload.arguments = ["unload", "-w", plistPath]
            try? unload.run()
            unload.waitUntilExit()
            try? FileManager.default.removeItem(atPath: plistPath)
        }

        // Kill slickyd only (not slicky — that would kill CLI commands we need)
        let killall = Process()
        killall.executableURL = URL(fileURLWithPath: "/usr/bin/killall")
        killall.arguments = ["slickyd"]
        try? killall.run()
        killall.waitUntilExit()

        // Wait for slickyd to fully exit (up to 5 seconds)
        for _ in 0..<10 {
            let check = Process()
            check.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
            check.arguments = ["-x", "slickyd"]
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
            .appendingPathComponent(".config/openslicky")
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
            .appendingPathComponent(".config/openslicky")
        if let contents = try? FileManager.default.contentsOfDirectory(atPath: configDir.path) {
            for file in contents where file.hasPrefix(".installed-") {
                try? FileManager.default.removeItem(atPath: configDir.appendingPathComponent(file).path)
            }
        }
    }

    // MARK: - Private Helpers

    /// Run the CLI binary with the given arguments, piping input to stdin.
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

                do {
                    try process.run()
                    // Write tokens to stdin and close.
                    if let data = input.data(using: .utf8) {
                        inPipe.fileHandleForWriting.write(data)
                    }
                    inPipe.fileHandleForWriting.closeFile()
                    process.waitUntilExit()
                } catch {
                    continuation.resume(returning: ("", false))
                    return
                }

                let data = outPipe.fileHandleForReading.readDataToEndOfFile()
                let output = String(data: data, encoding: .utf8) ?? ""
                let ok = process.terminationStatus == 0
                continuation.resume(returning: (output, ok))
            }
        }
    }

    /// Run the CLI binary with the given arguments and return (stdout, success).
    private func run(_ arguments: [String]) async -> (String, Bool) {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async { [binaryPath] in
                let process = Process()
                process.executableURL = URL(fileURLWithPath: binaryPath)
                process.arguments = arguments

                let pipe = Pipe()
                process.standardOutput = pipe
                process.standardError = pipe

                do {
                    try process.run()
                    process.waitUntilExit()
                } catch {
                    continuation.resume(returning: ("", false))
                    return
                }

                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                let output = String(data: data, encoding: .utf8) ?? ""
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
