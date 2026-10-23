// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-vault",
  platforms: [
    .iOS(.v14)
  ],
  products: [
    .library(
      name: "tauri-plugin-vault",
      type: .static,
      targets: ["tauri-plugin-vault"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-vault",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
