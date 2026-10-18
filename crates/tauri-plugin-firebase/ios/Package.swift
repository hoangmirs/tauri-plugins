// swift-tools-version:5.9

import PackageDescription

let package = Package(
  name: "tauri-plugin-firebase",
  platforms: [
    .iOS(.v15)
  ],
  products: [
    .library(
      name: "tauri-plugin-firebase",
      type: .static,
      targets: ["tauri-plugin-firebase"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api"),
    .package(url: "https://github.com/firebase/firebase-ios-sdk", .upToNextMajor(from: "13.0.1")),
  ],
  targets: [
    .target(
      name: "tauri-plugin-firebase",
      dependencies: [
        .byName(name: "Tauri"),
        .product(name: "FirebaseRemoteConfig", package: "firebase-ios-sdk"),
        .product(name: "FirebaseAnalyticsCore", package: "firebase-ios-sdk"),
        .product(name: "FirebaseCrashlytics", package: "firebase-ios-sdk"),
      ],
      path: "Sources")
  ]
)
