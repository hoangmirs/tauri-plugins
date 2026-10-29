// swift-tools-version:5.9

import PackageDescription

let package = Package(
  name: "tauri-plugin-sign-in",
  platforms: [
    .iOS(.v15)
  ],
  products: [
    .library(
      name: "tauri-plugin-sign-in",
      type: .static,
      targets: ["tauri-plugin-sign-in"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-sign-in",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
