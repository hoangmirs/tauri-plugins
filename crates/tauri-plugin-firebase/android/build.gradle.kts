plugins {
  id("com.android.library")
  id("org.jetbrains.kotlin.android")
}

android {
  namespace = "dev.hoangmirs.firebase"
  compileSdk = 36

  defaultConfig {
    minSdk = 24
  }

  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
  kotlinOptions {
    jvmTarget = "17"
    // Firebase's measurement jars carry Kotlin 2.4 metadata, and a Tauri app's
    // Android project compiles with Kotlin 1.9, which refuses to read it. The
    // plugin calls only Firebase's Java surface, so skipping the check is safe
    // here. Raising Kotlin in the app instead breaks every plugin that still
    // sets `kotlinOptions`, which Kotlin 2.2 and later reject.
    freeCompilerArgs += listOf("-Xskip-metadata-version-check")
  }
}

dependencies {
  implementation(project(":tauri-android"))
  implementation(platform("com.google.firebase:firebase-bom:34.19.0"))
  implementation("com.google.firebase:firebase-config")
  implementation("com.google.firebase:firebase-analytics")
}
