import SwiftUI

private struct MagicJobResult: Sendable {
    let id: String
    let outcome: MagicLibraryOutcome?
    let error: String?
}

private struct MagicToolPresentation: Identifiable {
    enum Kind { case lookup, coverLookup, edit, merge }
    let id = UUID()
    let groupID: String
    let trackIDs: Set<String>
    let kind: Kind
}

struct MagicLibraryView: View {
    @ObservedObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State private var groups: [MagicLibraryGroup] = []
    @State private var outcomes: [String: MagicLibraryOutcome] = [:]
    @State private var errors: [String: String] = [:]
    @State private var selectedID: String?
    @State private var completed = 0
    @State private var runGroupCount = 0
    @State private var runTrackCount = 0
    @State private var working = false
    @State private var ownsBusyState = false
    @State private var stopped = false
    @State private var initialized = false
    @State private var sourceIDs = Set<String>()
    @State private var applied = Set<String>()
    @State private var manuallyEdited = Set<String>()
    @State private var tool: MagicToolPresentation?
    @State private var runID = UUID()
    @State private var request: Task<Void, Never>?

    private var selectedGroup: MagicLibraryGroup? { groups.first { $0.id == selectedID } }
    private var preparedCount: Int { groups.filter { applied.contains($0.id) || manuallyEdited.contains($0.id) }.count }
    private var canWork: Bool { !working && model.canEdit }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            HSplitView {
                groupList.frame(minWidth: 220, idealWidth: 265, maxWidth: 330)
                resultPreview.frame(minWidth: 480, maxWidth: .infinity, maxHeight: .infinity)
            }
            Divider()
            footer
        }
        .frame(minWidth: 860, idealWidth: 1000, minHeight: 600, idealHeight: 740)
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            guard !initialized else { return }
            initialized = true
            sourceIDs = model.selection
            retryAll()
        }
        .onDisappear { stop() }
        .onExitCommand {
            if working { stop() }
            else if tool == nil { dismiss() }
        }
        .onChange(of: selectedID) { _ in scopeSelectedGroup() }
        .onChange(of: working) { if !$0 { scopeSelectedGroup() } }
        .onChange(of: model.remoteNeedsReload) { if $0 { stop() } }
        .sheet(item: $tool, onDismiss: { scopeSelectedGroup() }) { presentation in
            switch presentation.kind {
            case .lookup, .coverLookup:
                AlbumLookupView(model: model, targetIDs: presentation.trackIDs,
                                artworkOnly: presentation.kind == .coverLookup,
                                onApply: { markEdited(presentation.groupID) })
            case .edit:
                MagicTrackEditor(model: model, trackIDs: presentation.trackIDs,
                                 onEdit: { markEdited(presentation.groupID) })
            case .merge:
                LibraryMergeView(model: model, targetIDs: presentation.trackIDs, keepParentPresented: true,
                                 onMerge: { markEdited(presentation.groupID) })
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: "wand.and.stars").font(.system(size: 30)).foregroundStyle(.secondary)
                VStack(alignment: .leading, spacing: 5) {
                    Text("Magic").font(.title2.weight(.semibold))
                    Text(working ? "Identifying albums, matching tracks and finding artwork." : "Review each group, choose another match or make your own corrections here.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                if working { ProgressView().controlSize(.small) }
            }
            if working {
                ProgressView(value: Double(completed), total: Double(max(1, runGroupCount)))
                Text("\(completed) of \(runGroupCount) Album Groups Checked · \(runTrackCount) Tracks")
                    .font(.caption).foregroundStyle(.secondary)
            } else if !groups.isEmpty {
                HStack(spacing: 18) {
                    Label("\(preparedCount) Groups with Corrections", systemImage: "pencil.circle")
                    Label("\(groups.count - preparedCount) Awaiting Review", systemImage: "magnifyingglass")
                    if stopped { Text("Identification Stopped") }
                }.font(.callout).foregroundStyle(.secondary)
            }
        }.padding(22)
    }

    private var footer: some View {
        HStack(spacing: 12) {
            Text(working ? "This run applies no fixes until identification finishes." : (model.isRemote ? "All corrections are drafts. Save Changes in the library updates betterPod." : "All corrections are drafts. Save Changes in the library writes them to your files."))
                .font(.caption).foregroundStyle(.secondary)
            Spacer()
            if working {
                Button("Stop") { stop() }.keyboardShortcut(.cancelAction)
            } else {
                Button("Retry All") { retryAll() }.disabled(!canWork || sourceIDs.isEmpty)
                Button("Done") { dismiss() }
                    .buttonStyle(AppleProminentButtonStyle()).keyboardShortcut(.defaultAction)
            }
        }.padding(18)
    }

    private var groupList: some View {
        List(groups, selection: $selectedID) { group in
            VStack(alignment: .leading, spacing: 7) {
                Text(groupTitle(group)).fontWeight(.medium).lineLimit(2)
                Text(groupArtist(group)).font(.callout).foregroundStyle(.secondary).lineLimit(1)
                Label(status(group.id), systemImage: statusIcon(group.id))
                    .font(.caption).foregroundStyle(.secondary)
                Text("\(group.tracks.count) \(group.tracks.count == 1 ? "Track" : "Tracks")")
                    .font(.caption).foregroundStyle(.secondary)
            }.padding(.vertical, 6).tag(group.id)
        }.listStyle(.inset)
    }

    @ViewBuilder private var resultPreview: some View {
        if let group = selectedGroup {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    currentDraft(group)
                    if model.remoteNeedsReload {
                        Label("Reconnect and refresh betterPod before making more corrections.", systemImage: "wifi.exclamationmark")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                    if let error = model.error {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .font(.callout).foregroundStyle(.red)
                    }
                    if !working { correctionTools(group) }
                    identification(group)
                    Divider()
                    currentTracks(group)
                }.padding(22)
            }
        } else {
            VStack(spacing: 14) {
                Image(systemName: "wand.and.stars").font(.system(size: 38)).foregroundStyle(.tertiary)
                Text("Select Music, Then Let Magic Work").font(.headline)
                Text("Clear matches are prepared automatically. You can search again, edit tags, choose artwork or merge tracks here.")
                    .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center).frame(maxWidth: 360)
            }.padding(30).frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func currentDraft(_ group: MagicLibraryGroup) -> some View {
        let rows = currentRows(group)
        return HStack(alignment: .top, spacing: 18) {
            LibraryCover(data: rows.first?.cover).frame(width: 125, height: 125)
            VStack(alignment: .leading, spacing: 8) {
                Text("Current Draft").font(.caption).foregroundStyle(.secondary)
                Text(groupTitle(group)).font(.title3.weight(.semibold))
                Text(groupArtist(group)).foregroundStyle(.secondary)
                if let year = common(rows.map { $0[.year] }), !year.isEmpty {
                    Text(year).font(.callout).foregroundStyle(.secondary)
                }
                if rows.contains(where: \.dirty) {
                    Label("Unsaved Corrections", systemImage: "pencil.circle").font(.callout)
                } else if manuallyEdited.contains(group.id) || applied.contains(group.id) {
                    Label("Reviewed", systemImage: "checkmark.circle").font(.callout)
                }
                if rows.dropFirst().contains(where: { $0.cover != rows.first?.cover }) {
                    Text("These tracks have different covers. The first track is shown.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if model.artworkLoading { ProgressView().controlSize(.small) }
            }
        }
    }

    private func correctionTools(_ group: MagicLibraryGroup) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Correct This Group").font(.headline)
            Text("Tools affect only these \(group.tracks.count) tracks. Choose individual songs in Edit Tracks.")
                .font(.caption).foregroundStyle(.secondary)
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 8) { primaryTools(group) }
                VStack(alignment: .leading, spacing: 8) { primaryTools(group) }
            }
            HStack(spacing: 12) {
                Button("Merge into Album…") { present(.merge, for: group) }
                    .disabled(!canWork || !model.canMergeSelection)
                Button("Retry This Group") { retry(group) }.disabled(!canWork)
            }.controlSize(.small)
        }
    }

    @ViewBuilder private func primaryTools(_ group: MagicLibraryGroup) -> some View {
        Button { present(.lookup, for: group) } label: { Label("Find Album…", systemImage: "magnifyingglass") }
            .buttonStyle(AppleToolbarButtonStyle()).disabled(!canWork)
        Button { present(.edit, for: group) } label: { Label("Edit Tracks…", systemImage: "slider.horizontal.3") }
            .buttonStyle(AppleToolbarButtonStyle()).disabled(!canWork)
        Menu {
            Button("Find Cover…") { present(.coverLookup, for: group) }
            Button("Choose Image…") { chooseArtwork(group) }
            Button("Remove Cover") { removeArtwork(group) }
        } label: { Label("Artwork", systemImage: "photo") }
        .menuStyle(.borderlessButton).fixedSize().disabled(!canWork)
    }

    @ViewBuilder private func identification(_ group: MagicLibraryGroup) -> some View {
        Divider()
        if let outcome = outcomes[group.id] {
            VStack(alignment: .leading, spacing: 10) {
                Text("Last Identification").font(.headline)
                if let details = outcome.details {
                    Text("\(details.title) — \(details.artist)").font(.callout.weight(.medium))
                }
                Text(outcome.reason).font(.callout).foregroundStyle(.secondary)
                if let release = outcome.release {
                    Link("View on \(release.provider.name)", destination: release.sourceURL).font(.caption)
                }
                if let warning = outcome.details?.coverWarning { Text(warning).font(.caption).foregroundStyle(.secondary) }
                if !outcome.warnings.isEmpty {
                    DisclosureGroup("Service Details") {
                        ForEach(Array(outcome.warnings.enumerated()), id: \.offset) { _, warning in
                            Text(warning).font(.caption).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }.font(.caption)
                }
                if !outcome.safeToApply && !manuallyEdited.contains(group.id) {
                    Text("No automatic corrections were made. Find another album or edit these tracks using the tools above.")
                        .font(.callout).foregroundStyle(.secondary)
                }
            }
        } else if let error = errors[group.id] {
            Label("Couldn’t Identify This Group", systemImage: "exclamationmark.magnifyingglass").font(.headline)
            Text(error).font(.callout).foregroundStyle(.secondary)
            Text("You can search manually, edit tags or choose artwork above.").font(.callout).foregroundStyle(.secondary)
        } else {
            Label(working ? "Identifying Album…" : "Not Checked", systemImage: "opticaldisc").font(.headline)
            Text(group.summary).font(.callout).foregroundStyle(.secondary)
        }
    }

    private func currentTracks(_ group: MagicLibraryGroup) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Current Tracks").font(.headline)
            ForEach(currentRows(group)) { row in
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline) {
                        Text([row[.disc], row[.track]].filter { !$0.isEmpty }.joined(separator: "."))
                            .font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                        Text(row[.title].isEmpty ? row.original.fileName : row[.title]).font(.callout)
                        Spacer()
                        if row.dirty { LibraryChangeBadge(state: .modified, compact: true) }
                    }
                    Text(row.original.fileName).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    if !row[.artist].isEmpty { Text(row[.artist]).font(.caption).foregroundStyle(.secondary) }
                    Divider().padding(.top, 4)
                }
            }
        }
    }

    private func currentRows(_ group: MagicLibraryGroup) -> [LibraryDraft] { model.orderedTargets(Set(group.tracks.map(\.id))) }
    private func common(_ values: [String]) -> String? {
        guard let first = values.first, values.allSatisfy({ $0 == first }) else { return nil }
        return first
    }
    private func groupTitle(_ group: MagicLibraryGroup) -> String {
        let values = currentRows(group).map(\.effectiveAlbum)
        if let title = common(values), !AlbumLookupMatching.placeholder(title) { return title }
        if Set(values.filter { !AlbumLookupMatching.placeholder($0) }).count > 1 { return "Multiple Albums" }
        return group.query.inferred.album.isEmpty ? "Unidentified Tracks" : group.query.inferred.album
    }
    private func groupArtist(_ group: MagicLibraryGroup) -> String {
        let rows = currentRows(group)
        if let artist = common(rows.map(\.effectiveAlbumArtist)), !artist.isEmpty { return artist }
        if let artist = common(rows.map { $0[.artist] }), !artist.isEmpty { return artist }
        let artists = Set(rows.map { $0[.artist] }.filter { !AlbumLookupMatching.placeholder($0) })
        if artists.count > 1 { return "Various Artists" }
        return group.query.inferred.artist.isEmpty ? "Unknown Artist" : group.query.inferred.artist
    }
    private func status(_ id: String) -> String {
        if manuallyEdited.contains(id) { return "Edited Manually" }
        if applied.contains(id) { return "Prepared" }
        if let outcome = outcomes[id] { return outcome.safeToApply ? "Matched" : "Needs Review" }
        if errors[id] != nil { return "Unavailable" }
        return working ? "Checking…" : "Not Checked"
    }
    private func statusIcon(_ id: String) -> String {
        if manuallyEdited.contains(id) { return "pencil.circle" }
        if applied.contains(id) { return "checkmark.circle" }
        if let outcome = outcomes[id] { return outcome.safeToApply ? "checkmark" : "magnifyingglass" }
        return errors[id] == nil ? "ellipsis" : "exclamationmark.circle"
    }
    private func scopeSelectedGroup() {
        guard canWork, let group = selectedGroup else { return }
        model.selectMagicTargets(Set(group.tracks.map(\.id)))
    }
    private func present(_ kind: MagicToolPresentation.Kind, for group: MagicLibraryGroup) {
        guard canWork else { return }
        let ids = Set(group.tracks.map(\.id))
        model.selectMagicTargets(ids)
        if kind == .merge && !model.canMergeSelection { return }
        tool = MagicToolPresentation(groupID: group.id, trackIDs: ids, kind: kind)
    }
    private func markEdited(_ groupID: String) { manuallyEdited.insert(groupID) }
    private func chooseArtwork(_ group: MagicLibraryGroup) {
        guard canWork else { return }
        model.selectMagicTargets(Set(group.tracks.map(\.id)))
        let before = currentRows(group).map(\.artwork)
        model.chooseArtwork()
        if currentRows(group).map(\.artwork) != before { markEdited(group.id) }
    }
    private func removeArtwork(_ group: MagicLibraryGroup) {
        guard canWork else { return }
        model.selectMagicTargets(Set(group.tracks.map(\.id)))
        model.setArtwork(.remove)
        markEdited(group.id)
    }
    private func stop() {
        request?.cancel()
        runID = UUID()
        if ownsBusyState { model.busy = false; ownsBusyState = false }
        if working { stopped = true }
        working = false
    }
    private func inputs(for ids: Set<String>) -> [MagicLibraryTrack] {
        model.orderedTargets(ids).filter { $0.original.writable }.map { row in
            MagicLibraryTrack(id: row.id, fileName: row.original.fileName, title: row[.title], artist: row[.artist],
                              album: row.effectiveAlbum, albumArtist: row.effectiveAlbumArtist, disc: Int(row[.disc]) ?? 0,
                              track: Int(row[.track]) ?? 0,
                              folderName: URL(fileURLWithPath: row.id).deletingLastPathComponent().lastPathComponent)
        }
    }
    private func retryAll() {
        guard canWork else { return }
        groups = MagicLibraryService.group(inputs(for: sourceIDs))
        selectedID = groups.first?.id
        outcomes = [:]
        errors = [:]
        applied = []
        manuallyEdited.formIntersection(Set(groups.map(\.id)))
        run(groups)
    }
    private func retry(_ group: MagicLibraryGroup) {
        guard canWork else { return }
        let tracks = inputs(for: Set(group.tracks.map(\.id)))
        guard !tracks.isEmpty else { return }
        let rows = currentRows(group)
        let albumArtist = common(rows.map(\.effectiveAlbumArtist)) ?? ""
        let artist = albumArtist.isEmpty ? (common(rows.map { $0[.artist] }) ?? "") : albumArtist
        let query = AlbumLookupQuery(artist: artist,
                                     album: common(rows.map(\.effectiveAlbum)) ?? "", folderName: group.query.folderName,
                                     tracks: tracks.map { AlbumLookupLocalTrack(fileName: $0.fileName, title: $0.title, disc: $0.disc, track: $0.track) })
        let refreshed = MagicLibraryGroup(id: group.id, tracks: tracks, query: query, summary: query.summary)
        if let index = groups.firstIndex(where: { $0.id == group.id }) { groups[index] = refreshed }
        outcomes.removeValue(forKey: group.id)
        errors.removeValue(forKey: group.id)
        applied.remove(group.id)
        run([refreshed])
    }
    private func run(_ jobs: [MagicLibraryGroup]) {
        request?.cancel()
        guard !jobs.isEmpty else { return }
        completed = 0
        runGroupCount = jobs.count
        runTrackCount = jobs.reduce(0) { $0 + $1.tracks.count }
        stopped = false
        working = true
        ownsBusyState = true
        model.busy = true
        let token = UUID()
        runID = token
        request = Task { @MainActor in
            await withTaskGroup(of: MagicJobResult.self) { queue in
                var next = 0
                func submit(_ group: MagicLibraryGroup) {
                    queue.addTask {
                        do {
                            let service = MagicLibraryService()
                            let result = try await service.identify(group: group)
                            return MagicJobResult(id: group.id, outcome: result, error: nil)
                        } catch is CancellationError {
                            return MagicJobResult(id: group.id, outcome: nil, error: nil)
                        } catch {
                            return MagicJobResult(id: group.id, outcome: nil, error: error.localizedDescription)
                        }
                    }
                }
                for _ in 0..<min(2, jobs.count) { submit(jobs[next]); next += 1 }
                while let result = await queue.next() {
                    guard !Task.isCancelled, runID == token else { queue.cancelAll(); return }
                    if let outcome = result.outcome { outcomes[result.id] = outcome }
                    if let error = result.error { errors[result.id] = error }
                    completed += 1
                    if next < jobs.count { submit(jobs[next]); next += 1 }
                }
            }
            guard !Task.isCancelled, runID == token else { return }
            model.busy = false
            ownsBusyState = false
            for group in jobs {
                guard let outcome = outcomes[group.id], outcome.safeToApply, let details = outcome.details else { continue }
                model.applyMagicResult(details, to: Set(group.tracks.map(\.id)), matches: outcome.trackMatches)
                applied.insert(group.id)
            }
            working = false
            model.message = "Magic finished. Review the current drafts, use the group’s correction tools if needed, then save changes."
        }
    }
}

private struct MagicTrackEditor: View {
    @ObservedObject var model: LibraryModel
    let trackIDs: Set<String>
    var onEdit: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var selection = Set<String>()

    private var rows: [LibraryDraft] { model.orderedTargets(trackIDs) }
    private var editableRows: [LibraryDraft] { rows.filter { selection.contains($0.id) && $0.original.writable } }
    private var canEdit: Bool { !model.busy && !model.remoteNeedsReload && !editableRows.isEmpty }

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Edit Tracks").font(.title2.weight(.semibold))
                    Text("Select one song or several. Every field edits only the selected tracks in this group.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Select All") { selection = Set(rows.map(\.id)); scope() }.disabled(model.busy)
            }.padding(22)
            Divider()
            HSplitView {
                List(rows, selection: $selection) { row in
                    VStack(alignment: .leading, spacing: 5) {
                        Text(row[.title].isEmpty ? row.original.fileName : row[.title]).fontWeight(.medium).lineLimit(2)
                        Text(row.original.fileName).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        if row.dirty { LibraryChangeBadge(state: .modified, compact: true) }
                    }.padding(.vertical, 5).tag(row.id)
                }.listStyle(.inset).frame(minWidth: 220, idealWidth: 270, maxWidth: 340).disabled(model.busy)
                editor.frame(minWidth: 320, maxWidth: .infinity, maxHeight: .infinity)
            }
            Divider()
            HStack {
                Text("\(selection.count) Tracks Selected · Corrections Stay in the Draft")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Done") { dismiss() }.buttonStyle(AppleProminentButtonStyle()).keyboardShortcut(.defaultAction)
            }.padding(18)
        }
        .frame(minWidth: 760, idealWidth: 860, minHeight: 580, idealHeight: 680)
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear { selection = Set(rows.map(\.id)); scope() }
        .onChange(of: selection) { _ in scope() }
        .onExitCommand { dismiss() }
    }

    private var editor: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Text(selection.count == 1 ? "Track Details" : "\(selection.count) Tracks Selected").font(.headline)
                if selection.isEmpty {
                    Text("Select tracks to edit their metadata.").foregroundStyle(.secondary)
                } else {
                    if model.isRemote, editableRows.contains(where: { $0.original.hasAlbumGroupingOverride }) {
                        Text("Some tracks use album information set on betterPod. Editing Album or Album Artist also updates that grouping.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    HStack(alignment: .top, spacing: 14) {
                        LibraryCover(data: editableRows.first?.cover).frame(width: 80, height: 80)
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Artwork for Selected Tracks").font(.callout.weight(.medium))
                            Button("Choose Image…") {
                                guard canEdit else { return }
                                scope()
                                let before = editableRows.map(\.artwork)
                                model.chooseArtwork()
                                if editableRows.map(\.artwork) != before { onEdit() }
                            }
                            Button("Remove Cover") {
                                guard canEdit else { return }
                                scope(); model.setArtwork(.remove); onEdit()
                            }
                        }
                    }
                    Divider()
                    ForEach(LibraryField.allCases) { field in
                        VStack(alignment: .leading, spacing: 5) {
                            HStack {
                                Text(field.label).font(.callout).foregroundStyle(.secondary)
                                if editableRows.contains(where: { $0.changeState(for: field) == .modified }) {
                                    LibraryChangeBadge(state: .modified, compact: true)
                                }
                                Spacer()
                                if mixed(field) {
                                    Button("Clear") { set(field, to: "") }.font(.caption).buttonStyle(.link)
                                }
                            }
                            TextField(mixed(field) ? "Multiple Values" : field.label,
                                      text: Binding(get: { value(field) }, set: { set(field, to: $0) }))
                                .textFieldStyle(.roundedBorder).accessibilityLabel(field.label)
                        }
                    }
                    if editableRows.contains(where: \.dirty) {
                        Button("Revert Selected Unsaved Changes") {
                            guard canEdit else { return }
                            scope(); model.revertSelection(); onEdit()
                        }
                    }
                }
            }.padding(22).disabled(!canEdit)
        }
    }
    private func value(_ field: LibraryField) -> String {
        guard let first = editableRows.first, !mixed(field) else { return "" }
        return first[field]
    }
    private func mixed(_ field: LibraryField) -> Bool { Set(editableRows.map { $0[field] }).count > 1 }
    private func scope() {
        guard !model.busy, !model.remoteNeedsReload else { return }
        model.selectMagicTargets(selection.intersection(trackIDs))
    }
    private func set(_ field: LibraryField, to value: String) {
        guard canEdit else { return }
        scope(); model.set(field, to: value); onEdit()
    }
}
