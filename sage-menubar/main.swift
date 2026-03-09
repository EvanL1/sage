import Cocoa
import UserNotifications

// MARK: - Sage Menu Bar Companion
// macOS native menu bar app for sage-daemon.
// Must be inside a .app bundle for UNUserNotificationCenter to work.
// Build: swiftc -O -o sage-menubar main.swift

class SageMenuBar: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    private var statusItem: NSStatusItem!
    private var statusMenuItem: NSMenuItem!
    private var statusTimer: Timer?
    private var notifyTimer: Timer?
    private var lastDaemonRunning: Bool? = nil

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        // Request notification permission
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound]) { _, _ in }

        buildStatusItem()
        buildMenu()
        startPolling()
    }

    // Show notification banner even when app is in foreground
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler handler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        handler([.banner, .sound])
    }

    // MARK: - UI Setup

    private func buildStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = statusItem.button {
            button.title = " S "
            button.font = NSFont.monospacedSystemFont(ofSize: 12, weight: .medium)
        }
    }

    private func buildMenu() {
        let menu = NSMenu()

        // — Status —
        statusMenuItem = NSMenuItem(title: "Sage — Checking...", action: nil, keyEquivalent: "")
        statusMenuItem.isEnabled = false
        menu.addItem(statusMenuItem)

        menu.addItem(NSMenuItem.separator())

        // — Primary Actions —
        addItem(to: menu, title: "Open Logs", action: #selector(openLogs), key: "l")
        addItem(to: menu, title: "Open Config", action: #selector(openConfig), key: ",")
        addItem(to: menu, title: "Open Memory", action: #selector(openMemory), key: "m")

        menu.addItem(NSMenuItem.separator())

        // — Daemon Control —
        addItem(to: menu, title: "Restart Daemon", action: #selector(restartDaemon), key: "r")
        addItem(to: menu, title: "Stop Daemon", action: #selector(stopDaemon), key: "")

        menu.addItem(NSMenuItem.separator())

        // — Installation Info —
        for path in ["~/.sage/config.toml", "~/.sage/logs/", "~/.sage/memory/"] {
            let item = NSMenuItem(title: path, action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
        }

        menu.addItem(NSMenuItem.separator())

        menu.addItem(NSMenuItem(
            title: "Quit Sage",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        ))

        statusItem.menu = menu
    }

    private func addItem(to menu: NSMenu, title: String, action: Selector, key: String) {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.target = self
        menu.addItem(item)
    }

    // MARK: - Polling

    private func startPolling() {
        updateStatus()
        checkNotifyDir()

        // Status: every 10s
        statusTimer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
            self?.updateStatus()
        }
        // Notifications: every 2s
        notifyTimer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            self?.checkNotifyDir()
        }
    }

    private func updateStatus() {
        let running = getDaemonPID()
        DispatchQueue.main.async {
            if running != nil {
                self.statusMenuItem.title = "Sage — Running"
                self.statusItem.button?.appearsDisabled = false
            } else {
                self.statusMenuItem.title = "Sage — Stopped"
                self.statusItem.button?.appearsDisabled = true
            }
        }

        let isRunning = running != nil
        if let last = lastDaemonRunning, last != isRunning {
            if isRunning {
                postNotification(title: "Sage Daemon", body: "已启动")
            } else {
                postNotification(title: "Sage Daemon", body: "已停止")
            }
        }
        lastDaemonRunning = isRunning
    }

    private func getDaemonPID() -> Int32? {
        let task = Process()
        let pipe = Pipe()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/pgrep")
        task.arguments = ["-f", "sage-daemon.*--foreground"]
        task.standardOutput = pipe
        task.standardError = FileHandle.nullDevice
        do {
            try task.run()
            task.waitUntilExit()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            if let str = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
               let pid = Int32(str.components(separatedBy: "\n").first ?? "") {
                return pid
            }
        } catch {}
        return nil
    }

    // MARK: - Notification File IPC

    private func checkNotifyDir() {
        let dir = expand("~/.sage/notify")
        guard let files = try? FileManager.default.contentsOfDirectory(atPath: dir) else { return }

        for file in files.sorted() where file.hasSuffix(".json") {
            let path = (dir as NSString).appendingPathComponent(file)
            defer { try? FileManager.default.removeItem(atPath: path) }

            guard let data = FileManager.default.contents(atPath: path),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: String]
            else { continue }

            postNotification(
                title: json["title"] ?? "Sage",
                body: json["body"] ?? ""
            )
        }
    }

    private func postNotification(title: String, body: String) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        content.sound = .default

        let request = UNNotificationRequest(
            identifier: UUID().uuidString,
            content: content,
            trigger: nil
        )
        UNUserNotificationCenter.current().add(request)
    }

    // MARK: - Actions

    @objc private func openLogs() { openFile("~/.sage/logs/sage.out.log") }
    @objc private func openConfig() { openFile("~/.sage/config.toml") }
    @objc private func openMemory() {
        NSWorkspace.shared.open(URL(fileURLWithPath: expand("~/.sage/memory")))
    }

    @objc private func restartDaemon() {
        runLaunchctl(["kickstart", "-k", "gui/\(getuid())/com.sage.daemon"])
    }

    @objc private func stopDaemon() {
        runLaunchctl(["unload", expand("~/Library/LaunchAgents/com.sage.daemon.plist")])
    }

    private func openFile(_ p: String) {
        NSWorkspace.shared.open(URL(fileURLWithPath: expand(p)))
    }
    private func expand(_ p: String) -> String {
        NSString(string: p).expandingTildeInPath
    }
    private func runLaunchctl(_ args: [String]) {
        let t = Process()
        t.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        t.arguments = args
        try? t.run()
    }
}

// MARK: - Entry

let app = NSApplication.shared
let delegate = SageMenuBar()
app.delegate = delegate
app.run()
