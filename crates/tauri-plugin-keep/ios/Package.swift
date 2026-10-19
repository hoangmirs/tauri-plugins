// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-keep",
  platforms: [
    .iOS(.v14)
  ],
  products: [
    .library(
      name: "tauri-plugin-keep",
      type: .static,
      targets: ["tauri-plugin-keep"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-keep",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
