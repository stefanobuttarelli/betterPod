// App.swift — betterPod Share for the Mac: send songs and albums to a
// betterPod with an 8-digit code (Music → Receive on the betterPod).

import AppKit
import SwiftUI

@main
struct BetterPodShareApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    @StateObject private var model = Model()
    @StateObject private var updater = Updater()

    var body: some Scene {
        Window("betterPod Share", id: "main") {
            ContentView(model: model, updater: updater)
                // the studio is drawn light, as on the console: the
                // buttons and the name field stay light too (with the Mac
                // in Dark mode they showed white text on the light studio)
                .preferredColorScheme(.light)
                .onAppear {
                    delegate.model = model
                    updater.checkAtStart()
                }
        }
        .windowResizability(.contentSize)
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…") { updater.check(byHand: true) }
            }
            CommandGroup(replacing: .newItem) {
                Button("Choose Songs…") { model.choose() }
                    .keyboardShortcut("o")
            }
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    weak var model: Model?
    private var pending: [URL] = []

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    // files dropped on the app's icon (or opened with it)
    func application(_ application: NSApplication, open urls: [URL]) {
        Task { @MainActor in
            if let model { model.open(urls) } else { pending += urls }
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        Task { @MainActor in
            if let model, !pending.isEmpty {
                model.open(pending)
                pending = []
            }
        }
    }
}
