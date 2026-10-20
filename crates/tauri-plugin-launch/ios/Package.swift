// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-launch",
  platforms: [
    .iOS(.v14)
  ],
  products: [
    .library(
      name: "tauri-plugin-launch",
      type: .static,
      targets: ["tauri-plugin-launch"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-launch",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
