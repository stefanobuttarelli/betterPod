import SwiftUI

/// betterPod over Wi-Fi: the connection form, then the player's library in
/// the same editor as the Mac's. Refresh and Disconnect sit in the header.
struct DeviceLibraryView: View {
    @ObservedObject var model: DeviceModel
    @ObservedObject var library: LibraryModel

    init(model: DeviceModel) {
        self.model = model
        self.library = model.library
    }

    var body: some View {
        VStack(spacing: 0) {
            if let error = model.error {
                PodBanner(text: error, isError: true) { model.error = nil }
            }
            if model.connected {
                if library.remoteSession?.supportsAlbumJoining != true {
                    PodBanner(text: "Update betterPod Rust to merge albums over Wi-Fi. Tags and covers can still be edited.")
                }
                if library.remoteNeedsReload {
                    PodBanner(text: "Reload the library before saving: choose Refresh, or disconnect and connect again.", isError: true)
                }
                LibraryView(model: library)
            } else {
                connectionForm
            }
        }
    }

    private var connectionForm: some View {
        VStack(spacing: 22) {
            Spacer()
            Image(systemName: "wifi").font(.system(size: 46, weight: .light)).foregroundStyle(Pod.accent)
            VStack(spacing: 6) {
                Text("Your betterPod library, on your Mac").font(.system(size: 19, weight: .bold))
                Text("On betterPod Rust, open Music → Wi-Fi Library and start a connection.\nKeep both on the same Wi-Fi network, then enter its eight-digit code.")
                    .font(.callout).foregroundStyle(Pod.secondaryText).multilineTextAlignment(.center)
            }
            PodCard {
                HStack {
                    PodSectionTitle("Nearby betterPods")
                    Spacer()
                    if model.discovering { ProgressView().controlSize(.small) }
                    Button("Find betterPod") { model.discover() }
                        .buttonStyle(PodButtonStyle(compact: true))
                        .disabled(model.discovering || model.connecting)
                }
                if !model.devices.isEmpty {
                    Picker("Device", selection: $model.selectedDevice) {
                        ForEach(model.devices) { device in Text("\(device.name) · \(device.host)").tag(device.id) }
                    }
                    .disabled(model.connecting)
                } else {
                    Text(model.discovering ? "Looking on your local network…" : "Start the connection on betterPod, then choose Find betterPod.")
                        .font(.callout).foregroundStyle(Pod.secondaryText)
                }
                HStack {
                    TextField("0000 0000", text: $model.code)
                        .textFieldStyle(.roundedBorder).font(.system(size: 24, weight: .semibold, design: .rounded).monospacedDigit())
                        .frame(width: 200).accessibilityLabel("Eight-digit connection code")
                        .onSubmit { model.connect() }
                        .disabled(model.connecting)
                    Spacer()
                    if model.connecting {
                        Button("Cancel") { model.disconnect(askToDiscard: false) }.buttonStyle(PodButtonStyle())
                    } else {
                        Button("Connect") { model.connect() }
                            .buttonStyle(PodButtonStyle(kind: .primary))
                            .disabled(!model.validCode)
                    }
                }
                DisclosureGroup("Enter a local address", isExpanded: $model.useManualAddress) {
                    HStack {
                        TextField("IP address", text: $model.manualHost).textFieldStyle(.roundedBorder)
                        TextField("Port", text: $model.manualPort).textFieldStyle(.roundedBorder).frame(width: 80)
                    }.padding(.top, 8).disabled(model.connecting)
                    Text("Use the address and port shown on betterPod if your network blocks discovery.")
                        .font(.caption).foregroundStyle(Pod.secondaryText).padding(.top, 4)
                }
                .font(.callout)
                .onChange(of: model.useManualAddress) { expanded in
                    if !expanded { model.manualHost = ""; model.manualPort = "" }
                }
                if model.connecting {
                    HStack {
                        ProgressView().controlSize(.small)
                        Text(model.progress).font(.callout).foregroundStyle(Pod.secondaryText)
                    }
                }
            }
            .frame(width: 500)
            Text("Changes are saved to betterPod only when you choose Save Changes.\nThe connection stays inside your local network.")
                .font(.caption).foregroundStyle(Pod.secondaryText).multilineTextAlignment(.center)
            Spacer()
        }
        .padding(30)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Pod.panel)
    }
}
