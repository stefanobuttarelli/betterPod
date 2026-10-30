// Updater.swift — the app updates itself, as Software Update does on the
// consoles: the newest mac-v… release on GitHub, its zip checked (the size
// and the SHA-256 GitHub gives for it), the app replaced where it is, then
// started again.

import AppKit
import CryptoKit
import Foundation

/// Where the releases are.
let updateRepo = "stefanobuttarelli/betterPod"

struct UpdateError: Error {
    let text: String
    init(_ text: String) { self.text = text }
}

@MainActor
final class Updater: ObservableObject {
    enum State: Equatable {
        case idle
        case checking          // by hand, from the menu
        case upToDate          // by hand: nothing newer
        case available(String) // the new version
        case updating(String)
        case failed(String)    // what went wrong
    }

    struct Found {
        let version: String
        let url: URL
        let size: Int64
        let sha256: String
        let page: URL // the release on GitHub, to download it by hand
    }

    @Published var state: State = .idle
    private var found: Found?
    /// The new version's page on GitHub, when the update can't be done here.
    var page: URL? { found?.page }
    private var checkedAtStart = false

    static var current: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }

    /// Once, when the window opens: says nothing when there is no newer
    /// version or GitHub can't be reached.
    func checkAtStart() {
        guard !checkedAtStart else { return }
        checkedAtStart = true
        check(byHand: false)
    }

    /// Looks for a newer version; by hand (the menu) it also says when
    /// there is none, or when GitHub can't be reached.
    func check(byHand: Bool) {
        if case .updating = state { return }
        if byHand { state = .checking }
        Task {
            do {
                if let f = try await Self.newest(), Self.newer(f.version, than: Self.current) {
                    found = f
                    state = .available(f.version)
                } else if byHand {
                    state = .upToDate
                    try? await Task.sleep(nanoseconds: 4_000_000_000)
                    if state == .upToDate { state = .idle }
                }
            } catch {
                if byHand { state = .failed("Couldn't reach GitHub to check for updates.") }
            }
        }
    }

    /// Downloads the new version, checks it, puts it in place of this one
    /// and starts it.
    func install() {
        guard let f = found else { return }
        state = .updating(f.version)
        Task {
            do {
                let app = try Self.replaceable()
                let (new, dir) = try await Self.fetch(f, beside: app)
                _ = try FileManager.default.replaceItemAt(app, withItemAt: new)
                try? FileManager.default.removeItem(at: dir)
                Self.relaunch(app)
            } catch let e as UpdateError {
                state = .failed(e.text)
            } catch {
                state = .failed("The update didn't work: \(error.localizedDescription)")
            }
        }
    }

    /// Back from a message: to the update on offer, if there is one.
    func dismiss() {
        if let f = found { state = .available(f.version) } else { state = .idle }
    }

    // ---- GitHub ----

    private struct Release: Decodable {
        let tag_name: String
        let draft: Bool
        let prerelease: Bool
        let html_url: URL
        let assets: [Asset]
    }

    private struct Asset: Decodable {
        let name: String
        let size: Int64
        let digest: String?
        let browser_download_url: URL
    }

    /// The newest release of the Mac app (tagged mac-v…) and its zip. The
    /// consoles' releases come in between, newest first: a few pages at most.
    nonisolated static func newest() async throws -> Found? {
        for page in 1...5 {
            var req = URLRequest(url: URL(string: "https://api.github.com/repos/\(updateRepo)/releases?per_page=100&page=\(page)")!)
            req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
            req.timeoutInterval = 15
            let (data, resp) = try await URLSession.shared.data(for: req)
            guard (resp as? HTTPURLResponse)?.statusCode == 200 else { throw URLError(.badServerResponse) }
            let releases = try JSONDecoder().decode([Release].self, from: data)
            if let r = releases.first(where: { $0.tag_name.hasPrefix("mac-v") && !$0.draft && !$0.prerelease }) {
                let v = String(r.tag_name.dropFirst("mac-v".count))
                guard let a = r.assets.first(where: { $0.name == "betterPod-Share-mac-v\(v).zip" }),
                      let d = a.digest, d.hasPrefix("sha256:") else { return nil }
                return Found(version: v, url: a.browser_download_url, size: a.size,
                             sha256: String(d.dropFirst("sha256:".count)), page: r.html_url)
            }
            if releases.count < 100 { break }
        }
        return nil
    }

    /// a is a newer version than b (1.0.2 > 1.0.1 > 1.0).
    nonisolated static func newer(_ a: String, than b: String) -> Bool {
        let x = a.split(separator: ".").map { Int($0) ?? 0 }
        let y = b.split(separator: ".").map { Int($0) ?? 0 }
        for i in 0..<max(x.count, y.count) {
            let p = i < x.count ? x[i] : 0, q = i < y.count ? y[i] : 0
            if p != q { return p > q }
        }
        return false
    }

    // ---- the update ----

    /// This app, when it can be replaced where it is: not run from the
    /// read-only copy macOS makes of an app opened from Downloads, nor from a
    /// folder it can't write to.
    static func replaceable() throws -> URL {
        let app = Bundle.main.bundleURL
        let folder = app.deletingLastPathComponent()
        if app.path.contains("/AppTranslocation/") {
            throw UpdateError("macOS runs betterPod Share from a copy of its own. Move it to Applications, open it from there, then update.")
        }
        let fm = FileManager.default
        guard fm.isWritableFile(atPath: folder.path), fm.isWritableFile(atPath: app.path) else {
            throw UpdateError("betterPod Share can't replace itself in “\(fm.displayName(atPath: folder.path))”: download the new version instead.")
        }
        return app
    }

    /// Downloads the zip next to the app (on the same disk, for the swap),
    /// checks it and unpacks it: the new app, and the folder to remove after.
    nonisolated static func fetch(_ f: Found, beside app: URL) async throws -> (URL, URL) {
        let fm = FileManager.default
        let dir = try fm.url(for: .itemReplacementDirectory, in: .userDomainMask, appropriateFor: app, create: true)
        let (tmp, resp) = try await URLSession.shared.download(from: f.url)
        guard (resp as? HTTPURLResponse)?.statusCode == 200 else {
            throw UpdateError("The download didn't work: nothing was changed.")
        }
        let zip = dir.appendingPathComponent("update.zip")
        try fm.moveItem(at: tmp, to: zip)
        let data = try Data(contentsOf: zip)
        let sum = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        guard Int64(data.count) == f.size, sum == f.sha256.lowercased() else {
            throw UpdateError("The download doesn't match GitHub's checksum: nothing was changed.")
        }
        try run("/usr/bin/ditto", ["-x", "-k", zip.path, dir.path])
        guard let new = try fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil).first(where: { $0.pathExtension == "app" }),
              let info = NSDictionary(contentsOf: new.appendingPathComponent("Contents/Info.plist")),
              info["CFBundleIdentifier"] as? String == Bundle.main.bundleIdentifier,
              info["CFBundleShortVersionString"] as? String == f.version else {
            throw UpdateError("The download isn't betterPod Share \(f.version): nothing was changed.")
        }
        // its signature holds (every file as it was signed)
        try run("/usr/bin/codesign", ["--verify", "--deep", new.path])
        return (new, dir)
    }

    nonisolated static func run(_ tool: String, _ args: [String]) throws {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: tool)
        p.arguments = args
        try p.run()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else {
            throw UpdateError("The download couldn't be checked (\((tool as NSString).lastPathComponent)): nothing was changed.")
        }
    }

    /// Quits and opens the app again (the new one) once this one is gone.
    static func relaunch(_ app: URL) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", "while /bin/kill -0 \(ProcessInfo.processInfo.processIdentifier) 2>/dev/null; do /bin/sleep 0.2; done; /usr/bin/open \"$0\"", app.path]
        try? p.run()
        NSApp.terminate(nil)
    }
}
