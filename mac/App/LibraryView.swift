import AppKit
import SwiftUI
import UniformTypeIdentifiers

/// The track table, its filter bar and status bar, and the inspector. The
/// window's header holds Open, Find Album and Save; less common commands are
/// in the context menu and the Library menu.
struct LibraryView: View {
    @ObservedObject var model: LibraryModel
    @Environment(\.undoManager) private var undoManager
    @State private var targeted = false
    private var rows: [LibraryDraft] { model.visibleTracks }
    private var canEdit: Bool { model.editableCount > 0 && !model.busy && !model.remoteNeedsReload }

    var body: some View {
        VStack(spacing: 0) {
            if let error = model.error {
                PodBanner(text: error, isError: true) { model.error = nil; model.message = nil }
            } else if let message = model.message {
                PodBanner(text: message) { model.error = nil; model.message = nil }
            }
            if model.tracks.isEmpty {
                emptyState
            } else {
                HStack(spacing: 0) {
                    VStack(spacing: 0) {
                        filterBar
                        trackTable
                    }
                    .frame(minWidth: 480, maxWidth: .infinity, maxHeight: .infinity)
                    Pod.hairline.frame(width: 1)
                    inspector.frame(width: 300)
                }
            }
            statusBar
        }
        .background(Color(nsColor: .controlBackgroundColor))
        .overlay {
            if targeted {
                RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(Pod.accent, style: StrokeStyle(lineWidth: 3, dash: [10, 6]))
                    .padding(6).allowsHitTesting(false)
            }
        }
        .onDrop(of: [.fileURL], isTargeted: $targeted) { providers in
            guard !model.busy, !model.isRemote else { return false }
            loadURLs(providers) { model.open($0) }
            return true
        }
        .sheet(isPresented: $model.showingLookup) {
            if let group = model.currentLookupGroup {
                AlbumLookupView(model: model, targetIDs: group, step: model.lookupIndex + 1, steps: model.lookupGroups.count)
                    .id(model.lookupIndex)
            }
        }
        .sheet(isPresented: $model.showingMerge) {
            LibraryMergeView(model: model)
        }
        .onChange(of: model.selection) { _ in model.requestArtwork() }
        .onChange(of: model.changesOnly) { _ in
            model.selection.formIntersection(Set(rows.map(\.id)))
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "opticaldisc")
                .font(.system(size: 54, weight: .light))
                .foregroundStyle(Pod.secondaryText.opacity(0.7))
            VStack(spacing: 6) {
                Text(model.isRemote ? "No Music on betterPod" : "Organize Your Music")
                    .font(.system(size: 19, weight: .bold))
                Text(model.isRemote ? "Add music and rescan on betterPod, then refresh the library."
                     : "Open an album folder, then use Find Album to fill in its cover,\ndetails and track names. Nothing changes until you save.")
                    .font(.callout).foregroundStyle(Pod.secondaryText).multilineTextAlignment(.center)
            }
            if !model.isRemote {
                HStack(spacing: 10) {
                    Button("Open Album Folder…") { model.choose() }
                        .buttonStyle(PodButtonStyle(kind: .primary))
                        .disabled(model.busy)
                    if model.canUndoRemoval {
                        Button("Undo Library Removal") { model.undoRemoval() }.buttonStyle(PodButtonStyle())
                    }
                }
                Text("Or drag a folder here. Removing it later keeps your files on disk.")
                    .font(.caption).foregroundStyle(Pod.secondaryText)
            }
            if model.busy { ProgressView("Reading Audio Files…").controlSize(.small) }
            Text("MP3 · FLAC · Ogg Vorbis · M4A. WAV and AIFF are read-only.")
                .font(.caption2).foregroundStyle(Pod.secondaryText)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity).padding(32)
        .background(Color(nsColor: .textBackgroundColor))
    }

    private var filterBar: some View {
        HStack(spacing: 10) {
            Picker("Album", selection: Binding(get: { model.albumFilter }, set: {
                model.albumFilter = $0
                model.filterChanged()
            })) {
                Text("All Albums (\(model.tracks.count) tracks)").tag("")
                Divider()
                ForEach(model.albums) { album in
                    Text("\(album.title) — \(album.artist) (\(album.count))").tag(album.id)
                }
            }
            .labelsHidden().fixedSize()
            .controlSize(.small)
            Spacer(minLength: 4)
            Button { model.changesOnly.toggle() } label: { Label("Changes", systemImage: "pencil.line") }
                .buttonStyle(PodButtonStyle(selected: model.changesOnly, compact: true))
                .help("Show only tracks with edited or saved tags or covers")
                .accessibilityAddTraits(model.changesOnly ? .isSelected : [])
        }
        .padding(.horizontal, 12).padding(.vertical, 7)
        .background(Pod.strip)
        .overlay(alignment: .bottom) { Pod.hairline.frame(height: 1) }
    }

    private var trackTable: some View {
        Table(rows, selection: $model.selection, sortOrder: $model.sortOrder) {
            TableColumn("#", value: \.sortTrack) { row in
                LibraryMetadataCell(row: row, field: .track, selected: model.selection.contains(row.id))
            }.width(34)
            TableColumn("Cover", value: \.sortArtwork) { row in
                LibraryCover(data: row.cover, cornerRadius: 3)
                    .frame(width: 30, height: 30)
                    .overlay(alignment: .bottomTrailing) {
                        if row.artworkChangeState != .unchanged {
                            Circle().fill(row.artworkChangeState == .modified ? Pod.edited : Pod.saved)
                                .frame(width: 9, height: 9)
                                .overlay(Circle().strokeBorder(Color.white, lineWidth: 1.5))
                                .offset(x: 3, y: 3)
                        }
                    }
                    .help(artworkHelp(row))
                    .accessibilityLabel(artworkHelp(row))
            }.width(40)
            TableColumn("Title", value: \.sortTitle) { row in
                VStack(alignment: .leading, spacing: 2) {
                    LibraryMetadataCell(row: row, field: .title, selected: model.selection.contains(row.id), fallback: row.original.fileName)
                    Text(row.original.fileName).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                }.padding(.vertical, 3)
            }.width(min: 140, ideal: 230)
            TableColumn("Artist", value: \.sortArtist) { row in
                LibraryMetadataCell(row: row, field: .artist, selected: model.selection.contains(row.id))
            }.width(min: 80, ideal: 130)
            TableColumn("Album", value: \.sortAlbum) { row in
                LibraryMetadataCell(row: row, field: .album, selected: model.selection.contains(row.id), display: row.sortAlbum)
            }.width(min: 90, ideal: 150)
            TableColumn("Changes", value: \.sortChanges) { row in
                HStack(spacing: 5) {
                    if row.dirty {
                        LibraryChangeBadge(state: .modified, selected: model.selection.contains(row.id))
                    } else if row.hasRecordedChanges {
                        LibraryChangeBadge(state: .saved, selected: model.selection.contains(row.id))
                    }
                }.help(changeSummary(row))
            }.width(min: 64, ideal: 80)
        }
        .tableStyle(.inset(alternatesRowBackgrounds: true))
        .disabled(model.busy)
        .onDeleteCommand {
            if !model.isRemote { model.removeSelection(undoManager: undoManager) }
        }
        .contextMenu(forSelectionType: String.self) { ids in
            Button("Find Album…") {
                model.selection = ids
                model.beginLookup()
            }.disabled(!canEdit(ids))
            Button("Choose Cover Image…") {
                model.selection = ids
                model.chooseArtwork()
            }.disabled(!canEdit(ids))
            Button("Merge into Album…") {
                model.selection = ids
                model.beginMerge()
            }.disabled(!canEdit(ids) || !model.canJoinAlbums)
            if !model.isRemote {
                Divider()
                Button("Remove from Library") {
                    model.selection = ids
                    model.removeSelection(undoManager: undoManager)
                }.disabled(model.busy || ids.isEmpty)
            }
        }
    }

    // MARK: Inspector

    private var inspector: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if model.selection.isEmpty {
                    PodCard {
                        Text("Select a track to see its details. Hold ⌘ or ⇧ to select several tracks, or ⌘A for all of them.")
                            .font(.callout).foregroundStyle(Pod.secondaryText)
                    }
                } else {
                    artworkCard
                    detailsCard
                    if let row = model.selectedTracks.first, model.selection.count == 1 { fileCard(row) }
                }
            }
            .padding(12)
        }
        .background(Pod.panel)
    }

    private var artworkCard: some View {
        PodCard(title: "Cover") {
            HStack(alignment: .top, spacing: 12) {
                CoverDropTarget(model: model, canEdit: canEdit, state: selectedArtworkState)
                    .frame(width: 112, height: 112)
                VStack(alignment: .leading, spacing: 7) {
                    LibraryChangeBadge(state: selectedArtworkState)
                    if model.artworkLoading { ProgressView().controlSize(.small) }
                    Button("Find Album…") { model.beginLookup() }
                        .buttonStyle(PodButtonStyle(compact: true)).disabled(!canEdit)
                    Button("Choose Image…") { model.chooseArtwork() }
                        .buttonStyle(PodButtonStyle(compact: true)).disabled(!canEdit)
                    Button("Remove Cover") { model.setArtwork(.remove) }
                        .buttonStyle(.link).font(.caption).disabled(!canEdit)
                }
            }
            if model.isRemote && model.selection.count > 1 {
                Text("Preview of the first selected track. Changes apply to every selected editable track.")
                    .font(.caption).foregroundStyle(Pod.secondaryText)
            } else if let first = model.selectedTracks.first, model.selectedTracks.dropFirst().contains(where: { $0.cover != first.cover }) {
                Text("The selected tracks have different covers.").font(.caption).foregroundStyle(Pod.secondaryText)
            } else {
                Text("Drop a JPEG or PNG on the cover to use it.").font(.caption).foregroundStyle(Pod.secondaryText)
            }
        }
    }

    private var detailsCard: some View {
        PodCard(title: model.selection.count == 1 ? "Details" : "\(model.selection.count) Tracks") {
            if selectedAlbumGroupingState != .unchanged {
                HStack(spacing: 6) {
                    Text("Album grouping").font(.caption).foregroundStyle(Pod.secondaryText)
                    LibraryChangeBadge(state: selectedAlbumGroupingState, compact: true)
                }
                .help("The selected tracks' album membership is included when saving to betterPod.")
            }
            VStack(alignment: .leading, spacing: 9) {
                field(.title)
                field(.artist)
                field(.album)
                field(.albumArtist)
                HStack(alignment: .top, spacing: 8) { field(.genre); field(.year, narrow: true).frame(width: 84) }
                HStack(alignment: .top, spacing: 8) { field(.disc, narrow: true); field(.track, narrow: true); field(.trackTotal, narrow: true) }
            }
            .disabled(!canEdit)
            if model.editableCount < model.selection.count {
                Label("\(model.selection.count - model.editableCount) selected files are read-only. Edits apply to the others.", systemImage: "lock")
                    .font(.caption).foregroundStyle(Pod.secondaryText)
            }
            if model.selectedTracks.contains(where: \.dirty) {
                Button("Revert Unsaved Changes") { model.revertSelection() }
                    .buttonStyle(PodButtonStyle(compact: true)).disabled(model.busy)
            }
        }
    }

    /// A tag field. Narrow numeric fields have no room for Clear; typing sets every selected track.
    private func field(_ field: LibraryField, narrow: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 5) {
                Text(field == .trackTotal ? "Of" : field.label).font(.caption).foregroundStyle(Pod.secondaryText).lineLimit(1)
                    .accessibilityLabel(field.label)
                LibraryChangeBadge(state: selectedState(field), compact: true, iconOnly: true)
                Spacer(minLength: 0)
                if model.isMixed(field) && !narrow {
                    Button("Clear") { model.set(field, to: "") }
                        .font(.caption2).buttonStyle(.link)
                        .accessibilityLabel("Clear \(field.label) for Selected Tracks")
                }
            }
            TextField(model.isMixed(field) ? (narrow ? "Mixed" : "Multiple Values") : "", text: Binding(get: { model.commonValue(field) }, set: { model.set(field, to: $0) }))
                .textFieldStyle(.roundedBorder)
                .controlSize(.small)
                .accessibilityLabel(field.label)
                .help(selectedHelp(field))
        }
    }

    private func fileCard(_ row: LibraryDraft) -> some View {
        PodCard(title: "File") {
            if let album = row.original.displayAlbum, album != row.original.album || !(row.original.albumGroup ?? "").isEmpty {
                Text("Album shown on betterPod: \(album). Editing Album or Album artist also updates its grouping.")
                    .font(.caption).foregroundStyle(Pod.secondaryText)
            }
            if let warning = row.original.warning { Text(warning).font(.caption).foregroundStyle(Pod.secondaryText) }
            Text("\(row.original.format.uppercased()) · \(humanBytes(row.original.size))")
                .font(.caption.weight(.medium))
            Text(row.id).font(.caption).foregroundStyle(Pod.secondaryText).textSelection(.enabled).lineLimit(3).truncationMode(.middle)
        }
    }

    private var statusBar: some View {
        HStack(spacing: 14) {
            Label(model.busy ? "Working…" : "\(rows.count) tracks · \(model.albums.count) albums", systemImage: model.isRemote ? "wifi" : "internaldrive")
            Spacer()
            if model.tracks.contains(where: \.hasRecordedChanges) {
                LibraryChangeBadge(state: .modified)
                LibraryChangeBadge(state: .saved)
            }
        }
        .font(.caption).foregroundStyle(Pod.secondaryText)
        .padding(.horizontal, 14)
        .frame(height: 24)
        .background(PodChrome(border: .top))
    }

    private var selectedArtworkState: LibraryChangeState {
        if model.selectedTracks.contains(where: { $0.artworkChangeState == .modified }) { return .modified }
        if model.selectedTracks.contains(where: { $0.artworkChangeState == .saved }) { return .saved }
        return .unchanged
    }
    private var selectedAlbumGroupingState: LibraryChangeState {
        if model.selectedTracks.contains(where: { $0.albumGroupingChangeState == .modified }) { return .modified }
        if model.selectedTracks.contains(where: { $0.albumGroupingChangeState == .saved }) { return .saved }
        return .unchanged
    }
    private func canEdit(_ ids: Set<String>) -> Bool {
        !model.busy && !model.remoteNeedsReload && model.tracks.contains { ids.contains($0.id) && $0.original.writable }
    }
    private func selectedState(_ field: LibraryField) -> LibraryChangeState {
        if model.selectedTracks.contains(where: { $0.changeState(for: field) == .modified }) { return .modified }
        if model.selectedTracks.contains(where: { $0.changeState(for: field) == .saved }) { return .saved }
        return .unchanged
    }
    private func selectedHelp(_ field: LibraryField) -> String {
        guard let row = model.selectedTracks.first, model.selection.count == 1 else { return "Changes apply to every selected editable track." }
        return metadataHelp(row, field)
    }
    private func changeSummary(_ row: LibraryDraft) -> String {
        let fields = row.changedFields.union(row.savedChangedFields)
        var names = LibraryField.allCases.filter(fields.contains).map(\.label)
        if row.albumGroupingChangeState != .unchanged { names.append("Album grouping") }
        if row.artworkChangeState != .unchanged { names.append("Cover") }
        return names.joined(separator: ", ")
    }
    private func artworkHelp(_ row: LibraryDraft) -> String {
        switch row.artworkChangeState {
        case .modified: return "Cover edited · Not yet saved"
        case .saved: return "New cover saved to this file"
        case .unchanged: return "Cover unchanged"
        }
    }
}

/// The inspector's cover. A JPEG or PNG dropped on it becomes the selected
/// tracks' cover.
private struct CoverDropTarget: View {
    @ObservedObject var model: LibraryModel
    let canEdit: Bool
    let state: LibraryChangeState
    @State private var targeted = false

    var body: some View {
        LibraryCover(data: model.selectedTracks.first?.cover, cornerRadius: 4)
            .shadow(color: .black.opacity(0.25), radius: 3, y: 2)
            .overlay {
                RoundedRectangle(cornerRadius: 4)
                    .strokeBorder(targeted ? Pod.accent : (state == .modified ? Pod.edited : (state == .saved ? Pod.saved : Pod.cardBorder)),
                                  lineWidth: targeted || state != .unchanged ? 2 : 1)
            }
            .onDrop(of: [.fileURL], isTargeted: $targeted) { providers in
                guard canEdit else { return false }
                loadURLs(providers) { urls in
                    if let url = urls.first { model.useArtwork(at: url) }
                }
                return true
            }
            .accessibilityLabel("Cover. Drop a JPEG or PNG image to replace it.")
    }
}

private func metadataHelp(_ row: LibraryDraft, _ field: LibraryField) -> String {
    switch row.changeState(for: field) {
    case .unchanged: return field.label + " unchanged"
    case .modified: return "\(field.label): \(row.previousValue(for: field).isEmpty ? "Empty" : row.previousValue(for: field)) → \(row[field].isEmpty ? "Empty" : row[field]) · Not yet saved"
    case .saved: return "\(field.label) updated and saved · Previously: \(row.previousValue(for: field).isEmpty ? "Empty" : row.previousValue(for: field))"
    }
}

private struct LibraryMetadataCell: View {
    let row: LibraryDraft
    let field: LibraryField
    var selected = false
    var fallback = ""
    var display: String? = nil
    private var state: LibraryChangeState { row.changeState(for: field) }
    var body: some View {
        HStack(spacing: 4) {
            Text((display ?? row[field]).isEmpty ? fallback : (display ?? row[field]))
                .fontWeight(state == .unchanged ? .regular : .medium).lineLimit(1)
            if state != .unchanged {
                Image(systemName: state == .modified ? "pencil" : "checkmark")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(selected ? Color.primary : (state == .modified ? Pod.edited : Pod.saved))
            }
        }
        .padding(.vertical, 2)
        .help(metadataHelp(row, field))
        .accessibilityElement(children: .combine)
        .accessibilityLabel(metadataHelp(row, field))
    }
}

/// "Edited" (not yet saved) in blue, "Saved" in green.
struct LibraryChangeBadge: View {
    let state: LibraryChangeState
    var compact = false
    var selected = false
    var iconOnly = false
    var body: some View {
        if state != .unchanged {
            let color = selected ? Color.primary : (state == .modified ? Pod.edited : Pod.saved)
            HStack(spacing: 3) {
                Image(systemName: state == .modified ? "pencil.circle.fill" : "checkmark.circle.fill").foregroundStyle(color)
                if !iconOnly { Text(state == .modified ? "Edited" : "Saved").foregroundStyle(selected ? .primary : .secondary) }
            }
            .font(compact ? .caption2 : .caption)
            .accessibilityElement(children: .combine)
            .accessibilityLabel(state == .modified ? "Edited, not yet saved" : "Saved")
        }
    }
}

struct LibraryCover: View {
    let data: Data?
    var cornerRadius: CGFloat = 6
    var body: some View {
        Group {
            if let data, let image = NSImage(data: data) {
                Image(nsImage: image).resizable().interpolation(.high).scaledToFit()
            } else {
                ZStack {
                    LinearGradient(colors: [Color(white: 0.62), Color(white: 0.42)], startPoint: .top, endPoint: .bottom)
                    Image(systemName: "music.note").font(.title2).foregroundStyle(.white.opacity(0.85))
                }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: cornerRadius))
    }
}
