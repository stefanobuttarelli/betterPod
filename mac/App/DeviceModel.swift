import AppKit
import Combine
import Foundation

@MainActor
final class DeviceModel: ObservableObject {
    let library = LibraryModel()
    @Published var devices: [LanDevice] = []
    @Published var selectedDevice = ""
    @Published var code = ""
    @Published var manualHost = ""
    @Published var manualPort = ""
    @Published var useManualAddress = false
    @Published var discovering = false
    @Published var connecting = false
    @Published var connected = false
    @Published var progress = ""
    @Published var error: String?
    private var session: LanSession?
    private var generation = 0
    private var task: Task<Void, Never>?

    var busy: Bool { connecting || library.busy }
    var updatesBlocked: Bool { connected || busy || library.hasChanges }
    var validCode: Bool { code.filter(\.isNumber).count == 8 }

    func discover() {
        guard !discovering, !connecting else { return }
        discovering = true
        error = nil
        Task {
            do {
                let found = try await Task.detached(priority: .utility) { try LanEngine.discover() }.value
                devices = found
                if !found.contains(where: { $0.id == selectedDevice }) { selectedDevice = found.first?.id ?? "" }
            } catch { self.error = error.localizedDescription }
            discovering = false
        }
    }

    func connect() {
        guard !busy, validCode, library.confirmDiscard() else { return }
        let device: LanDevice
        if useManualAddress {
            guard !manualHost.trimmingCharacters(in: .whitespaces).isEmpty else {
                error = "Enter the local IP address shown on betterPod."
                return
            }
            guard let port = Int(manualPort), (1...65535).contains(port) else {
                error = "Enter the port shown on betterPod."
                return
            }
            device = LanDevice(id: "", name: "betterPod", host: manualHost.trimmingCharacters(in: .whitespaces), port: port)
        } else {
            guard let selected = devices.first(where: { $0.id == selectedDevice }) else {
                error = "Find your betterPod on Wi-Fi, or enter its local address."
                return
            }
            device = selected
        }
        generation += 1
        let epoch = generation
        let enteredCode = code, name = Host.current().localizedName ?? "Mac"
        error = nil
        connecting = true
        progress = "Checking the connection code…"
        task = Task {
            do {
                let joined = try await Task.detached(priority: .userInitiated) { try LanEngine.connect(device: device, code: enteredCode, name: name) }.value
                guard generation == epoch, !Task.isCancelled else {
                    LanEngine.disconnect(joined)
                    return
                }
                session = joined
                code = ""
                progress = "Reading the betterPod library…"
                let loaded = try await readAll(joined, epoch: epoch)
                guard generation == epoch, !Task.isCancelled else { return }
                session = loaded.session
                library.showRemote(loaded.session, records: loaded.records)
                connected = true
                connecting = false
                progress = ""
            } catch {
                guard generation == epoch else { return }
                if let session { LanEngine.disconnect(session) }
                session = nil
                connected = false
                connecting = false
                progress = ""
                self.error = error.localizedDescription
            }
        }
    }

    func refresh() {
        guard connected, !busy, let session, library.confirmDiscard() else { return }
        let epoch = generation
        library.busy = true
        error = nil
        progress = "Refreshing the betterPod library…"
        task = Task {
            do {
                let loaded = try await readAll(session, epoch: epoch)
                guard epoch == generation, !Task.isCancelled else { return }
                self.session = loaded.session
                library.showRemote(loaded.session, records: loaded.records)
            } catch {
                guard epoch == generation else { return }
                self.error = error.localizedDescription
                library.remoteNeedsReload = true
            }
            library.busy = false
            progress = ""
        }
    }

    private func readAll(_ session: LanSession, epoch: Int) async throws -> (session: LanSession, records: [LibraryRecord]) {
        var records: [LibraryRecord] = []
        var seen = Set<String>()
        var revision: String?
        var expectedTotal: Int?
        var albumJoining: Bool?
        var offset = 0
        while true {
            try Task.checkCancellation()
            guard generation == epoch else { throw CancellationError() }
            let page = try await Task.detached(priority: .userInitiated) { try LanEngine.page(session, offset: offset) }.value
            guard let total = page.total, (0...200_000).contains(total), let rows = page.tracks, rows.count <= 256,
                  let pageRevision = page.revision, !pageRevision.isEmpty else {
                throw LibraryError("betterPod returned an unreadable library page.")
            }
            if let previous = revision, previous != pageRevision {
                throw LibraryError("The betterPod library changed while loading. Connect or refresh again.")
            }
            if let previous = expectedTotal, previous != total { throw LibraryError("The betterPod library changed while loading. Refresh again.") }
            let supportsJoining = page.capabilities?.contains("album-join-v1") == true
            if let previous = albumJoining, previous != supportsJoining {
                throw LibraryError("The betterPod connection changed while loading. Refresh again.")
            }
            albumJoining = supportsJoining
            expectedTotal = total
            revision = pageRevision
            for row in rows {
                guard seen.insert(row.id).inserted else { throw LibraryError("betterPod returned the same track twice. Refresh its library and try again.") }
                records.append(row)
            }
            let next = page.nextOffset ?? (offset + rows.count)
            guard next >= offset, next <= total, next - offset == rows.count else { throw LibraryError("betterPod returned an invalid library page.") }
            progress = "Reading tracks · \(min(next, total)) of \(total)"
            if next == total { break }
            guard next > offset, next <= total else { throw LibraryError("The betterPod library could not be loaded completely.") }
            offset = next
        }
        var loadedSession = session
        loadedSession.supportsAlbumJoining = albumJoining == true
        return (loadedSession, records)
    }

    func disconnect(askToDiscard: Bool = true) {
        guard !library.busy else { return }
        if askToDiscard && !library.confirmDiscard() { return }
        generation += 1
        task?.cancel()
        task = nil
        if let session { LanEngine.disconnect(session) }
        session = nil
        connected = false
        connecting = false
        progress = ""
        code = ""
        library.remoteDisconnected()
        if askToDiscard { library.discardRemoteDrafts() }
    }

    func shutdown() {
        generation += 1
        task?.cancel()
        if let session { LanEngine.disconnect(session) }
        session = nil
    }
}
