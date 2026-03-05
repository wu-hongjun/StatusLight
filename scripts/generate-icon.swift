#!/usr/bin/env swift
//
// generate-icon.swift — Create OpenSlicky.iconset PNGs programmatically.
//
// Usage: swift generate-icon.swift <output-iconset-dir>
//   Then: iconutil -c icns <output-iconset-dir> -o AppIcon.icns

import AppKit

let iconsetDir = CommandLine.arguments[1]
try! FileManager.default.createDirectory(
    atPath: iconsetDir, withIntermediateDirectories: true
)

let sizes: [(Int, String)] = [
    (16, "icon_16x16.png"),
    (32, "icon_16x16@2x.png"),
    (32, "icon_32x32.png"),
    (64, "icon_32x32@2x.png"),
    (128, "icon_128x128.png"),
    (256, "icon_128x128@2x.png"),
    (256, "icon_256x256.png"),
    (512, "icon_256x256@2x.png"),
    (512, "icon_512x512.png"),
    (1024, "icon_512x512@2x.png"),
]

for (px, name) in sizes {
    let s = CGFloat(px)
    let img = NSImage(size: NSSize(width: s, height: s))
    img.lockFocus()

    // Dark rounded-rect background
    let inset = s * 0.1
    let radius = s * 0.22
    let bgRect = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    NSColor(red: 0.11, green: 0.11, blue: 0.13, alpha: 1.0).setFill()
    NSBezierPath(roundedRect: bgRect, xRadius: radius, yRadius: radius).fill()

    // Soft glow behind the light
    let glowSize = s * 0.55
    let centerY = s * 0.52
    let glowRect = NSRect(
        x: (s - glowSize) / 2,
        y: centerY - glowSize / 2,
        width: glowSize,
        height: glowSize
    )
    NSColor(red: 0.2, green: 0.85, blue: 0.4, alpha: 0.25).setFill()
    NSBezierPath(ovalIn: glowRect).fill()

    // Main light: bright green circle
    let lightSize = s * 0.35
    let lightRect = NSRect(
        x: (s - lightSize) / 2,
        y: centerY - lightSize / 2,
        width: lightSize,
        height: lightSize
    )
    NSColor(red: 0.2, green: 0.9, blue: 0.4, alpha: 1.0).setFill()
    NSBezierPath(ovalIn: lightRect).fill()

    // Specular highlight
    let hlSize = lightSize * 0.35
    let hlRect = NSRect(
        x: (s - hlSize) / 2 - lightSize * 0.1,
        y: centerY + lightSize * 0.1,
        width: hlSize,
        height: hlSize
    )
    NSColor(red: 0.7, green: 1.0, blue: 0.8, alpha: 0.5).setFill()
    NSBezierPath(ovalIn: hlRect).fill()

    // Stem / base
    let stemW = s * 0.14
    let stemH = s * 0.07
    let stemRect = NSRect(
        x: (s - stemW) / 2,
        y: centerY - lightSize / 2 - stemH + s * 0.01,
        width: stemW,
        height: stemH
    )
    NSColor(red: 0.55, green: 0.55, blue: 0.6, alpha: 1.0).setFill()
    NSBezierPath(roundedRect: stemRect, xRadius: s * 0.02, yRadius: s * 0.02).fill()

    img.unlockFocus()

    guard let tiff = img.tiffRepresentation,
          let rep = NSBitmapImageRep(data: tiff),
          let png = rep.representation(using: .png, properties: [:]) else {
        fatalError("Failed to create PNG for \(name)")
    }
    try! png.write(to: URL(fileURLWithPath: "\(iconsetDir)/\(name)"))
}

print("Generated iconset at \(iconsetDir)")
