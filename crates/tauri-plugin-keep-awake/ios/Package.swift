// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-keep-awake",
  platforms: [
    .iOS(.v14)
  ],
  products: [
    .library(
      name: "tauri-plugin-keep-awake",
      type: .static,
      targets: ["tauri-plugin-keep-awake"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-keep-awake",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
