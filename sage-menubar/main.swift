import Cocoa
import UserNotifications

// MARK: - Sage Menu Bar Agent
// Menu bar 常驻 + 通知代理：监听 ~/.sage/notify/，发原生通知，点击打开 Sage Desktop。
// 必须在 .app bundle 内运行才能使用 UNUserNotificationCenter。
// Build: swiftc -O -o sage-menubar main.swift

class SageMenuBarAgent: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    private var statusItem: NSStatusItem!
    private var statusMenuItem: NSMenuItem!
    private var notifyTimer: Timer?
    private var statusTimer: Timer?
    private var lastDaemonRunning: Bool? = nil

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound]) { _, _ in }

        buildStatusItem()
        buildMenu()
        startPolling()
    }

    // 前台时也显示通知横幅
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler handler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        handler([.banner, .sound])
    }

    // 点击通知 → 打开 Sage Desktop
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler handler: @escaping () -> Void
    ) {
        openSageDesktop()
        handler()
    }

    // MARK: - UI

    private func buildStatusItem() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = statusItem.button {
            // SF Symbol: 脑图标，代表 AI 参谋
            if let img = NSImage(systemSymbolName: "brain", accessibilityDescription: "Sage") {
                img.isTemplate = true
                button.image = img
            } else {
                button.title = "◆"
            }
        }
    }

    private func buildMenu() {
        let menu = NSMenu()

        statusMenuItem = NSMenuItem(title: "Sage — Checking...", action: nil, keyEquivalent: "")
        statusMenuItem.isEnabled = false
        menu.addItem(statusMenuItem)
        menu.addItem(NSMenuItem.separator())

        addItem(to: menu, title: "Open Sage Desktop", action: #selector(onOpenDesktop), key: "o")
        menu.addItem(NSMenuItem.separator())

        addItem(to: menu, title: "Open Logs", action: #selector(onOpenLogs), key: "l")
        addItem(to: menu, title: "Open Config", action: #selector(onOpenConfig), key: ",")
        addItem(to: menu, title: "Open Memory", action: #selector(onOpenMemory), key: "m")
        menu.addItem(NSMenuItem.separator())

        addItem(to: menu, title: "Restart Daemon", action: #selector(onRestartDaemon), key: "r")
        menu.addItem(NSMenuItem.separator())

        menu.addItem(NSMenuItem(
            title: "Quit Sage Menu Bar",
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

        statusTimer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
            self?.updateStatus()
        }
        notifyTimer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            self?.checkNotifyDir()
        }
    }

    private func updateStatus() {
        DispatchQueue.global(qos: .utility).async { [weak self] in
            guard let self else { return }
            let running = self.getDaemonPID() != nil
            DispatchQueue.main.async {
                self.statusMenuItem.title = running ? "Sage Daemon — Running" : "Sage Daemon — Stopped"
                self.statusItem.button?.appearsDisabled = !running
                if let last = self.lastDaemonRunning, last != running {
                    self.postNotification(title: "Sage Daemon", body: running ? "已启动" : "已停止")
                }
                self.lastDaemonRunning = running
            }
        }
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
        DispatchQueue.global(qos: .utility).async { [weak self] in
            guard let self else { return }
            let dir = self.expand("~/.sage/notify")
            guard let files = try? FileManager.default.contentsOfDirectory(atPath: dir) else { return }

            for file in files.sorted() where file.hasSuffix(".json") {
                let path = (dir as NSString).appendingPathComponent(file)
                guard let data = FileManager.default.contents(atPath: path),
                      let json = try? JSONSerialization.jsonObject(with: data) as? [String: String]
                else { continue }

                self.postNotification(
                    title: json["title"] ?? "Sage",
                    body: json["body"] ?? ""
                )
                try? FileManager.default.removeItem(atPath: path)
            }
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

    @objc private func onOpenDesktop() { openSageDesktop() }
    @objc private func onOpenLogs() { openFile("~/.sage/logs/sage.out.log") }
    @objc private func onOpenConfig() { openFile("~/.sage/config.toml") }
    @objc private func onOpenMemory() {
        NSWorkspace.shared.open(URL(fileURLWithPath: expand("~/.sage/memory")))
    }
    @objc private func onRestartDaemon() {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let t = Process()
            t.executableURL = URL(fileURLWithPath: "/bin/launchctl")
            t.arguments = ["kickstart", "-k", "gui/\(getuid())/com.sage.daemon"]
            do {
                try t.run()
                t.waitUntilExit()
                let success = t.terminationStatus == 0
                self?.postNotification(
                    title: "Sage Daemon",
                    body: success ? "重启成功" : "重启失败 (exit \(t.terminationStatus))"
                )
            } catch {
                self?.postNotification(
                    title: "Sage Daemon",
                    body: "重启失败: \(error.localizedDescription)"
                )
            }
        }
    }

    private func openSageDesktop() {
        if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.sage.desktop") {
            NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
        }
    }

    private func openFile(_ p: String) {
        NSWorkspace.shared.open(URL(fileURLWithPath: expand(p)))
    }

    private func expand(_ p: String) -> String {
        NSString(string: p).expandingTildeInPath
    }
}

// MARK: - Entry

let app = NSApplication.shared
let delegate = SageMenuBarAgent()
app.delegate = delegate
app.run()
