// Independent local library editing and the existing Share workspace.

import AppKit
import SwiftUI

@main
struct BetterPodShareApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    @StateObject private var model = Model()
    @StateObject private var library = LibraryModel()
    @StateObject private var updater = Updater()
    @StateObject private var device = DeviceModel()
    @State private var section = MacSection.library

    var body: some Scene {
        Window("betterPod Share", id: "main") {
            MacWorkspace(model: model, library: library, updater: updater, device: device, section: $section)
                .onAppear {
                    delegate.model = model
                    delegate.library = library
                    delegate.device = device
                    delegate.openFiles = { urls in
                        if section == .share {
                            model.open(urls)
                        } else {
                            section = .library
                            library.open(urls)
                        }
                    }
                    delegate.consumePending()
                    DispatchQueue.main.async { delegate.attachWindow() }
                    updater.checkAtStart()
                }
        }
        .defaultSize(width: 1200, height: 800)
        .windowResizability(.contentMinSize)
        // The window draws its own glossy header; the title bar only keeps its buttons.
        .windowStyle(.hiddenTitleBar)
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…") { updater.check(byHand: true) }
            }
            CommandGroup(replacing: .newItem) {
                Button("Open Folder or Songs…") {
                    if section == .share { model.choose() } else {
                        section = .library
                        library.choose()
                    }
                }
                    .keyboardShortcut("o")
            }
            CommandGroup(after: .newItem) {
                Button("Save Changes") { activeLibrary?.save() }
                    .keyboardShortcut("s")
                    .disabled(activeLibrary?.canSave != true)
            }
            CommandMenu("Library") {
                Button("Find Album…") { activeLibrary?.beginLookup() }
                    .keyboardShortcut("f")
                    .disabled(activeLibrary?.canLookup != true)
                Button("Choose Cover Image…") { activeLibrary?.chooseArtwork() }
                    .disabled(!canEditSelection)
                Button("Merge into Album…") { activeLibrary?.beginMerge() }
                    .disabled(activeLibrary?.canMergeSelection != true)
                Divider()
                Button("Remove From Library") { library.removeSelection(undoManager: NSApp.keyWindow?.undoManager) }
                    .keyboardShortcut(.delete, modifiers: .command)
                    .disabled(section != .library || library.busy || library.selection.isEmpty)
                Button("Clear Library") { library.clearLibrary(undoManager: NSApp.keyWindow?.undoManager) }
                    .disabled(section != .library || library.busy || library.tracks.isEmpty)
                Button("Undo Library Removal") { library.undoRemoval() }
                    .disabled(section != .library || !library.canUndoRemoval)
            }
            CommandMenu("Navigate") {
                Button("Library") { section = .library }.keyboardShortcut("1")
                Button("Share") { section = .share }.keyboardShortcut("2")
                Button("betterPod") { section = .device }.keyboardShortcut("3")
            }
        }
    }

    private var activeLibrary: LibraryModel? {
        switch section {
        case .library: return library
        case .device: return device.connected ? device.library : nil
        case .share: return nil
        }
    }

    private var canEditSelection: Bool {
        guard let editor = activeLibrary else { return false }
        return editor.editableCount > 0 && !editor.busy && !editor.remoteNeedsReload
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    weak var model: Model?
    weak var library: LibraryModel?
    weak var device: DeviceModel?
    var openFiles: (([URL]) -> Void)?
    private var pending: [URL] = []
    private var closeApproved = false

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if !closeApproved && !canClose() { return .terminateCancel }
        device?.shutdown()
        return .terminateNow
    }

    func attachWindow() {
        NSApplication.shared.windows.first(where: { $0.canBecomeMain })?.delegate = self
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard canClose() else { return false }
        closeApproved = true
        return true
    }

    private func canClose() -> Bool {
        if library?.busy == true || device?.library.busy == true {
            let alert = NSAlert()
            alert.messageText = "Wait for the library operation to finish."
            alert.informativeText = "The app is reading or saving your local audio files."
            alert.runModal()
            return false
        }
        return library?.confirmDiscard() != false && device?.library.confirmDiscard() != false
    }

    func consumePending() {
        guard let openFiles, !pending.isEmpty else { return }
        let urls = pending
        pending = []
        openFiles(urls)
    }

    // files dropped on the app's icon (or opened with it)
    func application(_ application: NSApplication, open urls: [URL]) {
        Task { @MainActor in
            if let openFiles { openFiles(urls) } else { pending += urls }
        }
    }

    // Files dropped on the Dock icon while the app runs arrive as an "open
    // documents" event. Left to SwiftUI, it can close the window, and the app
    // then quits; handled here, the files go to the current section. The
    // handler starts after launch: files that launch the app still go
    // through SwiftUI, which then opens the window.
    private func handleOpenDocuments() {
        NSAppleEventManager.shared().setEventHandler(self, andSelector: #selector(openDocuments(_:withReply:)),
                                                     forEventClass: AEEventClass(kCoreEventClass), andEventID: AEEventID(kAEOpenDocuments))
    }

    @objc private func openDocuments(_ event: NSAppleEventDescriptor, withReply reply: NSAppleEventDescriptor) {
        guard let list = event.paramDescriptor(forKeyword: keyDirectObject) else { return }
        let items = list.numberOfItems > 0 ? (1...list.numberOfItems).compactMap { list.atIndex($0) } : [list]
        let urls = items.compactMap(\.fileURLValue)
        if !urls.isEmpty { application(NSApplication.shared, open: urls) }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        handleOpenDocuments()
        Task { @MainActor in
            consumePending()
        }
    }
}
