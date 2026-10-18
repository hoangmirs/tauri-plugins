// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-purchase",
  platforms: [
    .iOS(.v15)
  ],
  products: [
    .library(
      name: "tauri-plugin-purchase",
      type: .static,
      targets: ["tauri-plugin-purchase"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-purchase",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
