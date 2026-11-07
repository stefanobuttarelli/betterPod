import AppKit
import SwiftUI

// Native materials belong to navigation and floating controls. Tables, artwork
// and editable content keep their standard, readable system surfaces.
private struct AppleGlass: ViewModifier {
    let cornerRadius: CGFloat
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.colorSchemeContrast) private var contrast

    @ViewBuilder func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        if reduceTransparency || contrast == .increased {
            content
                .background(Color(nsColor: .windowBackgroundColor), in: shape)
                .overlay(shape.strokeBorder(Color(nsColor: .separatorColor), lineWidth: 1))
        } else if #available(macOS 26.0, *) {
            content
                .glassEffect(.regular, in: shape)
                .transaction { if reduceMotion { $0.animation = nil } }
        } else {
            content.background(.regularMaterial, in: shape)
        }
    }
}

extension View {
    func appleGlass(cornerRadius: CGFloat = 12) -> some View {
        modifier(AppleGlass(cornerRadius: cornerRadius))
    }

    @ViewBuilder func appleBackgroundExtension() -> some View {
        if #available(macOS 26.0, *) {
            backgroundExtensionEffect()
        } else {
            self
        }
    }
}

struct AppleToolbarButtonStyle: PrimitiveButtonStyle {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.colorSchemeContrast) private var contrast

    @ViewBuilder func makeBody(configuration: Configuration) -> some View {
        if #available(macOS 26.0, *), !reduceTransparency, contrast != .increased {
            Button(configuration).buttonStyle(.glass)
        } else {
            Button(configuration).buttonStyle(.bordered)
        }
    }
}

struct AppleProminentButtonStyle: PrimitiveButtonStyle {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.colorSchemeContrast) private var contrast

    @ViewBuilder func makeBody(configuration: Configuration) -> some View {
        if #available(macOS 26.0, *), !reduceTransparency, contrast != .increased {
            Button(configuration).buttonStyle(.glassProminent)
        } else {
            Button(configuration).buttonStyle(.borderedProminent)
        }
    }
}
