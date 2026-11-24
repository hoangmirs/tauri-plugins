// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-sensors",
  platforms: [
    .iOS(.v14)
  ],
  products: [
    .library(
      name: "tauri-plugin-sensors",
      type: .static,
      targets: ["tauri-plugin-sensors"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-sensors",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
