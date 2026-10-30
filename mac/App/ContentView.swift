// ContentView.swift — the window: betterPod's Share studio (the same
// pedestal, laser and digit tiles as on the console, at the same size), the
// cover on the left, what goes and the code on the right.

import AppKit
import SwiftUI
import UniformTypeIdentifiers

let ink = Color(red: 40 / 255, green: 44 / 255, blue: 52 / 255)
let subInk = Color(red: 110 / 255, green: 118 / 255, blue: 130 / 255)
let bodyInk = Color(red: 70 / 255, green: 76 / 255, blue: 88 / 255)
let shareBlue = Color(red: 28 / 255, green: 112 / 255, blue: 218 / 255)
let laserBlue = Color(red: 40 / 255, green: 150 / 255, blue: 255 / 255)
let shareGreen = Color(red: 40 / 255, green: 150 / 255, blue: 70 / 255)
let shareRed = Color(red: 200 / 255, green: 60 / 255, blue: 50 / 255)

// the console's coordinates (its 640 × 480 screen, in points here)
let pedTop: CGFloat = 292  // the pedestal's top (its disc's centre)
let pedCX: CGFloat = 180
let textCX: CGFloat = 468
let coverSide: CGFloat = 150

func resourceImage(_ name: String) -> NSImage? {
    guard let url = Bundle.main.url(forResource: name, withExtension: "png") else { return nil }
    return NSImage(contentsOf: url)
}

struct ContentView: View {
    @ObservedObject var model: Model
    @ObservedObject var updater: Updater
    @State private var targeted = false

    var body: some View {
        ZStack(alignment: .topLeading) {
            Studio()
            Stage(model: model)
            Panel(model: model)
                .frame(width: 300)
                .position(x: textCX, y: 210)
            if model.stage != .sharing {
                NameField(model: model)
                    .position(x: 320, y: 452)
            }
            if updater.state != .idle {
                UpdateStrip(updater: updater, sharing: model.stage == .sharing)
                    .frame(maxWidth: 560)
                    .position(x: 320, y: 30)
            }
            if targeted {
                RoundedRectangle(cornerRadius: 10)
                    .strokeBorder(shareBlue.opacity(0.7), style: StrokeStyle(lineWidth: 3, dash: [10, 6]))
                    .padding(8)
                    .allowsHitTesting(false)
            }
        }
        .frame(width: 640, height: 480)
        .onDrop(of: [.fileURL], isTargeted: $targeted) { providers in
            loadURLs(providers) { model.open($0) }
            return true
        }
    }
}

/// The file URLs of a drop, all of them, then done (on the main thread).
func loadURLs(_ providers: [NSItemProvider], done: @escaping ([URL]) -> Void) {
    var urls: [URL] = []
    let group = DispatchGroup()
    let lock = NSLock()
    for p in providers where p.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
        group.enter()
        _ = p.loadObject(ofClass: URL.self) { url, _ in
            if let url {
                lock.lock()
                urls.append(url)
                lock.unlock()
            }
            group.leave()
        }
    }
    group.notify(queue: .main) { done(urls) }
}

/// The studio: a light backdrop and a pale floor.
struct Studio: View {
    var body: some View {
        VStack(spacing: 0) {
            LinearGradient(colors: [Color(white: 0.985), Color(red: 0.9, green: 0.91, blue: 0.93)],
                           startPoint: .top, endPoint: .bottom)
                .frame(height: 330)
            LinearGradient(colors: [Color(red: 0.93, green: 0.94, blue: 0.95), Color.white],
                           startPoint: .top, endPoint: .bottom)
        }
    }
}

/// The pedestal, its light, and the cover on it (with the laser while it goes).
struct Stage: View {
    @ObservedObject var model: Model
    @State private var start = Date()

    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 30, paused: !animating)) { tl in
            let t = tl.date.timeIntervalSince(start)
            ZStack(alignment: .topLeading) {
                if let beam = resourceImage("share_beam_blue") {
                    Image(nsImage: beam)
                        .opacity(beamLevel(t))
                        .position(x: pedCX, y: pedTop - 226 + 120)
                }
                if let ped = resourceImage("share_ped_blue") {
                    Image(nsImage: ped)
                        .position(x: pedCX, y: pedTop - 24 + 75)
                }
                cover(t)
            }
        }
        .frame(width: 640, height: 480, alignment: .topLeading)
    }

    var animating: Bool {
        switch model.stage {
        case .empty, .reading: return true
        case .ready: return false
        case .sharing: return model.status?.phase != "done" && model.status?.phase != "failed"
        }
    }

    func beamLevel(_ t: Double) -> Double {
        switch model.stage {
        case .empty, .reading:
            return 0.55 + 0.45 * (0.5 + 0.5 * sin(t / 5 * 2 * .pi))
        case .ready:
            return 0.8
        case .sharing:
            switch model.status?.phase {
            case "moving": return 1
            case "done", "failed": return 0.5
            default: return 0.55 + 0.45 * (0.5 + 0.5 * sin(t / 5 * 2 * .pi))
            }
        }
    }

    @ViewBuilder func cover(_ t: Double) -> some View {
        let x0 = pedCX - coverSide / 2
        let y0 = pedTop - 24 - coverSide
        if model.stage == .empty || model.stage == .reading {
            // where the cover will be
            RoundedRectangle(cornerRadius: 4)
                .strokeBorder(shareBlue.opacity(0.35), style: StrokeStyle(lineWidth: 2, dash: [7, 5]))
                .frame(width: coverSide, height: coverSide)
                .overlay {
                    if model.stage == .reading {
                        ProgressView().controlSize(.small)
                    } else {
                        Image(systemName: "music.note")
                            .font(.system(size: 40, weight: .light))
                            .foregroundColor(shareBlue.opacity(0.35))
                    }
                }
                .position(x: pedCX, y: y0 + coverSide / 2)
        } else {
            let p = model.progress
            let phase = model.status?.phase ?? ""
            ZStack(alignment: .top) {
                coverImage
                if model.stage == .sharing && (phase == "moving" || phase == "done") {
                    // what has gone turns pale
                    Rectangle()
                        .fill(Color.white.opacity(0.62))
                        .frame(height: coverSide * p)
                        .frame(maxHeight: .infinity, alignment: .top)
                }
            }
            .frame(width: coverSide, height: coverSide)
            .clipped()
            .shadow(color: .black.opacity(0.25), radius: 4, y: 2)
            .opacity(phase == "failed" ? 0.5 : 1)
            .position(x: pedCX, y: y0 + coverSide / 2)
            if model.stage == .sharing && phase != "done" && phase != "failed" {
                // waiting: a slow sweep up and down; sending: on the edge
                // between what's gone and what's still to go
                let y = phase == "moving"
                    ? y0 + p * coverSide
                    : y0 + coverSide * (0.5 - 0.5 * cos(t.truncatingRemainder(dividingBy: 7) / 7 * 2 * .pi))
                Laser()
                    .frame(width: coverSide + 36, height: 14)
                    .position(x: x0 + coverSide / 2, y: y)
            }
        }
    }

    @ViewBuilder var coverImage: some View {
        if let img = model.cover {
            Image(nsImage: img).resizable().interpolation(.high)
        } else {
            ZStack {
                LinearGradient(colors: [Color(white: 0.55), Color(white: 0.35)], startPoint: .top, endPoint: .bottom)
                Image(systemName: "music.note").font(.system(size: 54, weight: .light)).foregroundColor(.white.opacity(0.8))
            }
        }
    }
}

/// The laser: a blue line with a soft glow and a dot at each end.
struct Laser: View {
    var body: some View {
        GeometryReader { g in
            let w = g.size.width, cy = g.size.height / 2
            ZStack {
                Capsule().fill(laserBlue.opacity(0.12)).frame(width: w, height: 9)
                Capsule().fill(laserBlue.opacity(0.25)).frame(width: w, height: 5)
                Capsule().fill(laserBlue.opacity(0.9)).frame(width: w, height: 3)
                Capsule().fill(Color.white).frame(width: w - 36, height: 1)
                ForEach([6.0, w - 6], id: \.self) { x in
                    ZStack {
                        Circle().fill(Color.white.opacity(0.5)).frame(width: 12, height: 12)
                        Circle().fill(laserBlue).frame(width: 6, height: 6)
                    }
                    .position(x: x, y: cy)
                }
            }
            .frame(width: w, height: g.size.height)
        }
    }
}

/// The right side: what goes, the code, how it goes.
struct Panel: View {
    @ObservedObject var model: Model

    var body: some View {
        VStack(spacing: 10) {
            switch model.stage {
            case .empty, .reading: emptyPanel
            case .ready: readyPanel
            case .sharing: sharingPanel
            }
        }
        .multilineTextAlignment(.center)
    }

    var emptyPanel: some View {
        VStack(spacing: 12) {
            Text("SHARE WITH A BETTERPOD").font(.system(size: 13, weight: .semibold)).foregroundColor(subInk)
            Text(model.stage == .reading ? "Reading…" : "Drop songs or albums here")
                .font(.system(size: 20, weight: .bold)).foregroundColor(ink)
            Text("A whole album, a few songs, or songs from several albums: they arrive on the betterPod as albums, with their covers.")
                .font(.system(size: 13)).foregroundColor(subInk).fixedSize(horizontal: false, vertical: true)
            if let e = model.scanError {
                Text(e).font(.system(size: 13, weight: .medium)).foregroundColor(shareRed)
            }
            Button("Choose…") { model.choose() }
                .controlSize(.large)
                .disabled(model.stage == .reading)
                .padding(.top, 6)
        }
    }

    var subtitle: String {
        guard let s = model.scan else { return "" }
        var parts: [String] = []
        if let a = s.artist, !a.isEmpty { parts.append(a) }
        if let n = s.songs, n > 1 { parts.append("\(n) songs") }
        if let b = s.bytes { parts.append(humanBytes(b)) }
        return parts.joined(separator: " · ")
    }

    @ViewBuilder var titleBlock: some View {
        Text(model.scan?.title ?? "").font(.system(size: 20, weight: .bold)).foregroundColor(ink).lineLimit(1)
        Text(subtitle).font(.system(size: 15, weight: .medium)).foregroundColor(subInk).lineLimit(1)
    }

    var readyPanel: some View {
        VStack(spacing: 6) {
            Text(model.scan?.kind == "mix" ? "\(model.scan?.albums?.count ?? 0) ALBUMS, ONE CODE" : "READY")
                .font(.system(size: 13, weight: .semibold)).foregroundColor(subInk)
            titleBlock
            if model.scan?.kind == "mix", let albums = model.scan?.albums {
                AlbumList(albums: albums, covers: model.albumCovers)
                    .frame(height: min(CGFloat(albums.count), 4) * 34)
                    .padding(.top, 6)
            }
            notes.padding(.top, 4)
            HStack(spacing: 10) {
                Button("Clear") { model.clear() }.controlSize(.large)
                Button("Share") { model.share() }
                    .controlSize(.large)
                    .keyboardShortcut(.defaultAction)
            }
            .padding(.top, 10)
        }
    }

    @ViewBuilder var notes: some View {
        if let sk = model.scan?.skipped, !sk.isEmpty {
            Text(sk.count == 1 ? "\(sk[0]) stays here: betterPod can't play it."
                 : "\(sk.count) files stay here: betterPod can't play them.")
                .font(.system(size: 12)).foregroundColor(subInk)
        }
        if model.scan?.m4a == true {
            Text("A Miyoo can't play M4A: it will say no before anything is sent.")
                .font(.system(size: 12)).foregroundColor(subInk)
        }
    }

    var sharingPanel: some View {
        let s = model.status
        let phase = s?.phase ?? "preparing"
        let label: (String, Color) = {
            switch phase {
            case "moving": return ("SHARING", shareBlue)
            case "done": return ("SENT", shareGreen)
            case "failed": return ("NOT SENT", shareRed)
            default: return ("YOUR CODE", subInk)
            }
        }()
        return VStack(spacing: 6) {
            Text(label.0).font(.system(size: 13, weight: .semibold)).foregroundColor(label.1)
            Digits(code: s?.code ?? "", flipping: phase == "preparing")
                .padding(.bottom, 14)
            titleBlock
            Group {
                switch phase {
                case "preparing":
                    Text("Getting ready…").font(.system(size: 15, weight: .medium)).foregroundColor(bodyInk)
                case "waiting":
                    Text("Waiting for a friend…").font(.system(size: 17, weight: .semibold)).foregroundColor(bodyInk)
                    Text("On their betterPod: Music → Receive,\nand these eight digits")
                        .font(.system(size: 13, weight: .medium)).foregroundColor(subInk)
                case "moving":
                    Text("Sending…").font(.system(size: 15, weight: .medium)).foregroundColor(bodyInk)
                    ProgressBar(value: model.progress).frame(width: 216, height: 12)
                    Text("\(Int(model.progress * 100))% · \(humanBytes(Int64(s?.rate ?? 0)))/s")
                        .font(.system(size: 14, weight: .medium)).foregroundColor(subInk)
                case "done":
                    Text("Sent! It's on their betterPod now.").font(.system(size: 15, weight: .semibold)).foregroundColor(bodyInk)
                default:
                    Text(s?.error ?? "It didn't work").font(.system(size: 13, weight: .medium)).foregroundColor(shareRed)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(.top, 8)
            HStack(spacing: 10) {
                switch phase {
                case "done":
                    Button("Share Again") { model.again() }.controlSize(.large)
                    Button("Share More") { model.clear() }.controlSize(.large).keyboardShortcut(.defaultAction)
                case "failed":
                    Button("Back") { model.back() }.controlSize(.large)
                    if s?.unplayable == nil {
                        Button("Try Again") { model.again() }.controlSize(.large).keyboardShortcut(.defaultAction)
                    }
                default:
                    Button("Cancel") { model.back() }.controlSize(.large).keyboardShortcut(.cancelAction)
                }
            }
            .padding(.top, 14)
        }
    }
}

/// The eight digits, in white tiles (4 and 4); they spin into place when
/// the code appears.
struct Digits: View {
    let code: String
    let flipping: Bool
    @State private var start = Date()

    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 20, paused: !flipping)) { tl in
            let el = tl.date.timeIntervalSince(start)
            HStack(spacing: 3) {
                ForEach(0..<8, id: \.self) { i in
                    let ch: String = {
                        let chars = Array(code)
                        if flipping && el < 0.3 + Double(i) * 0.09 || i >= chars.count {
                            return String(Int(el * 16 + Double(i * 3)) % 10)
                        }
                        return String(chars[i])
                    }()
                    DigitTile(digit: ch)
                        .padding(.leading, i == 4 ? 8 : 0)
                }
            }
        }
    }
}

struct DigitTile: View {
    let digit: String
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 5)
                .fill(LinearGradient(colors: [Color.white, Color(red: 0.91, green: 0.92, blue: 0.94)],
                                     startPoint: .top, endPoint: .bottom))
            RoundedRectangle(cornerRadius: 5)
                .strokeBorder(Color(red: 0.76, green: 0.78, blue: 0.81), lineWidth: 1)
            Rectangle().fill(Color.black.opacity(0.07)).frame(height: 1)
            Text(digit).font(.system(size: 28, weight: .semibold, design: .rounded)).foregroundColor(ink)
        }
        .frame(width: 26, height: 40)
        .shadow(color: .black.opacity(0.12), radius: 1, y: 1)
    }
}

/// A blue bar in a pale groove.
struct ProgressBar: View {
    let value: Double
    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .leading) {
                Capsule().fill(Color(red: 0.86, green: 0.87, blue: 0.89))
                Capsule().strokeBorder(Color.black.opacity(0.12), lineWidth: 1)
                Capsule()
                    .fill(LinearGradient(colors: [Color(red: 0.45, green: 0.72, blue: 1), shareBlue],
                                         startPoint: .top, endPoint: .bottom))
                    .frame(width: max(g.size.height, g.size.width * value))
                    .opacity(value > 0 ? 1 : 0)
            }
        }
    }
}

/// A mix's albums: cover, name, songs.
struct AlbumList: View {
    let albums: [ScanAlbum]
    let covers: [String: NSImage]
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(albums) { a in
                    HStack(spacing: 8) {
                        Group {
                            if let img = covers[a.id] {
                                Image(nsImage: img).resizable()
                            } else {
                                Rectangle().fill(Color(white: 0.6))
                            }
                        }
                        .frame(width: 30, height: 30)
                        .clipShape(RoundedRectangle(cornerRadius: 2))
                        VStack(alignment: .leading, spacing: 0) {
                            Text(a.album).font(.system(size: 13, weight: .semibold)).foregroundColor(ink).lineLimit(1)
                            Text(a.artist.isEmpty ? "\(a.songs) \(a.songs == 1 ? "song" : "songs")"
                                 : "\(a.artist) · \(a.songs) \(a.songs == 1 ? "song" : "songs")")
                                .font(.system(size: 11)).foregroundColor(subInk).lineLimit(1)
                        }
                        Spacer(minLength: 0)
                    }
                }
            }
        }
    }
}

/// A new version of the app, or how the update goes, at the top of the
/// studio.
struct UpdateStrip: View {
    @ObservedObject var updater: Updater
    let sharing: Bool

    var body: some View {
        HStack(spacing: 10) {
            switch updater.state {
            case .idle:
                EmptyView()
            case .checking:
                ProgressView().controlSize(.small)
                Text("Checking for updates…").foregroundColor(bodyInk)
            case .upToDate:
                Text("betterPod Share \(Updater.current) is the newest version.").foregroundColor(bodyInk)
            case .available(let v):
                Text(sharing ? "betterPod Share \(v) is out: update after this share."
                     : "betterPod Share \(v) is out.").foregroundColor(bodyInk)
                Button("Update") { updater.install() }
                    .disabled(sharing)
            case .updating(let v):
                ProgressView().controlSize(.small)
                Text("Updating to \(v)…").foregroundColor(bodyInk)
            case .failed(let e):
                Text(e).foregroundColor(shareRed).fixedSize(horizontal: false, vertical: true)
                if let page = updater.page {
                    Button("Download") { NSWorkspace.shared.open(page) }
                }
                Button("OK") { updater.dismiss() }
            }
        }
        .font(.system(size: 12, weight: .medium))
        .multilineTextAlignment(.leading)
        .padding(.horizontal, 14)
        .padding(.vertical, 7)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.white.opacity(0.94))
                .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Color.black.opacity(0.1), lineWidth: 1))
                .shadow(color: .black.opacity(0.1), radius: 5, y: 2)
        )
    }
}

/// "Shown as": the name the betterPod shows ("from …").
struct NameField: View {
    @ObservedObject var model: Model
    var body: some View {
        HStack(spacing: 6) {
            Text("Your friends see:").font(.system(size: 12)).foregroundColor(subInk)
            TextField("Name", text: $model.fromName)
                .textFieldStyle(.roundedBorder)
                .font(.system(size: 12))
                .frame(width: 200)
        }
    }
}
