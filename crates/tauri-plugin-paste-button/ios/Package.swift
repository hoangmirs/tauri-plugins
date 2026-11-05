// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-paste-button",
  platforms: [
    .iOS(.v14)
  ],
  products: [
    .library(
      name: "tauri-plugin-paste-button",
      type: .static,
      targets: ["tauri-plugin-paste-button"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-paste-button",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
