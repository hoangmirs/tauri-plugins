// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-store-review",
  platforms: [
    .iOS(.v14)
  ],
  products: [
    .library(
      name: "tauri-plugin-store-review",
      type: .static,
      targets: ["tauri-plugin-store-review"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-store-review",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
