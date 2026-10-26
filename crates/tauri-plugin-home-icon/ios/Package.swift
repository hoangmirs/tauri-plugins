// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-home-icon",
  platforms: [
    .iOS(.v14)
  ],
  products: [
    .library(
      name: "tauri-plugin-home-icon",
      type: .static,
      targets: ["tauri-plugin-home-icon"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-home-icon",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
