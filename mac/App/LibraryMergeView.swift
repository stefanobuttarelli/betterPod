import SwiftUI

struct LibraryMergeView: View {
    @ObservedObject var model: LibraryModel
    var targetIDs: Set<String>? = nil
    var keepParentPresented = false
    var onMerge: (() -> Void)? = nil
    @Environment(\.dismiss) private var dismiss
    @State private var targetID: String?

    private var targets: [LocalAlbum] { model.mergeTargets }
    private var selectedTarget: LocalAlbum? { targets.first { $0.id == targetID } }
    private var sourceCount: Int { model.selectedAlbumIDs.count }
    private var readOnlyCount: Int { model.selectedTracks.filter { !$0.original.writable }.count }

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: "rectangle.stack.badge.plus")
                    .font(.system(size: 28, weight: .regular))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 5) {
                    Text("Merge into Album").font(.title2.weight(.semibold))
                    Text("\(model.selectedTracks.count) selected \(model.selectedTracks.count == 1 ? "track" : "tracks") from \(sourceCount) \(sourceCount == 1 ? "album" : "albums")")
                        .foregroundStyle(.secondary)
                }
            }

            if targets.isEmpty {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Open Another Album to Choose a Destination").font(.headline)
                    Text(model.isRemote ? "Load a betterPod library containing multiple albums, then select the tracks you want to bring together." : "Open a folder containing multiple albums, then select the tracks you want to bring together.")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, minHeight: 150, alignment: .leading)
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Choose the album these tracks belong to:").font(.headline)
                    List(targets, selection: $targetID) { album in
                        HStack(spacing: 12) {
                            Image(systemName: "square.stack").foregroundStyle(.secondary)
                                .font(.title3)
                            VStack(alignment: .leading, spacing: 3) {
                                Text(album.title).fontWeight(.medium)
                                Text(album.artist.isEmpty ? "Unknown Artist" : album.artist)
                                    .font(.callout).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Text("\(album.count) \(album.count == 1 ? "track" : "tracks")")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        .padding(.vertical, 5)
                        .tag(album.id)
                    }
                    .listStyle(.inset)
                    .frame(minHeight: 170, idealHeight: 225, maxHeight: 260)
                    .accessibilityLabel("Destination Album")
                }

                VStack(alignment: .leading, spacing: 9) {
                    Text("The selected tracks will use the destination’s album name and album artist. Their individual artists and covers stay as they are.")
                        .font(.callout).foregroundStyle(.secondary)
                    Text("A missing album artist is filled consistently for the destination album and incoming tracks, using its performer or Various Artists.")
                        .font(.callout).foregroundStyle(.secondary)
                    Label(model.isRemote ? "Review the highlighted drafts, then choose Save Changes to update betterPod." : "Review the highlighted edits, then choose Save Changes. Audio files stay in their folders.", systemImage: "pencil.circle")
                        .font(.callout).foregroundStyle(.secondary)
                    if readOnlyCount > 0 {
                        Label("\(readOnlyCount) selected \(readOnlyCount == 1 ? "file is" : "files are") read-only and will be skipped.", systemImage: "lock")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
            }

            if let error = model.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.callout).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
            }

            HStack(spacing: 12) {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Merge into Album") {
                    guard let target = selectedTarget else { return }
                    if let targetIDs { model.selectMagicTargets(targetIDs) }
                    model.mergeSelection(into: target.id, dismissPresentedEditors: !keepParentPresented)
                    if model.error == nil { onMerge?(); dismiss() }
                }
                .buttonStyle(AppleProminentButtonStyle())
                .keyboardShortcut(.defaultAction)
                .disabled(selectedTarget == nil || !model.canMergeSelection)
            }
        }
        .padding(24)
        .frame(width: 560)
        .onAppear {
            if let targetIDs { model.selectMagicTargets(targetIDs) }
            chooseDefault()
        }
        .onChange(of: targets.map(\.id)) { _ in
            if !targets.contains(where: { $0.id == targetID }) { chooseDefault() }
        }
        .onChange(of: targetID) { _ in model.error = nil }
    }

    private func chooseDefault() { targetID = targets.first?.id }
}
