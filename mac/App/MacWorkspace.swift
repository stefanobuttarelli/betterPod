import SwiftUI

enum MacSection: String, CaseIterable {
    case library = "Library"
    case share = "Share"
    case device = "betterPod"
    var icon: String {
        switch self {
        case .library: return "music.note.list"
        case .share: return "paperplane"
        case .device: return "wifi"
        }
    }
}

struct MacWorkspace: View {
    @ObservedObject var model: Model
    @ObservedObject var library: LibraryModel
    @ObservedObject var updater: Updater
    @ObservedObject var device: DeviceModel
    @Binding var section: MacSection
    @State private var visibility = NavigationSplitViewVisibility.all

    private var selection: Binding<MacSection?> {
        Binding(get: { section }, set: { if let value = $0 { section = value } })
    }

    private var updatesBlocked: Bool {
        model.shareGoing || library.updatesBlocked || device.updatesBlocked
    }

    var body: some View {
        NavigationSplitView(columnVisibility: $visibility) {
            List(selection: selection) {
                Section("On This Mac") {
                    sidebarRow(.library)
                    sidebarRow(.share)
                }
                Section("Devices") {
                    sidebarRow(.device)
                }
            }
            .listStyle(.sidebar)
            .navigationTitle("betterPod")
            .navigationSplitViewColumnWidth(min: 175, ideal: 190, max: 240)
        } detail: {
            detail.navigationTitle(section.rawValue)
        }
        .navigationSplitViewStyle(.balanced)
        .frame(minWidth: 1100, minHeight: 680)
    }

    private func sidebarRow(_ item: MacSection) -> some View {
        Label(item.rawValue, systemImage: item.icon)
            .tag(item)
            .accessibilityLabel(item == .device ? "betterPod over Wi-Fi" : item.rawValue)
    }

    @ViewBuilder private var detail: some View {
        switch section {
        case .library:
            LibraryView(model: library)
                .safeAreaInset(edge: .top, spacing: 0) { updateNotice }
        case .device:
            DeviceLibraryView(model: device)
                .safeAreaInset(edge: .top, spacing: 0) { updateNotice }
        case .share:
            ContentView(model: model, updater: updater, updatesBlocked: library.updatesBlocked || device.updatesBlocked)
        }
    }

    @ViewBuilder private var updateNotice: some View {
        if updater.state != .idle {
            UpdateStrip(updater: updater, sharing: updatesBlocked)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
