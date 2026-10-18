// swift-tools-version:5.9

import PackageDescription

let package = Package(
  name: "tauri-plugin-ads",
  platforms: [
    .iOS(.v15)
  ],
  products: [
    .library(
      name: "tauri-plugin-ads",
      type: .static,
      targets: ["tauri-plugin-ads"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api"),
    .package(
      url: "https://github.com/googleads/swift-package-manager-google-mobile-ads",
      .upToNextMajor(from: "12.14.0")),
  ],
  targets: [
    .target(
      name: "tauri-plugin-ads",
      dependencies: [
        .byName(name: "Tauri"),
        .product(name: "GoogleMobileAds", package: "swift-package-manager-google-mobile-ads"),
      ],
      path: "Sources")
  ]
)
