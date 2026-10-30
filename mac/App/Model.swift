// Model.swift — what the window shows: nothing yet, the songs dropped (as
// the betterPod will see them), then the share with its code.

import AppKit
import Combine
import SwiftUI

@MainActor
final class Model: ObservableObject {
    enum Stage { case empty, reading, ready, sharing }

    @Published var stage: Stage = .empty
    @Published var scan: ScanResult?
    @Published var scanError: String?
    @Published var status: ShareStatus?
    @Published var cover: NSImage?
    @Published var albumCovers: [String: NSImage] = [:]

    private var shareID = 0
    private var timer: Timer?
    private var dropped: [URL] = []

    /// The name the betterPod shows ("from Stefano's MacBook").
    @AppStorage("fromName") var fromName: String = Host.current().localizedName ?? "Mac"

    func open(_ urls: [URL]) {
        guard !urls.isEmpty, stage != .sharing else { return }
        dropped = urls
        stage = .reading
        scanError = nil
        Task.detached(priority: .userInitiated) {
            let r = Engine.scan(urls)
            await MainActor.run { self.show(r) }
        }
    }

    private func show(_ r: ScanResult) {
        if let e = r.error {
            scan = nil
            scanError = e
            stage = .empty
            return
        }
        scan = r
        cover = r.cover.flatMap { NSImage(data: $0) }
        var covers: [String: NSImage] = [:]
        for a in r.albums ?? [] {
            if let d = a.cover, let img = NSImage(data: d) {
                covers[a.id] = img
            }
        }
        albumCovers = covers
        stage = .ready
    }

    func choose() {
        let p = NSOpenPanel()
        p.canChooseFiles = true
        p.canChooseDirectories = true
        p.allowsMultipleSelection = true
        p.prompt = "Choose"
        p.message = "Songs or album folders to share"
        if p.runModal() == .OK {
            open(p.urls)
        }
    }

    func share() {
        guard let id = scan?.id else { return }
        let name = fromName.trimmingCharacters(in: .whitespaces)
        shareID = Engine.send(selection: id, from: String(name.prefix(32)))
        status = Engine.status(shareID)
        stage = .sharing
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            guard let model = self else { return }
            Task { @MainActor in model.poll() }
        }
    }

    private func poll() {
        guard shareID != 0, let s = Engine.status(shareID) else { return }
        status = s
        if s.phase == "done" || s.phase == "failed" {
            timer?.invalidate()
            timer = nil
        }
    }

    /// Stops the share (if it still goes) and starts over.
    func clear() {
        stopShare()
        scan = nil
        cover = nil
        albumCovers = [:]
        scanError = nil
        stage = .empty
    }

    /// Back to the songs, to share them again (a new code).
    func back() {
        stopShare()
        stage = scan == nil ? .empty : .ready
    }

    func again() {
        stopShare()
        // the files may have changed meanwhile: read them again
        open(dropped)
    }

    private func stopShare() {
        timer?.invalidate()
        timer = nil
        if shareID != 0 {
            Engine.cancel(shareID)
            shareID = 0
        }
        status = nil
    }

    /// A share is under way (not once it's sent or has failed): the app
    /// doesn't update itself meanwhile.
    var shareGoing: Bool {
        guard stage == .sharing else { return false }
        let p = status?.phase ?? "preparing"
        return p != "done" && p != "failed"
    }

    /// About how long the rest takes, at the speed of now (as the
    /// betterPod says it).
    var timeLeft: String? {
        guard let s = status, s.phase == "moving", s.rate > 0, s.total > s.done else { return nil }
        let sec = Double(s.total - s.done) / s.rate
        switch sec {
        case ..<10: return "A few seconds left"
        case ..<60: return "About \((Int(sec) + 4) / 5 * 5) s left"
        case ..<90: return "About a minute left"
        case ..<3600: return "About \(Int(sec / 60 + 0.5)) min left"
        default: return "About \(Int(sec / 3600)) h \(Int(sec / 60) % 60) min left"
        }
    }

    var progress: Double {
        guard let s = status, s.total > 0 else { return 0 }
        if s.phase == "done" { return 1 }
        return min(1, Double(s.done) / Double(s.total))
    }
}
