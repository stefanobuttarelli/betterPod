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

/// The window: a glossy header across the top with the section's title in the
/// middle (as on the console) and its few actions on the right, a fixed source
/// list on the left, and the section below.
struct MacWorkspace: View {
    @ObservedObject var model: Model
    @ObservedObject var library: LibraryModel
    @ObservedObject var updater: Updater
    @ObservedObject var device: DeviceModel
    @Binding var section: MacSection

    private var updatesBlocked: Bool {
        model.shareGoing || library.updatesBlocked || device.updatesBlocked
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            HStack(spacing: 0) {
                sidebar
                Pod.sidebarBorder.frame(width: 1)
                detail.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .ignoresSafeArea()
        .frame(minWidth: 1100, minHeight: 680)
        .tint(Pod.accent)
    }

    private var header: some View {
        ZStack {
            PodChrome()
            WindowDragArea()
            VStack(spacing: 1) {
                Text(title).font(.system(size: 13, weight: .bold)).foregroundStyle(Pod.chromeText)
                Text(subtitle).font(.system(size: 11)).foregroundStyle(Pod.secondaryText).lineLimit(1)
            }
            .frame(maxWidth: 420)
            .allowsHitTesting(false)
            HStack(spacing: 8) {
                Spacer()
                switch section {
                case .library: LibraryHeaderActions(model: library)
                case .device: if device.connected { LibraryHeaderActions(model: device.library, device: device) }
                case .share: EmptyView()
                }
            }
            .padding(.leading, 90).padding(.trailing, 14)
        }
        .frame(height: Pod.headerHeight)
    }

    private var title: String {
        switch section {
        case .library: return "Library"
        case .share: return "Share"
        case .device: return device.connected ? device.library.location : "betterPod over Wi-Fi"
        }
    }

    private var subtitle: String {
        switch section {
        case .library: return library.location.isEmpty ? "Albums on this Mac" : library.location
        case .share: return "Send songs and albums with an eight-digit code"
        case .device:
            if !device.connected { return "Not connected" }
            return device.progress.isEmpty ? "Connected over your local network" : device.progress
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 1) {
            PodSectionTitle("On This Mac").padding(.horizontal, 14).padding(.top, 14).padding(.bottom, 5)
            SourceRow(item: .library, selected: section == .library) { section = .library }
            SourceRow(item: .share, selected: section == .share) { section = .share }
            PodSectionTitle("Devices").padding(.horizontal, 14).padding(.top, 16).padding(.bottom, 5)
            SourceRow(item: .device, selected: section == .device, connected: device.connected) { section = .device }
            Spacer()
        }
        .frame(width: Pod.sidebarWidth)
        .frame(maxHeight: .infinity)
        .background(LinearGradient(colors: [Pod.sidebarTop, Pod.sidebarBottom], startPoint: .top, endPoint: .bottom))
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
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// A source list row; the selected one wears the console's blue cursor.
private struct SourceRow: View {
    let item: MacSection
    let selected: Bool
    var connected = false
    let action: () -> Void
    @Environment(\.controlActiveState) private var activeState

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: item.icon)
                    .font(.system(size: 13, weight: .medium))
                    .frame(width: 18)
                    .foregroundStyle(selected ? Color.white : Pod.accent)
                Text(item == .device ? "betterPod" : item.rawValue)
                    .font(.system(size: 13, weight: selected ? .semibold : .regular))
                    .foregroundStyle(selected ? Color.white : Pod.chromeText)
                Spacer()
                if connected {
                    Circle().fill(Pod.saved).frame(width: 7, height: 7)
                        .overlay(Circle().strokeBorder(Color.white.opacity(selected ? 0.8 : 0), lineWidth: 1))
                        .accessibilityLabel("Connected")
                }
            }
            .padding(.horizontal, 14)
            .frame(height: 26)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background { if selected { PodSelection(active: activeState != .inactive) } }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(item == .device ? "betterPod over Wi-Fi" : item.rawValue)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// The few actions a library offers in the header: open (on this Mac) or
/// refresh and disconnect (betterPod), Find Album, and Save.
struct LibraryHeaderActions: View {
    @ObservedObject var model: LibraryModel
    var device: DeviceModel?

    var body: some View {
        HStack(spacing: 8) {
            if let device {
                Button { device.refresh() } label: { Label("Refresh", systemImage: "arrow.clockwise") }
                    .disabled(device.busy)
                Button { device.disconnect() } label: { Label("Disconnect", systemImage: "wifi.slash") }
                    .disabled(model.busy)
            } else {
                Button { model.choose() } label: { Label("Open", systemImage: "folder") }
                    .help("Open an Album Folder or Songs (⌘O)")
                    .disabled(model.busy)
            }
            Button { model.beginLookup() } label: { Label("Find Album", systemImage: "magnifyingglass") }
                .help("Find the cover, album details and track names of the selected albums (⌘F)")
                .disabled(!model.canLookup)
            Button { model.save() } label: {
                Label(model.hasChanges ? "Save Changes (\(model.dirtyCount))" : "Save Changes", systemImage: "checkmark")
            }
            .buttonStyle(PodButtonStyle(kind: .primary))
            .help(model.isRemote ? "Save changed tags and covers on betterPod (⌘S)" : "Save changed tags and covers to your files (⌘S)")
            .disabled(!model.canSave)
        }
        .buttonStyle(PodButtonStyle())
    }
}
